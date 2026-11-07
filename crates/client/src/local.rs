use ed25519_dalek::{Signer, SigningKey};
use undiluted_domain::Money;
use undiluted_envelope::EnclaveSecret;
use undiluted_ledger::{PublicKey, SignatureBytes};
use undiluted_service::wire::{Input, run};
use undiluted_service::{
    ChainError, ChainReader, EnclaveSigner, Gateway, MemoryStore, Service, StoreError,
    TokenTransfer,
};

use crate::error::ClientError;
use crate::party::random_bytes;
use crate::payments::LOCAL_REFERENCE_PREFIX;
use crate::transport::{EnclaveIdentity, Exchange, Transport};
use crate::wire_codec::{into_exchange, into_identity, request_input};

#[derive(Debug)]
struct LocalSigner(SigningKey);

impl EnclaveSigner for LocalSigner {
    fn public_key(&self) -> Result<PublicKey, StoreError> {
        Ok(PublicKey::from_bytes(self.0.verifying_key().to_bytes()))
    }

    fn sign(&self, message: &[u8]) -> Result<SignatureBytes, StoreError> {
        Ok(SignatureBytes::from_bytes(self.0.sign(message).to_bytes()))
    }
}

/// The treasury and token every local payment arrives at.
pub const LOCAL_TREASURY: &str = "treasury.local";
pub const LOCAL_TOKEN: &str = "usdc.local";

/// Stands in for the chain: only references the local rail made are real.
#[derive(Debug)]
struct LocalChain;

impl ChainReader for LocalChain {
    fn token_transfers(&self, reference: &str) -> Result<Vec<TokenTransfer>, ChainError> {
        reference
            .strip_prefix(LOCAL_REFERENCE_PREFIX)
            .and_then(|rest| rest.split(':').next())
            .and_then(|amount| amount.parse().ok())
            .map(|amount| {
                vec![TokenTransfer {
                    token: LOCAL_TOKEN.to_owned(),
                    to: LOCAL_TREASURY.to_owned(),
                    amount: Money::new(amount),
                }]
            })
            .ok_or_else(|| ChainError("no such transaction".to_owned()))
    }
}

/// Runs the real enclave code in this process, without a TEE or any network.
/// It proves the rules work, not that anything is confidential.
#[derive(Debug)]
pub struct LocalTransport(Gateway<MemoryStore, LocalSigner, LocalChain>);

impl LocalTransport {
    pub fn new() -> Result<Self, ClientError> {
        let signer = LocalSigner(SigningKey::from_bytes(&random_bytes()?));
        let secret = EnclaveSecret::from_bytes(random_bytes()?);
        let service = Service::new(MemoryStore::default(), signer, LocalChain);
        Ok(Self(Gateway::new(secret, service)))
    }
}

impl Transport for LocalTransport {
    fn identity(&mut self) -> Result<EnclaveIdentity, ClientError> {
        into_identity(run(&mut self.0, &Input::Identity))
    }

    fn submit(&mut self, envelope: &[u8]) -> Result<Exchange, ClientError> {
        into_exchange(run(&mut self.0, &request_input(envelope)))
    }
}
