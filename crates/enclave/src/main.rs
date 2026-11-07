//! Runs the service inside an `OutLayer` enclave. All this binary does is connect
//! the platform to the ports the service defines.

mod platform;

use outlayer::env;
use undiluted_envelope::EnclaveSecret;
use undiluted_service::wire::{self, Input, Output};
use undiluted_service::{Gateway, Service};

use platform::{NearChain, SealedStore, TeeSigner};

/// Embedded in a custom section, so the wasm hash also covers these keys.
#[used]
#[unsafe(link_section = "outlayer.manifest")]
static OUTLAYER_MANIFEST: [u8; include_bytes!("../manifest.json").len()] =
    *include_bytes!("../manifest.json");

const SECRET_VARIABLE: &str = "PROTECTED_ENCLAVE_KEY";

fn execute() -> Result<Output, String> {
    let input: Input = env::input_json()
        .map_err(|error| format!("invalid input: {error}"))?
        .ok_or("missing input")?;
    let secret = load_secret()?;
    let service = Service::new(SealedStore, TeeSigner, NearChain);
    Ok(wire::run(&mut Gateway::new(secret, service), &input))
}

/// The key is generated inside the TEE, so no person ever sees it.
fn load_secret() -> Result<EnclaveSecret, String> {
    let encoded = env::var(SECRET_VARIABLE).ok_or("the enclave secret is not attached")?;
    let bytes: [u8; 32] = hex::decode(encoded)
        .ok()
        .and_then(|bytes| bytes.try_into().ok())
        .ok_or("the enclave secret is malformed")?;
    Ok(EnclaveSecret::from_bytes(bytes))
}

fn main() {
    let output = execute().unwrap_or_else(|message| Output::Error { message });
    let printed = env::output_json(&output);
    assert!(printed.is_ok(), "output could not be written");
}
