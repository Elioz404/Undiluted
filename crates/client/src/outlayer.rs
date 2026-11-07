use std::path::PathBuf;
use std::process::Command;

use undiluted_service::wire::{Input, Output};

use crate::error::ClientError;
use crate::transport::{EnclaveIdentity, Exchange, Transport};
use crate::wire_codec::{into_exchange, into_identity, request_input};

#[derive(Debug, Clone)]
pub struct OutLayerConfig {
    /// Path to the `outlayer` command-line tool.
    pub cli: PathBuf,
    /// The deployed project, as `owner/name`.
    pub project: String,
    /// Account and profile of the secrets the enclave needs.
    pub secrets_account: String,
    pub secrets_profile: String,
}

/// Calls a project deployed on `OutLayer` through its command-line tool, which
/// holds the credentials and pays for the run.
#[derive(Debug)]
pub struct OutLayerTransport(OutLayerConfig);

impl OutLayerTransport {
    #[must_use]
    pub const fn new(config: OutLayerConfig) -> Self {
        Self(config)
    }

    fn call(&self, input: &Input) -> Result<Output, ClientError> {
        let input = serde_json::to_string(input).map_err(|_| ClientError::Encoding)?;
        let output = Command::new(&self.0.cli)
            .arg("run")
            .arg(&self.0.project)
            .arg(input)
            .arg("--secrets-profile")
            .arg(&self.0.secrets_profile)
            .arg("--secrets-account")
            .arg(&self.0.secrets_account)
            .output()
            .map_err(|error| ClientError::Transport(error.to_string()))?;

        let stdout = String::from_utf8_lossy(&output.stdout);
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(ClientError::Transport(stderr.trim().to_owned()));
        }
        parse_output(&stdout)
    }
}

impl Transport for OutLayerTransport {
    fn identity(&mut self) -> Result<EnclaveIdentity, ClientError> {
        into_identity(self.call(&Input::Identity)?)
    }

    fn submit(&mut self, envelope: &[u8]) -> Result<Exchange, ClientError> {
        into_exchange(self.call(&request_input(envelope))?)
    }
}

/// The tool prints progress lines before the JSON result.
fn parse_output(stdout: &str) -> Result<Output, ClientError> {
    let start = stdout
        .find('{')
        .ok_or_else(|| ClientError::Transport(format!("no result in output: {stdout}")))?;
    serde_json::from_str(&stdout[start..]).map_err(|error| ClientError::Protocol(error.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn progress_lines_before_the_result_are_skipped() {
        let stdout =
            "Running project (on-chain)...\nTx: abc\n{\"status\":\"error\",\"message\":\"no\"}\n";
        assert_eq!(
            parse_output(stdout).unwrap(),
            Output::Error {
                message: "no".to_owned()
            }
        );
    }

    #[test]
    fn output_without_a_result_is_a_transport_error_so_it_can_be_retried() {
        assert!(matches!(
            parse_output("Running project...\n"),
            Err(ClientError::Transport(_))
        ));
    }
}
