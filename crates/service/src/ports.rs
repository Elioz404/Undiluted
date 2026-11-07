use thiserror::Error;
use undiluted_ledger::{PublicKey, SignatureBytes};

#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[error("storage failed: {0}")]
pub struct StoreError(pub String);

/// Confidential key-value storage that survives between runs.
pub trait Store {
    fn get(&self, key: &str) -> Result<Option<Vec<u8>>, StoreError>;

    fn put(&mut self, key: &str, value: &[u8]) -> Result<(), StoreError>;

    /// Writes only if `key` is free and reports whether it did. This is what
    /// stops two concurrent runs from forking a company's history.
    fn put_new(&mut self, key: &str, value: &[u8]) -> Result<bool, StoreError>;

    fn delete(&mut self, key: &str) -> Result<(), StoreError>;
}

/// The enclave's signing identity.
pub trait EnclaveSigner {
    fn public_key(&self) -> Result<PublicKey, StoreError>;

    fn sign(&self, message: &[u8]) -> Result<SignatureBytes, StoreError>;
}
