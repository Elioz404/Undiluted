//! Checks a deployed enclave from public sources only: `OutLayer`'s attestation
//! of one of its runs, the run's output as NEAR recorded it, and the Intel
//! collateral and approved measurements `OutLayer` keeps on chain.

use std::time::{SystemTime, UNIX_EPOCH};

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use undiluted_attestation::{Expected, Measurements, RunAttestation, Verified, verify};
use undiluted_service::wire::Output;

use crate::error::ClientError;
use crate::near_cli::view;
use crate::transport::EnclaveIdentity;
use crate::wire_codec::into_identity;

/// Where the public evidence is read from.
#[derive(Debug, Clone)]
pub struct EnclaveCheckConfig {
    pub rpc_url: String,
    /// `OutLayer`'s API, which serves each run's attestation.
    pub attestation_api: String,
    /// `OutLayer`'s register contract, which holds Intel's collateral and the
    /// measurements of the worker images it approved.
    pub register_contract: String,
}

/// An `identity` run whose attestation checked out, and the keys it produced.
#[derive(Debug, Clone)]
pub struct CheckedEnclave {
    pub verified: Verified,
    pub identity: EnclaveIdentity,
}

/// Verifies that `transaction`, sent by `sender`, ran the wasm `wasm_hash` in
/// an attested enclave and returned the enclave's identity.
pub fn check_enclave(
    config: &EnclaveCheckConfig,
    transaction: &str,
    sender: &str,
    wasm_hash: &str,
) -> Result<CheckedEnclave, ClientError> {
    let attestation = fetch_attestation(config, transaction)?;
    let output = attested_output(config, transaction, sender, &attestation.output_hash)?;
    let approved: Vec<Measurements> = serde_json::from_value(view(
        &config.rpc_url,
        &config.register_contract,
        "get_approved_measurements",
        &json!({}),
    )?)
    .map_err(|error| ClientError::Attestation(error.to_string()))?;
    let collaterals: Vec<String> = serde_json::from_value(view(
        &config.rpc_url,
        &config.register_contract,
        "get_collaterals",
        &json!({}),
    )?)
    .map_err(|error| ClientError::Attestation(error.to_string()))?;
    let now_secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|error| ClientError::Attestation(error.to_string()))?
        .as_secs();

    let verified = verify(
        &attestation,
        &Expected {
            wasm_hash,
            output: &output,
            approved: &approved,
            collaterals: &collaterals,
            now_secs,
        },
    )
    .map_err(|error| ClientError::Attestation(error.to_string()))?;

    let output: Output = serde_json::from_slice(&output).map_err(|error| {
        ClientError::Attestation(format!("the run's output is not an identity: {error}"))
    })?;
    Ok(CheckedEnclave {
        verified,
        identity: into_identity(output)?,
    })
}

fn fetch_attestation(
    config: &EnclaveCheckConfig,
    transaction: &str,
) -> Result<RunAttestation, ClientError> {
    let url = format!(
        "{}/attestations/by-tx/{transaction}",
        config.attestation_api.trim_end_matches('/')
    );
    ureq::get(&url)
        .call()
        .map_err(|error| ClientError::Transport(error.to_string()))?
        .into_json()
        .map_err(|error| ClientError::Attestation(error.to_string()))
}

/// The value a receipt of the transaction returned whose hash is `output_hash`.
/// The attestation check then proves that this hash is the one the enclave
/// committed to.
fn attested_output(
    config: &EnclaveCheckConfig,
    transaction: &str,
    sender: &str,
    output_hash: &str,
) -> Result<Vec<u8>, ClientError> {
    let request = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "EXPERIMENTAL_tx_status",
        "params": { "tx_hash": transaction, "sender_account_id": sender, "wait_until": "FINAL" },
    });
    let answer: Value = ureq::post(&config.rpc_url)
        .send_json(request)
        .map_err(|error| ClientError::Transport(error.to_string()))?
        .into_json()
        .map_err(|error| ClientError::Transport(error.to_string()))?;
    answer["result"]["receipts_outcome"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|receipt| receipt["outcome"]["status"]["SuccessValue"].as_str())
        .filter_map(|value| STANDARD.decode(value).ok())
        .find(|value| hex::encode(Sha256::digest(value)).eq_ignore_ascii_case(output_hash))
        .ok_or_else(|| {
            ClientError::Attestation("the transaction returned no attested output".to_owned())
        })
}
