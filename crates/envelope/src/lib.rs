//! End-to-end encryption between a client and the enclave.
//!
//! Whatever carries a request (a relay, a public transaction) sees only
//! ciphertext. The reply is encrypted under a separate key derived from the same
//! exchange, which only the client that sent the request can derive.
//!
//! Both keys use a fixed nonce, which is safe only because each encrypts a
//! single plaintext: every request has a fresh ephemeral X25519 key, and the
//! service answers each request once, returning the stored reply to a replay.
//! A caller that could encrypt two different replies under one reply key would
//! break confidentiality.
//!
//! Messages are padded to 1 KiB blocks, so a ciphertext's length does not tell
//! which operation it carries. It still shows the size bracket of the content.

use chacha20poly1305::aead::{Aead, KeyInit};
use chacha20poly1305::{ChaCha20Poly1305, Key, Nonce};
use hkdf::Hkdf;
use sha2::Sha256;
use thiserror::Error;
use x25519_dalek::{PublicKey, StaticSecret};

const KEY_LENGTH: usize = 32;
const LENGTH_PREFIX: usize = 4;
const PADDING_BLOCK: usize = 1024;
const REQUEST_INFO: &[u8] = b"undiluted/envelope/request/v1";
const RESPONSE_INFO: &[u8] = b"undiluted/envelope/response/v1";

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum EnvelopeError {
    #[error("envelope is malformed")]
    Malformed,
    #[error("key agreement failed")]
    KeyAgreement,
    #[error("message could not be decrypted")]
    Decryption,
    #[error("message could not be encrypted")]
    Encryption,
}

/// The enclave's long-term decryption key.
#[derive(Clone)]
pub struct EnclaveSecret(StaticSecret);

impl core::fmt::Debug for EnclaveSecret {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str("EnclaveSecret(..)")
    }
}

impl EnclaveSecret {
    #[must_use]
    pub fn from_bytes(bytes: [u8; KEY_LENGTH]) -> Self {
        Self(StaticSecret::from(bytes))
    }

    /// The key clients encrypt requests to.
    #[must_use]
    pub fn public_key(&self) -> [u8; KEY_LENGTH] {
        PublicKey::from(&self.0).to_bytes()
    }
}

/// Encrypts the single reply to a request, or opens it on the client.
#[derive(Clone)]
pub struct ReplyKey([u8; KEY_LENGTH]);

impl core::fmt::Debug for ReplyKey {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str("ReplyKey(..)")
    }
}

impl ReplyKey {
    /// Restores a key kept between sending a request and reading its reply.
    #[must_use]
    pub const fn from_bytes(bytes: [u8; KEY_LENGTH]) -> Self {
        Self(bytes)
    }

    #[must_use]
    pub const fn to_bytes(&self) -> [u8; KEY_LENGTH] {
        self.0
    }

    pub fn seal(&self, plaintext: &[u8]) -> Result<Vec<u8>, EnvelopeError> {
        encrypt(&self.0, plaintext)
    }

    pub fn open(&self, ciphertext: &[u8]) -> Result<Vec<u8>, EnvelopeError> {
        decrypt(&self.0, ciphertext)
    }
}

/// Encrypts a request to the enclave. `ephemeral` must be fresh random bytes,
/// never reused: this crate stays free of randomness so it runs anywhere.
pub fn seal_request(
    enclave_key: &[u8; KEY_LENGTH],
    ephemeral: [u8; KEY_LENGTH],
    plaintext: &[u8],
) -> Result<(Vec<u8>, ReplyKey), EnvelopeError> {
    let secret = StaticSecret::from(ephemeral);
    let client_key = PublicKey::from(&secret).to_bytes();
    let (request_key, reply_key) = derive_keys(&secret, enclave_key, &client_key, enclave_key)?;

    let mut envelope = client_key.to_vec();
    envelope.extend(encrypt(&request_key, plaintext)?);
    Ok((envelope, ReplyKey(reply_key)))
}

/// Opens a request inside the enclave.
pub fn open_request(
    secret: &EnclaveSecret,
    envelope: &[u8],
) -> Result<(Vec<u8>, ReplyKey), EnvelopeError> {
    let (client_key, ciphertext) = envelope
        .split_at_checked(KEY_LENGTH)
        .ok_or(EnvelopeError::Malformed)?;
    let client_key: [u8; KEY_LENGTH] = client_key
        .try_into()
        .map_err(|_| EnvelopeError::Malformed)?;
    let enclave_key = secret.public_key();
    let (request_key, reply_key) = derive_keys(&secret.0, &client_key, &client_key, &enclave_key)?;
    Ok((decrypt(&request_key, ciphertext)?, ReplyKey(reply_key)))
}

fn derive_keys(
    own_secret: &StaticSecret,
    their_key: &[u8; KEY_LENGTH],
    client_key: &[u8; KEY_LENGTH],
    enclave_key: &[u8; KEY_LENGTH],
) -> Result<([u8; KEY_LENGTH], [u8; KEY_LENGTH]), EnvelopeError> {
    let shared = own_secret.diffie_hellman(&PublicKey::from(*their_key));
    if !shared.was_contributory() {
        return Err(EnvelopeError::KeyAgreement);
    }

    let mut salt = [0u8; KEY_LENGTH * 2];
    salt[..KEY_LENGTH].copy_from_slice(client_key);
    salt[KEY_LENGTH..].copy_from_slice(enclave_key);
    let hkdf = Hkdf::<Sha256>::new(Some(&salt), shared.as_bytes());

    let mut request_key = [0u8; KEY_LENGTH];
    let mut reply_key = [0u8; KEY_LENGTH];
    hkdf.expand(REQUEST_INFO, &mut request_key)
        .map_err(|_| EnvelopeError::KeyAgreement)?;
    hkdf.expand(RESPONSE_INFO, &mut reply_key)
        .map_err(|_| EnvelopeError::KeyAgreement)?;
    Ok((request_key, reply_key))
}

fn encrypt(key: &[u8; KEY_LENGTH], plaintext: &[u8]) -> Result<Vec<u8>, EnvelopeError> {
    cipher(key)
        .encrypt(&Nonce::default(), pad(plaintext)?.as_slice())
        .map_err(|_| EnvelopeError::Encryption)
}

fn decrypt(key: &[u8; KEY_LENGTH], ciphertext: &[u8]) -> Result<Vec<u8>, EnvelopeError> {
    let padded = cipher(key)
        .decrypt(&Nonce::default(), ciphertext)
        .map_err(|_| EnvelopeError::Decryption)?;
    unpad(&padded)
}

fn cipher(key: &[u8; KEY_LENGTH]) -> ChaCha20Poly1305 {
    ChaCha20Poly1305::new(Key::from_slice(key))
}

fn pad(plaintext: &[u8]) -> Result<Vec<u8>, EnvelopeError> {
    let length = u32::try_from(plaintext.len()).map_err(|_| EnvelopeError::Encryption)?;
    let total = (LENGTH_PREFIX + plaintext.len()).div_ceil(PADDING_BLOCK) * PADDING_BLOCK;
    let mut padded = Vec::with_capacity(total);
    padded.extend_from_slice(&length.to_le_bytes());
    padded.extend_from_slice(plaintext);
    padded.resize(total, 0);
    Ok(padded)
}

fn unpad(padded: &[u8]) -> Result<Vec<u8>, EnvelopeError> {
    let (prefix, rest) = padded
        .split_at_checked(LENGTH_PREFIX)
        .ok_or(EnvelopeError::Malformed)?;
    let prefix: [u8; LENGTH_PREFIX] = prefix.try_into().map_err(|_| EnvelopeError::Malformed)?;
    let length =
        usize::try_from(u32::from_le_bytes(prefix)).map_err(|_| EnvelopeError::Malformed)?;
    rest.get(..length)
        .map(<[u8]>::to_vec)
        .ok_or(EnvelopeError::Malformed)
}

#[cfg(test)]
mod tests {
    use super::*;

    const ENCLAVE: [u8; 32] = [7; 32];
    const EPHEMERAL: [u8; 32] = [9; 32];

    fn enclave() -> EnclaveSecret {
        EnclaveSecret::from_bytes(ENCLAVE)
    }

    #[test]
    fn the_enclave_opens_a_request_and_the_client_opens_the_reply() {
        let (envelope, client_reply_key) =
            seal_request(&enclave().public_key(), EPHEMERAL, b"terms").unwrap();

        let (plaintext, enclave_reply_key) = open_request(&enclave(), &envelope).unwrap();
        assert_eq!(plaintext, b"terms");

        let reply = enclave_reply_key.seal(b"statement").unwrap();
        assert_eq!(client_reply_key.open(&reply).unwrap(), b"statement");
    }

    #[test]
    fn another_enclave_cannot_open_a_request() {
        let (envelope, _) = seal_request(&enclave().public_key(), EPHEMERAL, b"terms").unwrap();
        assert_eq!(
            open_request(&EnclaveSecret::from_bytes([8; 32]), &envelope).unwrap_err(),
            EnvelopeError::Decryption
        );
    }

    #[test]
    fn a_reply_cannot_be_read_with_another_requests_key() {
        let key = enclave().public_key();
        let (envelope, _) = seal_request(&key, EPHEMERAL, b"a").unwrap();
        let (_, enclave_reply_key) = open_request(&enclave(), &envelope).unwrap();
        let (_, other_client_key) = seal_request(&key, [3; 32], b"b").unwrap();

        let reply = enclave_reply_key.seal(b"statement").unwrap();
        assert_eq!(
            other_client_key.open(&reply).unwrap_err(),
            EnvelopeError::Decryption
        );
    }

    #[test]
    fn a_tampered_envelope_is_rejected() {
        let (mut envelope, _) = seal_request(&enclave().public_key(), EPHEMERAL, b"terms").unwrap();
        let last = envelope.len() - 1;
        envelope[last] ^= 1;
        assert_eq!(
            open_request(&enclave(), &envelope).unwrap_err(),
            EnvelopeError::Decryption
        );
    }

    #[test]
    fn a_short_envelope_is_malformed() {
        assert_eq!(
            open_request(&enclave(), &[0; 8]).unwrap_err(),
            EnvelopeError::Malformed
        );
    }

    #[test]
    fn a_low_order_client_key_is_rejected() {
        let mut envelope = vec![0u8; 32];
        envelope.extend_from_slice(&[0; 64]);
        assert_eq!(
            open_request(&enclave(), &envelope).unwrap_err(),
            EnvelopeError::KeyAgreement
        );
    }

    #[test]
    fn messages_of_similar_size_look_identical_in_length() {
        let key = enclave().public_key();
        let (short, _) = seal_request(&key, EPHEMERAL, b"x").unwrap();
        let (long, _) = seal_request(&key, EPHEMERAL, &[1; 900]).unwrap();
        assert_eq!(short.len(), long.len());
    }

    #[test]
    fn empty_and_block_sized_messages_round_trip() {
        let key = enclave().public_key();
        for size in [0, PADDING_BLOCK - LENGTH_PREFIX, PADDING_BLOCK] {
            let message = vec![5; size];
            let (envelope, _) = seal_request(&key, EPHEMERAL, &message).unwrap();
            let (opened, _) = open_request(&enclave(), &envelope).unwrap();
            assert_eq!(opened, message);
        }
    }
}
