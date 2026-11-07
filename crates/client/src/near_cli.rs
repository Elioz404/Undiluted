use std::fmt;
use std::path::PathBuf;
use std::process::Command;

use serde_json::Value;

use crate::error::ClientError;

const SUCCESS_MARKER: &str = "Transaction ID:";
/// What our contracts log just before refusing a call.
const REFUSAL_MARKER: &str = "undiluted refused: ";

/// Why a call did not clearly succeed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CallFailure {
    /// The contract refused it, for this reason. Repeating it will not help.
    Refused(String),
    /// Anything else, such as a timeout. The call may still land.
    Unclear(String),
}

impl fmt::Display for CallFailure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Refused(reason) | Self::Unclear(reason) => f.write_str(reason),
        }
    }
}

/// Signs and sends transactions through the `near` command-line tool, which
/// keeps the signer's key in the system keychain.
#[derive(Debug, Clone)]
pub struct NearCli {
    pub path: PathBuf,
    pub network: String,
    pub signer: String,
}

/// One contract call to make.
#[derive(Debug)]
pub struct ContractCall<'a> {
    pub contract: &'a str,
    pub method: &'a str,
    pub arguments: &'a Value,
    pub gas: &'a str,
    pub deposit: &'a str,
}

impl NearCli {
    /// Sends a call and returns its transaction hash when one is reported.
    pub fn call(&self, call: &ContractCall<'_>) -> Result<Option<String>, CallFailure> {
        let output = Command::new(&self.path)
            .args(["contract", "call-function", "as-transaction"])
            .arg(call.contract)
            .arg(call.method)
            .arg("json-args")
            .arg(call.arguments.to_string())
            .arg("prepaid-gas")
            .arg(call.gas)
            .arg("attached-deposit")
            .arg(call.deposit)
            .arg("sign-as")
            .arg(&self.signer)
            .arg("network-config")
            .arg(&self.network)
            .args(["sign-with-keychain", "send"])
            .output()
            .map_err(|error| CallFailure::Unclear(error.to_string()))?;

        let printed = format!(
            "{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        if let Some(reason) = refusal(&printed) {
            return Err(CallFailure::Refused(reason));
        }
        if output.status.success() {
            Ok(transaction_hash(&printed))
        } else {
            Err(CallFailure::Unclear(printed.trim().to_owned()))
        }
    }
}

/// The reason a contract gave, without the tool's terminal colours and quotes.
fn refusal(printed: &str) -> Option<String> {
    printed.lines().find_map(|line| {
        let (_, reason) = line.split_once(REFUSAL_MARKER)?;
        let reason = reason.split('\u{1b}').next().unwrap_or(reason);
        Some(reason.trim().trim_matches('"').to_owned())
    })
}

fn transaction_hash(printed: &str) -> Option<String> {
    printed.lines().find_map(|line| {
        let (_, rest) = line.split_once(SUCCESS_MARKER)?;
        rest.split_whitespace().next().map(str::to_owned)
    })
}

/// Reads a view method from an RPC node.
pub fn view(
    rpc_url: &str,
    contract: &str,
    method: &str,
    arguments: &Value,
) -> Result<Value, ClientError> {
    use base64::Engine;
    use base64::engine::general_purpose::STANDARD;

    let request = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "query",
        "params": {
            "request_type": "call_function",
            "finality": "final",
            "account_id": contract,
            "method_name": method,
            "args_base64": STANDARD.encode(arguments.to_string()),
        },
    });
    let answer: Value = ureq::post(rpc_url)
        .send_json(request)
        .map_err(|error| ClientError::Transport(error.to_string()))?
        .into_json()
        .map_err(|error| ClientError::Transport(error.to_string()))?;
    let bytes: Vec<u8> = answer["result"]["result"]
        .as_array()
        .ok_or_else(|| ClientError::Protocol(format!("unexpected RPC answer: {answer}")))?
        .iter()
        .filter_map(|byte| byte.as_u64().and_then(|byte| u8::try_from(byte).ok()))
        .collect();
    serde_json::from_slice(&bytes).map_err(|error| ClientError::Protocol(error.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_the_transaction_hash_among_decorated_output() {
        let printed = "├  Transaction Execution Info:\n│    Transaction ID: AbC123\n│    done";
        assert_eq!(transaction_hash(printed), Some("AbC123".to_owned()));
    }

    #[test]
    fn a_refusal_is_read_from_the_logs_without_colours() {
        let printed = "│  Logs:\n│    \u{1b}[2mundiluted refused: not enough funds\u{1b}[0m\n";
        assert_eq!(refusal(printed), Some("not enough funds".to_owned()));
    }

    #[test]
    fn other_failures_are_not_refusals() {
        assert_eq!(refusal("the request failed with timeout error"), None);
    }

    #[test]
    fn no_hash_when_none_is_printed() {
        assert_eq!(transaction_hash("nothing here"), None);
    }
}
