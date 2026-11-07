//! Checks a real attestation of the deployed enclave's `identity` run on NEAR
//! testnet, and that changing any part of it is caught.

use undiluted_attestation::{AttestationError, Expected, Measurements, RunAttestation, verify};

const ATTESTATION: &str = include_str!("fixtures/identity-attestation.json");
const OUTPUT: &[u8] = include_bytes!("fixtures/identity-output.json");
const APPROVED: &str = include_str!("fixtures/approved-measurements.json");
const COLLATERALS: &str = include_str!("fixtures/collaterals.json");
const WASM: &str = "c3b8b92a101e575d1c0e2e502c1fc21443fa0e08e864e84fe467d5840bda9f77";

struct Inputs {
    attestation: RunAttestation,
    approved: Vec<Measurements>,
    collaterals: Vec<String>,
}

fn inputs() -> Inputs {
    Inputs {
        attestation: serde_json::from_str(ATTESTATION).unwrap(),
        approved: serde_json::from_str(APPROVED).unwrap(),
        collaterals: serde_json::from_str(COLLATERALS).unwrap(),
    }
}

impl Inputs {
    /// Checked at the moment the run happened, so the test does not expire
    /// with Intel's collateral.
    fn check(
        &self,
        wasm: &str,
        output: &[u8],
    ) -> Result<undiluted_attestation::Verified, AttestationError> {
        verify(
            &self.attestation,
            &Expected {
                wasm_hash: wasm,
                output,
                approved: &self.approved,
                collaterals: &self.collaterals,
                now_secs: u64::try_from(self.attestation.timestamp).unwrap(),
            },
        )
    }
}

#[test]
fn the_deployed_enclave_ran_in_an_attested_tdx_enclave() {
    let verified = inputs().check(WASM, OUTPUT).unwrap();
    assert_eq!(verified.tcb_status, "UpToDate");
    assert_eq!(verified.wasm_hash, WASM);
}

#[test]
fn the_report_data_commits_to_the_runs_details() {
    let inputs = inputs();
    assert_eq!(
        hex::encode(inputs.attestation.task_hash()),
        "3233582a9ec676b20b6ec9ed2b5157fadedd4e1bab218c43f18eb35f9d8ba31e"
    );
}

#[test]
fn another_wasm_is_refused() {
    let other = "00".repeat(32);
    assert!(matches!(
        inputs().check(&other, OUTPUT),
        Err(AttestationError::WrongWasm { .. })
    ));
}

#[test]
fn an_output_other_than_the_attested_one_is_refused() {
    let mut forged = OUTPUT.to_vec();
    forged[20] ^= 1;
    assert_eq!(
        inputs().check(WASM, &forged),
        Err(AttestationError::OutputMismatch)
    );
}

#[test]
fn swapping_a_detail_breaks_the_commitment() {
    let mut inputs = inputs();
    inputs.attestation.project_id = Some("someone.testnet/other".to_owned());
    assert_eq!(
        inputs.check(WASM, OUTPUT),
        Err(AttestationError::ReportDataMismatch)
    );
}

#[test]
fn claiming_another_output_hash_breaks_the_commitment() {
    let mut inputs = inputs();
    inputs.attestation.output_hash = "00".repeat(32);
    assert_eq!(
        inputs.check(WASM, OUTPUT),
        Err(AttestationError::ReportDataMismatch)
    );
}

#[test]
fn an_image_outlayer_did_not_approve_is_refused() {
    let mut inputs = inputs();
    inputs.approved.clear();
    assert_eq!(
        inputs.check(WASM, OUTPUT),
        Err(AttestationError::MeasurementsNotApproved)
    );
}

#[test]
fn a_tampered_quote_is_refused() {
    use base64::Engine;
    use base64::engine::general_purpose::STANDARD;

    let mut inputs = inputs();
    let mut quote = STANDARD.decode(&inputs.attestation.tdx_quote).unwrap();
    quote[600] ^= 1;
    inputs.attestation.tdx_quote = STANDARD.encode(quote);
    assert!(matches!(
        inputs.check(WASM, OUTPUT),
        Err(AttestationError::QuoteNotVerified(_))
    ));
}

#[test]
fn without_intels_collateral_nothing_verifies() {
    let mut inputs = inputs();
    inputs.collaterals.clear();
    assert!(matches!(
        inputs.check(WASM, OUTPUT),
        Err(AttestationError::QuoteNotVerified(_))
    ));
}
