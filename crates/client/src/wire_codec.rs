//! Converts the JSON format at the edge of the enclave into client types.

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use undiluted_ledger::{PublicKey, SignatureBytes};
use undiluted_service::identity_digest;
use undiluted_service::wire::{Input, Output};

use crate::error::ClientError;
use crate::transport::{EnclaveIdentity, Exchange, decode_key};

pub(crate) fn request_input(envelope: &[u8]) -> Input {
    Input::Request {
        envelope: STANDARD.encode(envelope),
    }
}

/// Accepts an identity only if its signing key vouches for its encryption key.
pub(crate) fn into_identity(output: Output) -> Result<EnclaveIdentity, ClientError> {
    match output {
        Output::Identity {
            encryption_key,
            signing_key,
            encryption_key_signature,
        } => {
            let encryption_key = decode_key(&encryption_key)?;
            let signing_key = PublicKey::from_bytes(decode_key(&signing_key)?);
            let signature = hex::decode(&encryption_key_signature)
                .ok()
                .and_then(|bytes| <[u8; 64]>::try_from(bytes).ok())
                .map(SignatureBytes::from_bytes)
                .ok_or_else(|| ClientError::Protocol("malformed identity signature".to_owned()))?;
            if !signing_key.verifies(&identity_digest(&encryption_key), &signature) {
                return Err(ClientError::UnauthenticReply);
            }
            Ok(EnclaveIdentity {
                encryption_key,
                signing_key,
            })
        }
        Output::Error { message } => Err(ClientError::Enclave(message)),
        Output::Reply { .. } => Err(ClientError::Protocol("expected an identity".to_owned())),
    }
}

pub(crate) fn into_exchange(output: Output) -> Result<Exchange, ClientError> {
    match output {
        Output::Reply { envelope, anchor } => Ok(Exchange {
            envelope: STANDARD
                .decode(envelope)
                .map_err(|_| ClientError::Protocol("reply is not valid base64".to_owned()))?,
            anchor,
        }),
        Output::Error { message } => Err(ClientError::Enclave(message)),
        Output::Identity { .. } => Err(ClientError::Protocol("expected a reply".to_owned())),
    }
}
