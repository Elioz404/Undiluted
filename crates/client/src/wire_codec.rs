//! Converts the JSON format at the edge of the enclave into client types.

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use undiluted_ledger::PublicKey;
use undiluted_service::wire::{Input, Output};

use crate::error::ClientError;
use crate::transport::{EnclaveIdentity, Exchange, decode_key};

pub(crate) fn request_input(envelope: &[u8]) -> Input {
    Input::Request {
        envelope: STANDARD.encode(envelope),
    }
}

pub(crate) fn into_identity(output: Output) -> Result<EnclaveIdentity, ClientError> {
    match output {
        Output::Identity {
            encryption_key,
            signing_key,
        } => Ok(EnclaveIdentity {
            encryption_key: decode_key(&encryption_key)?,
            signing_key: PublicKey::from_bytes(decode_key(&signing_key)?),
        }),
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
