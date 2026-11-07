use ed25519_dalek::{Signer, SigningKey};
use undiluted_envelope::EnclaveSecret;
use undiluted_ledger::{PublicKey, SignatureBytes};
use undiluted_service::wire::{Input, run};
use undiluted_service::{EnclaveSigner, Gateway, MemoryStore, Service, StoreError};

use crate::error::ClientError;
use crate::party::random_bytes;
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

/// Runs the real enclave code in this process, without a TEE or any network.
/// It proves the rules work, not that anything is confidential.
#[derive(Debug)]
pub struct LocalTransport(Gateway<MemoryStore, LocalSigner>);

impl LocalTransport {
    pub fn new() -> Result<Self, ClientError> {
        let signer = LocalSigner(SigningKey::from_bytes(&random_bytes()?));
        let secret = EnclaveSecret::from_bytes(random_bytes()?);
        let service = Service::new(MemoryStore::default(), signer);
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
