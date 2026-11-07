use undiluted_ledger::PublicKey;
use undiluted_service::wire::AnchorOutput;

use crate::error::ClientError;

/// The public keys of an enclave deployment.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EnclaveIdentity {
    pub encryption_key: [u8; 32],
    pub signing_key: PublicKey,
}

/// One answer from the enclave.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Exchange {
    pub envelope: Vec<u8>,
    pub anchor: Option<AnchorOutput>,
}

pub trait Transport {
    fn identity(&mut self) -> Result<EnclaveIdentity, ClientError>;

    fn submit(&mut self, envelope: &[u8]) -> Result<Exchange, ClientError>;
}

pub(crate) fn decode_key(value: &str) -> Result<[u8; 32], ClientError> {
    hex::decode(value)
        .ok()
        .and_then(|bytes| <[u8; 32]>::try_from(bytes).ok())
        .ok_or_else(|| ClientError::Protocol("invalid key in identity".to_owned()))
}
