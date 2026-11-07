//! How an investment reaches the company's treasury.

use std::thread;
use std::time::{Duration, SystemTime};

use serde_json::{Value, json};
use undiluted_domain::Money;

use crate::error::ClientError;
use crate::near_cli::{ContractCall, NearCli};

const STORAGE_REGISTRATION_ONLY: bool = true;
const ONE_YOCTO: &str = "1 yoctoNEAR";

/// A payment that reached the treasury.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Settlement {
    /// What the treasury actually received, after fees.
    pub received: Money,
    pub reference: String,
}

pub trait PaymentRail {
    /// Pays `amount` into the treasury and waits for it to settle.
    fn pay(&mut self, amount: Money) -> Result<Settlement, ClientError>;
}

/// Pretends a payment settled in full, for the local demo.
#[derive(Debug, Default)]
pub struct LocalRail {
    paid: u32,
}

impl PaymentRail for LocalRail {
    fn pay(&mut self, amount: Money) -> Result<Settlement, ClientError> {
        self.paid += 1;
        Ok(Settlement {
            received: amount,
            reference: format!("local-settlement-{}", self.paid),
        })
    }
}

#[derive(Debug, Clone)]
pub struct OneClickConfig {
    /// Base URL of a 1Click-compatible API.
    pub api_url: String,
    /// Sent only to `api_url`. Leave it out for sandboxes run by third parties.
    pub api_token: Option<String>,
    /// Tokens as `nep141:<contract>`, both using six decimals.
    pub origin_asset: String,
    pub destination_asset: String,
    pub treasury: String,
    pub slippage_basis_points: u32,
    pub quote_validity: Duration,
    pub poll_interval: Duration,
    pub max_polls: u32,
    pub gas: String,
    pub storage_deposit: String,
}

/// Pays through the 1Click API: quote, deposit to the quoted address, wait.
/// The API's response format is shared by mainnet and compatible sandboxes.
#[derive(Debug)]
pub struct OneClickRail {
    config: OneClickConfig,
    payer: NearCli,
}

impl OneClickRail {
    #[must_use]
    pub const fn new(config: OneClickConfig, payer: NearCli) -> Self {
        Self { config, payer }
    }

    fn request(&self, request: ureq::Request) -> ureq::Request {
        match &self.config.api_token {
            Some(token) => request.set("Authorization", &format!("Bearer {token}")),
            None => request,
        }
    }

    fn post(&self, path: &str, body: &Value) -> Result<Value, ClientError> {
        let url = format!("{}{path}", self.config.api_url);
        self.request(ureq::post(&url))
            .send_json(body)
            .map_err(|error| ClientError::Payment(error.to_string()))?
            .into_json()
            .map_err(|error| ClientError::Payment(error.to_string()))
    }

    fn get(&self, path: &str) -> Result<Value, ClientError> {
        let url = format!("{}{path}", self.config.api_url);
        self.request(ureq::get(&url))
            .call()
            .map_err(|error| ClientError::Payment(error.to_string()))?
            .into_json()
            .map_err(|error| ClientError::Payment(error.to_string()))
    }

    fn quote_request(&self, amount: Money, payer: &str) -> Value {
        let deadline = SystemTime::now() + self.config.quote_validity;
        json!({
            "dry": false,
            "swapType": "EXACT_INPUT",
            "slippageTolerance": self.config.slippage_basis_points,
            "originAsset": self.config.origin_asset,
            "depositType": "ORIGIN_CHAIN",
            "destinationAsset": self.config.destination_asset,
            "amount": amount.value().to_string(),
            "refundTo": payer,
            "refundType": "ORIGIN_CHAIN",
            "recipient": self.config.treasury,
            "recipientType": "DESTINATION_CHAIN",
            "deadline": humantime::format_rfc3339_seconds(deadline).to_string(),
        })
    }

    /// The transfer is never retried: a failure may still have moved the funds.
    fn deposit(&self, address: &str, amount: Money) -> Result<Option<String>, ClientError> {
        let token = origin_contract(&self.config.origin_asset)?;
        let registration = json!({
            "account_id": address,
            "registration_only": STORAGE_REGISTRATION_ONLY,
        });
        self.payer
            .call(&ContractCall {
                contract: token,
                method: "storage_deposit",
                arguments: &registration,
                gas: &self.config.gas,
                deposit: &self.config.storage_deposit,
            })
            .map_err(ClientError::Payment)?;

        let transfer = json!({
            "receiver_id": address,
            "amount": amount.value().to_string(),
        });
        self.payer
            .call(&ContractCall {
                contract: token,
                method: "ft_transfer",
                arguments: &transfer,
                gas: &self.config.gas,
                deposit: ONE_YOCTO,
            })
            .map_err(ClientError::Payment)
    }

    fn wait_for_settlement(&self, address: &str) -> Result<Settlement, ClientError> {
        for _ in 0..self.config.max_polls {
            let status = self.get(&format!("/v0/status?depositAddress={address}"))?;
            if let Some(settlement) = settled(&status)? {
                return Ok(settlement);
            }
            thread::sleep(self.config.poll_interval);
        }
        Err(ClientError::Payment(format!(
            "payment to {address} did not settle in time"
        )))
    }
}

impl PaymentRail for OneClickRail {
    fn pay(&mut self, amount: Money) -> Result<Settlement, ClientError> {
        let body = self.quote_request(amount, &self.payer.signer);
        let quote = self.post("/v0/quote", &body)?;
        let address = quote["quote"]["depositAddress"]
            .as_str()
            .ok_or_else(|| ClientError::Payment(format!("no deposit address in: {quote}")))?;

        if let Some(transaction) = self.deposit(address, amount)? {
            let submission = json!({ "txHash": transaction, "depositAddress": address });
            self.post("/v0/deposit/submit", &submission)?;
        }
        self.wait_for_settlement(address)
    }
}

fn origin_contract(asset: &str) -> Result<&str, ClientError> {
    asset
        .strip_prefix("nep141:")
        .ok_or_else(|| ClientError::Payment(format!("not a NEAR token: {asset}")))
}

/// Reads a status answer: settled, still in progress, or failed for good.
fn settled(status: &Value) -> Result<Option<Settlement>, ClientError> {
    match status["status"].as_str() {
        Some("SUCCESS") => {
            let details = &status["swapDetails"];
            let received = details["amountOut"]
                .as_str()
                .and_then(|amount| amount.parse().ok())
                .ok_or_else(|| ClientError::Payment(format!("no amount in: {status}")))?;
            let reference = details["destinationChainTxHashes"][0]["hash"]
                .as_str()
                .ok_or_else(|| ClientError::Payment(format!("no settlement in: {status}")))?;
            Ok(Some(Settlement {
                received: Money::new(received),
                reference: reference.to_owned(),
            }))
        }
        Some(failed @ ("REFUNDED" | "FAILED")) => {
            Err(ClientError::Payment(format!("payment ended as {failed}")))
        }
        _ => Ok(None),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_successful_status_gives_what_the_treasury_received() {
        let status = json!({
            "status": "SUCCESS",
            "swapDetails": {
                "amountOut": "1993690",
                "destinationChainTxHashes": [{ "hash": "Erw1" }],
            },
        });
        assert_eq!(
            settled(&status).unwrap(),
            Some(Settlement {
                received: Money::new(1_993_690),
                reference: "Erw1".to_owned(),
            })
        );
    }

    #[test]
    fn a_payment_in_progress_is_not_settled() {
        for state in ["PENDING_DEPOSIT", "KNOWN_DEPOSIT_TX", "PROCESSING"] {
            assert_eq!(settled(&json!({ "status": state })).unwrap(), None);
        }
    }

    #[test]
    fn a_refund_is_a_failure_even_though_the_funds_are_safe() {
        assert!(matches!(
            settled(&json!({ "status": "REFUNDED" })),
            Err(ClientError::Payment(_))
        ));
    }

    #[test]
    fn success_without_details_is_not_trusted() {
        assert!(matches!(
            settled(&json!({ "status": "SUCCESS", "swapDetails": {} })),
            Err(ClientError::Payment(_))
        ));
    }

    #[test]
    fn only_near_tokens_can_be_deposited_this_way() {
        assert_eq!(
            origin_contract("nep141:usdt.testnet").unwrap(),
            "usdt.testnet"
        );
        assert!(origin_contract("eth:0x1").is_err());
    }

    #[test]
    fn the_local_rail_settles_in_full_with_distinct_references() {
        let mut rail = LocalRail::default();
        let first = rail.pay(Money::new(5)).unwrap();
        let second = rail.pay(Money::new(5)).unwrap();
        assert_eq!(first.received, Money::new(5));
        assert_ne!(first.reference, second.reference);
    }
}
