//! Checks the enclave's attestation in the browser, with the same code the CLI
//! uses. The page gathers the evidence from public sources and this decides.

use serde::{Deserialize, Serialize};
use undiluted_attestation::{Expected, Measurements, RunAttestation, verify};

use crate::error::WebError;
use crate::protocol::parse_identity;

/// Everything the check reads, as the page gathered it.
#[derive(Debug, Deserialize)]
pub struct EnclaveEvidence {
    /// The run's attestation, as `OutLayer` publishes it.
    pub attestation: RunAttestation,
    /// The run's output as NEAR returned it, base64.
    pub output: String,
    /// The wasm the site expects, as sha256 hex.
    pub wasm_hash: String,
    /// From `OutLayer`'s register contract on NEAR.
    pub approved: Vec<Measurements>,
    pub collaterals: Vec<String>,
    pub now_secs: u64,
}

/// What the check established, for the page to show.
#[derive(Debug, Serialize)]
pub struct EnclaveFindings {
    pub tcb_status: String,
    pub measurements: Measurements,
    pub wasm_hash: String,
    pub output_hash: String,
    pub signing_key: String,
    pub encryption_key: String,
}

pub fn verify_enclave_run(evidence: &str) -> Result<EnclaveFindings, WebError> {
    use base64::Engine;
    use base64::engine::general_purpose::STANDARD;

    let evidence: EnclaveEvidence =
        serde_json::from_str(evidence).map_err(|error| WebError::input(&error))?;
    let output = STANDARD
        .decode(&evidence.output)
        .map_err(|_| WebError::Input("the output is not base64".to_owned()))?;
    let verified = verify(
        &evidence.attestation,
        &Expected {
            wasm_hash: &evidence.wasm_hash,
            output: &output,
            approved: &evidence.approved,
            collaterals: &evidence.collaterals,
            now_secs: evidence.now_secs,
        },
    )
    .map_err(|error| WebError::Attestation(error.to_string()))?;

    let identity = std::str::from_utf8(&output)
        .map_err(|_| WebError::Attestation("the attested output is not text".to_owned()))?;
    let (encryption_key, signing_key) = parse_identity(identity)?;
    Ok(EnclaveFindings {
        tcb_status: verified.tcb_status,
        measurements: verified.measurements,
        wasm_hash: verified.wasm_hash,
        output_hash: verified.output_hash,
        signing_key: hex::encode(signing_key.as_bytes()),
        encryption_key: hex::encode(encryption_key),
    })
}
