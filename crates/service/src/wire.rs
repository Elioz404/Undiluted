//! The JSON format at the edge of the enclave.
//!
//! Everything here is visible to whoever relays a call, so it holds only
//! ciphertext, public keys and the head attestation.

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use serde::{Deserialize, Serialize};

use crate::chain::ChainReader;
use crate::gateway::Gateway;
use crate::ports::{EnclaveSigner, Store};
use crate::protocol::HeadAttestation;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "operation", rename_all = "snake_case")]
pub enum Input {
    Identity,
    Request { envelope: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum Output {
    /// The encryption key comes with the signing key's signature over it.
    Identity {
        encryption_key: String,
        signing_key: String,
        encryption_key_signature: String,
    },
    Reply {
        envelope: String,
        anchor: Option<AnchorOutput>,
    },
    Error {
        message: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AnchorOutput {
    pub company: String,
    pub sequence: u64,
    pub head: String,
    pub signature: String,
}

impl From<HeadAttestation> for AnchorOutput {
    fn from(attestation: HeadAttestation) -> Self {
        Self {
            company: attestation.company.to_string(),
            sequence: attestation.sequence,
            head: hex::encode(attestation.head.as_bytes()),
            signature: hex::encode(attestation.signature.as_bytes()),
        }
    }
}

/// Runs one call. Platforms only supply the input and print the output.
pub fn run<S: Store, K: EnclaveSigner, C: ChainReader>(
    gateway: &mut Gateway<S, K, C>,
    input: &Input,
) -> Output {
    match input {
        Input::Identity => identity(gateway),
        Input::Request { envelope } => request(gateway, envelope),
    }
}

fn identity<S: Store, K: EnclaveSigner, C: ChainReader>(gateway: &Gateway<S, K, C>) -> Output {
    let signed = gateway
        .signing_key()
        .and_then(|key| Ok((key, gateway.encryption_key_signature()?)));
    match signed {
        Ok((signing_key, signature)) => Output::Identity {
            encryption_key: hex::encode(gateway.encryption_key()),
            signing_key: hex::encode(signing_key.as_bytes()),
            encryption_key_signature: hex::encode(signature.as_bytes()),
        },
        Err(error) => Output::Error {
            message: error.to_string(),
        },
    }
}

fn request<S: Store, K: EnclaveSigner, C: ChainReader>(
    gateway: &mut Gateway<S, K, C>,
    envelope: &str,
) -> Output {
    let Ok(envelope) = STANDARD.decode(envelope) else {
        return Output::Error {
            message: "envelope is not valid base64".to_owned(),
        };
    };
    match gateway.process(&envelope) {
        Ok(output) => Output::Reply {
            envelope: STANDARD.encode(output.envelope),
            anchor: output.anchor.map(AnchorOutput::from),
        },
        Err(error) => Output::Error {
            message: error.to_string(),
        },
    }
}
