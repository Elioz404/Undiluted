use ed25519_dalek::{Signer, SigningKey};
use undiluted_ledger::{Digest, PublicKey, SignatureBytes};

use crate::error::ClientError;

/// Someone who signs: a company or a holder.
#[derive(Debug, Clone)]
pub struct Party(SigningKey);

impl Party {
    pub fn generate() -> Result<Self, ClientError> {
        Ok(Self(SigningKey::from_bytes(&random_bytes()?)))
    }

    #[must_use]
    pub fn public_key(&self) -> PublicKey {
        PublicKey::from_bytes(self.0.verifying_key().to_bytes())
    }

    #[must_use]
    pub fn sign(&self, digest: &Digest) -> SignatureBytes {
        SignatureBytes::from_bytes(self.0.sign(digest.as_bytes()).to_bytes())
    }
}

pub(crate) fn random_bytes<const N: usize>() -> Result<[u8; N], ClientError> {
    let mut bytes = [0u8; N];
    getrandom::getrandom(&mut bytes).map_err(|e| ClientError::Randomness(e.to_string()))?;
    Ok(bytes)
}
