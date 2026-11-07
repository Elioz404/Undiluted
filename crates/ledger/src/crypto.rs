//! Digests and Ed25519 signatures, the same key type NEAR accounts use.

use core::fmt;

use borsh::{BorshDeserialize, BorshSerialize};
use ed25519_dalek::{Signature, VerifyingKey};
use sha2::{Digest as _, Sha256};

#[derive(Clone, Copy, PartialEq, Eq, Hash, BorshSerialize, BorshDeserialize)]
pub struct Digest([u8; 32]);

impl Digest {
    /// Hashes `payload` under a domain tag, so a digest made for one purpose
    /// can never be replayed as if it were made for another.
    #[must_use]
    pub fn tagged(tag: &str, payload: &[u8]) -> Self {
        let mut hasher = Sha256::new();
        hasher.update((tag.len() as u64).to_le_bytes());
        hasher.update(tag.as_bytes());
        hasher.update(payload);
        Self(hasher.finalize().into())
    }

    #[must_use]
    pub const fn from_bytes(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    #[must_use]
    pub const fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }
}

impl fmt::Debug for Digest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for byte in &self.0 {
            write!(f, "{byte:02x}")?;
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct PublicKey([u8; 32]);

impl PublicKey {
    #[must_use]
    pub const fn from_bytes(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    #[must_use]
    pub const fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }

    /// Whether `signature` is a valid signature of `digest` by this key.
    /// Malformed keys and non-canonical signatures simply fail.
    #[must_use]
    pub fn verifies(&self, digest: &Digest, signature: &SignatureBytes) -> bool {
        VerifyingKey::from_bytes(&self.0).is_ok_and(|key| {
            key.verify_strict(digest.as_bytes(), &Signature::from_bytes(&signature.0))
                .is_ok()
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct SignatureBytes([u8; 64]);

impl SignatureBytes {
    #[must_use]
    pub const fn from_bytes(bytes: [u8; 64]) -> Self {
        Self(bytes)
    }

    #[must_use]
    pub const fn as_bytes(&self) -> &[u8; 64] {
        &self.0
    }
}

#[cfg(test)]
mod tests {
    use ed25519_dalek::{Signer, SigningKey};

    use super::*;

    fn key(seed: u8) -> SigningKey {
        SigningKey::from_bytes(&[seed; 32])
    }

    fn public(key: &SigningKey) -> PublicKey {
        PublicKey::from_bytes(key.verifying_key().to_bytes())
    }

    #[test]
    fn digests_depend_on_the_tag() {
        assert_ne!(Digest::tagged("a", b"x"), Digest::tagged("b", b"x"));
    }

    #[test]
    fn tag_and_payload_cannot_be_shifted_into_each_other() {
        assert_ne!(Digest::tagged("ab", b"c"), Digest::tagged("a", b"bc"));
    }

    #[test]
    fn signature_verifies_only_for_its_key_and_digest() {
        let digest = Digest::tagged("test", b"payload");
        let signer = key(1);
        let signature = SignatureBytes::from_bytes(signer.sign(digest.as_bytes()).to_bytes());

        assert!(public(&signer).verifies(&digest, &signature));
        assert!(!public(&key(2)).verifies(&digest, &signature));
        assert!(!public(&signer).verifies(&Digest::tagged("test", b"other"), &signature));
    }
}
