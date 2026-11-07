//! The boundary of the enclave: opens a request, handles it, seals the reply.

use borsh::{from_slice, to_vec};
use undiluted_envelope::{EnclaveSecret, ReplyKey, open_request};
use undiluted_ledger::PublicKey;

use crate::chain::ChainReader;
use crate::error::ServiceError;
use crate::ports::{EnclaveSigner, Store, StoreError};
use crate::protocol::{HeadAttestation, SignedRequest};
use crate::service::Service;

/// What leaves the enclave. Only the anchor is readable by whoever relays it,
/// because the relay needs it to publish the head on chain.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GatewayOutput {
    pub envelope: Vec<u8>,
    pub anchor: Option<HeadAttestation>,
}

#[derive(Debug)]
pub struct Gateway<S, K, C> {
    secret: EnclaveSecret,
    service: Service<S, K, C>,
}

impl<S: Store, K: EnclaveSigner, C: ChainReader> Gateway<S, K, C> {
    pub const fn new(secret: EnclaveSecret, service: Service<S, K, C>) -> Self {
        Self { secret, service }
    }

    /// The key clients encrypt their requests to.
    #[must_use]
    pub fn encryption_key(&self) -> [u8; 32] {
        self.secret.public_key()
    }

    /// The key clients check replies against.
    pub fn signing_key(&self) -> Result<PublicKey, StoreError> {
        self.service.signing_key()
    }

    /// Fails only when no authentic reply can be produced at all: an envelope
    /// that does not open, or storage that does not answer.
    pub fn process(&mut self, envelope: &[u8]) -> Result<GatewayOutput, GatewayError> {
        let (plaintext, reply_key) = open_request(&self.secret, envelope)?;
        let request: SignedRequest = from_slice(&plaintext).map_err(|_| GatewayError::Malformed)?;
        let reply = self.service.handle(&request)?;
        let anchor = reply.anchor.clone();
        Ok(GatewayOutput {
            envelope: seal_reply(&reply_key, &reply)?,
            anchor,
        })
    }
}

fn seal_reply(key: &ReplyKey, reply: &crate::protocol::Reply) -> Result<Vec<u8>, GatewayError> {
    let encoded = to_vec(reply).map_err(|_| ServiceError::Encoding)?;
    Ok(key.seal(&encoded)?)
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum GatewayError {
    #[error(transparent)]
    Envelope(#[from] undiluted_envelope::EnvelopeError),
    #[error(transparent)]
    Service(#[from] ServiceError),
    #[error("request could not be decoded")]
    Malformed,
}
