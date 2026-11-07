use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use borsh::{from_slice, to_vec};
use ed25519_dalek::{Signer, SigningKey};
use serde::{Deserialize, Serialize};
use undiluted_domain::CompanyId;
use undiluted_envelope::{ReplyKey, seal_request};
use undiluted_ledger::{PublicKey, SignatureBytes};
use undiluted_service::{Command, Reply, SignedRequest};

use crate::dto::{ActorDto, CommandDto, ResponseDto, decode_digest, decode_key};
use crate::error::WebError;

/// The enclave's public keys, as its identity call returns them.
#[derive(Debug, Clone, Deserialize)]
struct Identity {
    encryption_key: String,
    signing_key: String,
    encryption_key_signature: String,
}

/// A request ready to send, with what is needed to read its reply.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PreparedRequest {
    /// Base64, to put in the enclave input.
    pub envelope: String,
    /// Hex. Reads the reply; useless for anything else.
    pub reply_key: String,
    /// Hex digest of the signed request, which the reply must answer.
    pub request: String,
}

fn random<const N: usize>() -> Result<[u8; N], WebError> {
    let mut bytes = [0u8; N];
    getrandom::getrandom(&mut bytes).map_err(|_| WebError::Randomness)?;
    Ok(bytes)
}

pub(crate) fn new_party() -> Result<String, WebError> {
    Ok(hex::encode(random::<32>()?))
}

fn signing_key(secret: &str) -> Result<SigningKey, WebError> {
    Ok(SigningKey::from_bytes(&decode_key(secret)?))
}

fn public_key(key: &SigningKey) -> PublicKey {
    PublicKey::from_bytes(key.verifying_key().to_bytes())
}

pub fn party_public_key(secret: &str) -> Result<String, WebError> {
    Ok(hex::encode(signing_key(secret)?.verifying_key().to_bytes()))
}

/// The memo a payment for a subscription to `company`'s open round must carry,
/// for the holder whose public key is `key` (hex).
pub fn subscription_memo(company: &str, key: &str) -> Result<String, WebError> {
    Ok(undiluted_service::subscription_memo(
        &CompanyId::new(company)?,
        &PublicKey::from_bytes(decode_key(key)?),
    ))
}

/// Accepts an identity only if its signing key vouches for its encryption key,
/// so whoever relays it cannot substitute a key of their own.
fn parse_identity(identity: &str) -> Result<([u8; 32], PublicKey), WebError> {
    let identity: Identity =
        serde_json::from_str(identity).map_err(|error| WebError::input(&error))?;
    let encryption_key = decode_key(&identity.encryption_key)?;
    let signing_key = PublicKey::from_bytes(decode_key(&identity.signing_key)?);
    let signature = hex::decode(&identity.encryption_key_signature)
        .ok()
        .and_then(|bytes| <[u8; 64]>::try_from(bytes).ok())
        .map(SignatureBytes::from_bytes)
        .ok_or_else(|| WebError::Input("malformed identity signature".to_owned()))?;
    if !signing_key.verifies(
        &undiluted_service::identity_digest(&encryption_key),
        &signature,
    ) {
        return Err(WebError::UnauthenticReply);
    }
    Ok((encryption_key, signing_key))
}

fn into_command(command: CommandDto, key: &SigningKey) -> Result<Command, WebError> {
    Ok(match command {
        CommandDto::CreateCompany => Command::CreateCompany {
            key: public_key(key),
        },
        CommandDto::Propose { event } => Command::Propose {
            event: event.into_event()?,
        },
        CommandDto::Sign { digest } => {
            let digest = decode_digest(&digest)?;
            Command::Sign {
                digest,
                signature: SignatureBytes::from_bytes(key.sign(digest.as_bytes()).to_bytes()),
            }
        }
        CommandDto::CancelProposal => Command::CancelProposal,
        CommandDto::GetProposal => Command::GetProposal,
        CommandDto::GetCapTable => Command::GetCapTable,
        CommandDto::GetStatement => Command::GetStatement,
        CommandDto::GetPayoutPlan => Command::GetPayoutPlan,
        CommandDto::GetRound => Command::GetRound,
    })
}

/// Signs `command` as `actor` and encrypts it to the enclave in `identity`.
pub fn prepare_request(
    identity: &str,
    company: &str,
    actor: &str,
    command: &str,
    secret: &str,
) -> Result<PreparedRequest, WebError> {
    let (encryption_key, _) = parse_identity(identity)?;
    let key = signing_key(secret)?;
    let company = CompanyId::new(company)?;
    let actor = serde_json::from_str::<ActorDto>(actor)
        .map_err(|error| WebError::input(&error))?
        .into_actor()?;
    let command = into_command(
        serde_json::from_str(command).map_err(|error| WebError::input(&error))?,
        &key,
    )?;

    let id = random()?;
    let digest = SignedRequest::signing_digest(&id, &company, &actor, &command)?;
    let request = SignedRequest {
        id,
        company,
        actor,
        command,
        signature: SignatureBytes::from_bytes(key.sign(digest.as_bytes()).to_bytes()),
    };
    let encoded = to_vec(&request).map_err(|_| WebError::Encoding)?;
    let (envelope, reply_key) = seal_request(&encryption_key, random()?, &encoded)?;

    Ok(PreparedRequest {
        envelope: STANDARD.encode(envelope),
        reply_key: hex::encode(reply_key.to_bytes()),
        request: hex::encode(digest.as_bytes()),
    })
}

/// Opens a reply and accepts it only if the enclave signed it for this request.
pub fn open_reply(
    identity: &str,
    prepared: &PreparedRequest,
    envelope: &str,
) -> Result<ResponseDto, WebError> {
    let (_, signing_key) = parse_identity(identity)?;
    let ciphertext = STANDARD
        .decode(envelope)
        .map_err(|error| WebError::input(&error))?;
    let plaintext = ReplyKey::from_bytes(decode_key(&prepared.reply_key)?).open(&ciphertext)?;
    let reply: Reply = from_slice(&plaintext).map_err(|_| WebError::Encoding)?;

    if !reply.is_authentic(&signing_key) {
        return Err(WebError::UnauthenticReply);
    }
    if reply.request != decode_digest(&prepared.request)? {
        return Err(WebError::MismatchedReply);
    }
    Ok(ResponseDto::from_response(&reply.response))
}
