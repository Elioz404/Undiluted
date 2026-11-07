//! Reads settled payments from the chain, so an investment does not rest only on
//! what the parties claim.

use serde_json::Value;
use thiserror::Error;
use undiluted_domain::Money;

const EVENT_PREFIX: &str = "EVENT_JSON:";
const FUNGIBLE_TOKEN_STANDARD: &str = "nep141";
const TRANSFER_EVENT: &str = "ft_transfer";

#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[error("{0}")]
pub struct ChainError(pub String);

/// Tokens that moved to an account in one settlement.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TokenTransfer {
    pub token: String,
    pub to: String,
    pub amount: Money,
}

pub trait ChainReader {
    /// The token transfers a settlement really made.
    fn token_transfers(&self, reference: &str) -> Result<Vec<TokenTransfer>, ChainError>;
}

/// Extracts token transfers from a transaction as an RPC node returns it.
///
/// Any contract can log any text, so a transfer only counts if the token's own
/// contract logged it, in a receipt that succeeded.
pub fn parse_transfers(transaction: &str) -> Result<Vec<TokenTransfer>, ChainError> {
    let parsed: Value = serde_json::from_str(transaction)
        .map_err(|error| ChainError(format!("unreadable transaction: {error}")))?;
    let transaction = parsed.get("result").unwrap_or(&parsed);
    let receipts = transaction["receipts_outcome"]
        .as_array()
        .ok_or_else(|| ChainError("the transaction has no receipts".to_owned()))?;

    let mut transfers = Vec::new();
    for receipt in receipts {
        let outcome = &receipt["outcome"];
        if outcome["status"].get("Failure").is_some() {
            continue;
        }
        let Some(token) = outcome["executor_id"].as_str() else {
            continue;
        };
        for log in outcome["logs"].as_array().into_iter().flatten() {
            if let Some(log) = log.as_str() {
                transfers.extend(transfers_in_log(token, log));
            }
        }
    }
    Ok(transfers)
}

fn transfers_in_log(token: &str, log: &str) -> Vec<TokenTransfer> {
    let Some(event) = log
        .strip_prefix(EVENT_PREFIX)
        .and_then(|event| serde_json::from_str::<Value>(event).ok())
    else {
        return Vec::new();
    };
    if event["standard"] != FUNGIBLE_TOKEN_STANDARD || event["event"] != TRANSFER_EVENT {
        return Vec::new();
    }
    event["data"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|transfer| {
            Some(TokenTransfer {
                token: token.to_owned(),
                to: transfer["new_owner_id"].as_str()?.to_owned(),
                amount: Money::new(transfer["amount"].as_str()?.parse().ok()?),
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    const TOKEN: &str = "usdc.near";
    const TREASURY: &str = "treasury.near";

    fn transfer_log(to: &str, amount: &str) -> String {
        let event = json!({
            "standard": "nep141",
            "version": "1.0.0",
            "event": "ft_transfer",
            "data": [{ "old_owner_id": "maker.near", "new_owner_id": to, "amount": amount }],
        });
        format!("EVENT_JSON:{event}")
    }

    fn transaction(executor: &str, log: &str, status: &Value) -> String {
        json!({
            "receipts_outcome": [{
                "outcome": { "executor_id": executor, "logs": [log], "status": status },
            }],
        })
        .to_string()
    }

    fn success() -> Value {
        json!({ "SuccessValue": "" })
    }

    #[test]
    fn a_transfer_logged_by_the_token_counts() {
        let tx = transaction(TOKEN, &transfer_log(TREASURY, "1993690"), &success());
        assert_eq!(
            parse_transfers(&tx).unwrap(),
            vec![TokenTransfer {
                token: TOKEN.to_owned(),
                to: TREASURY.to_owned(),
                amount: Money::new(1_993_690),
            }]
        );
    }

    #[test]
    fn a_transfer_logged_by_another_contract_is_attributed_to_that_contract() {
        let tx = transaction("attacker.near", &transfer_log(TREASURY, "5"), &success());
        assert_eq!(parse_transfers(&tx).unwrap()[0].token, "attacker.near");
    }

    #[test]
    fn a_failed_receipt_does_not_count() {
        let failed = json!({ "Failure": { "ActionError": {} } });
        let tx = transaction(TOKEN, &transfer_log(TREASURY, "5"), &failed);
        assert_eq!(parse_transfers(&tx).unwrap(), vec![]);
    }

    #[test]
    fn other_events_and_plain_logs_are_ignored() {
        let other = json!({ "standard": "nep141", "event": "ft_mint", "data": [] });
        for log in [format!("EVENT_JSON:{other}"), "hello".to_owned()] {
            let tx = transaction(TOKEN, &log, &success());
            assert_eq!(parse_transfers(&tx).unwrap(), vec![]);
        }
    }

    #[test]
    fn an_rpc_envelope_is_accepted() {
        let inner = transaction(TOKEN, &transfer_log(TREASURY, "5"), &success());
        let wrapped = format!("{{\"result\":{inner}}}");
        assert_eq!(parse_transfers(&wrapped).unwrap().len(), 1);
    }

    #[test]
    fn an_unknown_transaction_is_an_error() {
        assert!(parse_transfers("not json").is_err());
        assert!(parse_transfers("{}").is_err());
    }
}
