//! Checks that a run on `OutLayer` really happened inside an Intel TDX enclave,
//! running a given wasm, and produced a given output.
//!
//! `OutLayer` publishes an attestation for every run: an Intel-signed TDX quote
//! whose `report_data` commits to the run's details, the wasm hash and the hash
//! of the output among them. Verifying one takes four steps, all done here with
//! no I/O, so the same code runs in a CLI and in a browser:
//!
//! 1. The quote is signed by Intel's attestation chain and the platform is up to
//!    date, as checked by `dcap-qvl` against Intel's collateral.
//! 2. The quote's five measurements (MRTD, RTMR0-3) are a set `OutLayer` approved
//!    on chain for its open-source worker image.
//! 3. The quote's `report_data` is the hash of the run's details, so none of
//!    them can be swapped.
//! 4. Those details name the expected wasm, and their output hash is the hash
//!    of the exact output the caller holds.
//!
//! The caller supplies the collateral and the approved measurements, read from
//! `OutLayer`'s register contract on NEAR, and the output, read from the chain.

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use dcap_qvl::QuoteCollateralV3;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use thiserror::Error;

/// The only platform state accepted, as `OutLayer`'s own register contract does.
const UP_TO_DATE: &str = "UpToDate";

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum AttestationError {
    #[error("the attestation is malformed: {0}")]
    Malformed(String),
    #[error("no collateral verifies this quote: {0}")]
    QuoteNotVerified(String),
    #[error("the platform is not up to date: {0}")]
    PlatformOutdated(String),
    #[error("the quote is not a TDX quote")]
    NotTdx,
    #[error("the enclave's measurements are not among those OutLayer approved")]
    MeasurementsNotApproved,
    #[error("the quote does not commit to this run's details")]
    ReportDataMismatch,
    #[error("the run executed wasm {actual}, not {expected}")]
    WrongWasm { expected: String, actual: String },
    #[error("the output does not match the one the enclave attested")]
    OutputMismatch,
}

/// A run's attestation as `OutLayer` publishes it at `/attestations/by-tx/{hash}`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RunAttestation {
    pub task_type: String,
    pub task_id: i64,
    pub tdx_quote: String,
    pub repo: Option<String>,
    pub commit: Option<String>,
    pub build_target: Option<String>,
    pub wasm_hash: Option<String>,
    pub executed_wasm_sha256: Option<String>,
    pub input_hash: Option<String>,
    pub output_hash: String,
    pub block_height: Option<u64>,
    pub caller_account_id: Option<String>,
    pub project_id: Option<String>,
    pub secrets_ref: Option<String>,
    pub timestamp: i64,
    pub attached_usd: Option<String>,
}

impl RunAttestation {
    /// What `OutLayer`'s worker puts in the first half of `report_data`.
    #[must_use]
    pub fn task_hash(&self) -> [u8; 32] {
        let mut hasher = Sha256::new();
        hasher.update(self.task_type.as_bytes());
        hasher.update(self.task_id.to_le_bytes());
        for field in [
            &self.repo,
            &self.commit,
            &self.build_target,
            &self.wasm_hash,
            &self.input_hash,
        ]
        .into_iter()
        .flatten()
        {
            hasher.update(field.as_bytes());
        }
        hasher.update(self.output_hash.as_bytes());
        if let Some(height) = self.block_height {
            hasher.update(height.to_le_bytes());
        }
        for field in [&self.caller_account_id, &self.project_id, &self.secrets_ref]
            .into_iter()
            .flatten()
        {
            hasher.update(field.as_bytes());
        }
        hasher.update(self.timestamp.to_le_bytes());
        if let Some(usd) = &self.attached_usd {
            hasher.update(usd.as_bytes());
        }
        hasher.finalize().into()
    }
}

/// The five TDX measurements of an enclave image, as hex.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Measurements {
    pub mrtd: String,
    pub rtmr0: String,
    pub rtmr1: String,
    pub rtmr2: String,
    pub rtmr3: String,
}

impl Measurements {
    fn matches(&self, other: &Self) -> bool {
        [
            (&self.mrtd, &other.mrtd),
            (&self.rtmr0, &other.rtmr0),
            (&self.rtmr1, &other.rtmr1),
            (&self.rtmr2, &other.rtmr2),
            (&self.rtmr3, &other.rtmr3),
        ]
        .into_iter()
        .all(|(left, right)| left.eq_ignore_ascii_case(right))
    }
}

/// What the check needs besides the attestation itself.
#[derive(Debug, Clone)]
pub struct Expected<'a> {
    /// The sha256 of the wasm that should have run, as hex.
    pub wasm_hash: &'a str,
    /// The run's output exactly as the chain returned it.
    pub output: &'a [u8],
    /// Measurement sets `OutLayer` approved on chain.
    pub approved: &'a [Measurements],
    /// Intel collateral as `OutLayer`'s register contract stores it: JSON with hex
    /// signatures and CRLs.
    pub collaterals: &'a [String],
    /// Seconds since the epoch, for certificate and collateral validity.
    pub now_secs: u64,
}

/// What a successful check established.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Verified {
    pub tcb_status: String,
    pub measurements: Measurements,
    pub wasm_hash: String,
    pub output_hash: String,
}

pub fn verify(
    attestation: &RunAttestation,
    expected: &Expected<'_>,
) -> Result<Verified, AttestationError> {
    let quote = STANDARD
        .decode(&attestation.tdx_quote)
        .map_err(|_| AttestationError::Malformed("the quote is not base64".to_owned()))?;
    let report = verify_quote(&quote, expected.collaterals, expected.now_secs)?;
    if report.status != UP_TO_DATE {
        return Err(AttestationError::PlatformOutdated(report.status));
    }
    let td = report.report.as_td10().ok_or(AttestationError::NotTdx)?;

    let measurements = Measurements {
        mrtd: hex::encode(td.mr_td),
        rtmr0: hex::encode(td.rt_mr0),
        rtmr1: hex::encode(td.rt_mr1),
        rtmr2: hex::encode(td.rt_mr2),
        rtmr3: hex::encode(td.rt_mr3),
    };
    if !expected
        .approved
        .iter()
        .any(|approved| approved.matches(&measurements))
    {
        return Err(AttestationError::MeasurementsNotApproved);
    }
    if td.report_data[..32] != attestation.task_hash() {
        return Err(AttestationError::ReportDataMismatch);
    }

    let ran = attestation.wasm_hash.as_deref().unwrap_or_default();
    let executed = attestation.executed_wasm_sha256.as_deref().unwrap_or(ran);
    if !ran.eq_ignore_ascii_case(expected.wasm_hash)
        || !executed.eq_ignore_ascii_case(expected.wasm_hash)
    {
        return Err(AttestationError::WrongWasm {
            expected: expected.wasm_hash.to_owned(),
            actual: executed.to_owned(),
        });
    }
    let output_hash = hex::encode(Sha256::digest(expected.output));
    if !output_hash.eq_ignore_ascii_case(&attestation.output_hash) {
        return Err(AttestationError::OutputMismatch);
    }

    Ok(Verified {
        tcb_status: report.status,
        measurements,
        wasm_hash: expected.wasm_hash.to_lowercase(),
        output_hash,
    })
}

/// Tries each collateral: there is one per platform, and only one fits a quote.
fn verify_quote(
    quote: &[u8],
    collaterals: &[String],
    now_secs: u64,
) -> Result<dcap_qvl::verify::VerifiedReport, AttestationError> {
    let mut last = String::from("no collateral was given");
    for collateral in collaterals {
        let collateral = parse_collateral(collateral)?;
        match dcap_qvl::verify::verify(quote, &collateral, now_secs) {
            Ok(report) => return Ok(report),
            Err(error) => last = error.to_string(),
        }
    }
    Err(AttestationError::QuoteNotVerified(last))
}

/// Reads collateral in the JSON form `OutLayer`'s register contract stores.
pub fn parse_collateral(json: &str) -> Result<QuoteCollateralV3, AttestationError> {
    let value: Value = serde_json::from_str(json)
        .map_err(|error| AttestationError::Malformed(error.to_string()))?;
    let text = |key: &str| {
        value
            .get(key)
            .and_then(Value::as_str)
            .map(str::to_owned)
            .ok_or_else(|| AttestationError::Malformed(format!("collateral has no {key}")))
    };
    let bytes = |key: &str| {
        hex::decode(text(key)?)
            .map_err(|_| AttestationError::Malformed(format!("collateral {key} is not hex")))
    };
    Ok(QuoteCollateralV3 {
        pck_crl_issuer_chain: text("pck_crl_issuer_chain")?,
        root_ca_crl: bytes("root_ca_crl")?,
        pck_crl: bytes("pck_crl")?,
        tcb_info_issuer_chain: text("tcb_info_issuer_chain")?,
        tcb_info: text("tcb_info")?,
        tcb_info_signature: bytes("tcb_info_signature")?,
        qe_identity_issuer_chain: text("qe_identity_issuer_chain")?,
        qe_identity: text("qe_identity")?,
        qe_identity_signature: bytes("qe_identity_signature")?,
        pck_certificate_chain: value
            .get("pck_certificate_chain")
            .and_then(Value::as_str)
            .map(str::to_owned),
    })
}
