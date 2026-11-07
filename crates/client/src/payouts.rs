//! Carries out an approved exit: holds the funds and makes the signed payments.

use std::collections::{BTreeMap, BTreeSet};

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use serde_json::json;
use undiluted_domain::{CompanyId, Money};
use undiluted_ledger::PublicKey;
use undiluted_service::PayoutInstruction;

use crate::error::ClientError;
use crate::near_cli::{ContractCall, NearCli, view};

const ONE_YOCTO: &str = "1 yoctoNEAR";

pub trait PayoutExecutor {
    /// Starts a distribution for `total`, payable only on `enclave_key`'s word.
    fn open(
        &mut self,
        company: &CompanyId,
        enclave_key: &PublicKey,
        total: Money,
    ) -> Result<(), ClientError>;

    fn fund(&mut self, company: &CompanyId, amount: Money) -> Result<(), ClientError>;

    fn pay(
        &mut self,
        company: &CompanyId,
        instruction: &PayoutInstruction,
    ) -> Result<(), ClientError>;
}

#[derive(Debug)]
struct LocalDistribution {
    enclave_key: PublicKey,
    total: Money,
    funded: Money,
    paid: Money,
    paid_indexes: BTreeSet<u32>,
}

/// Follows the contract's rules in memory, for the local demo and tests.
#[derive(Debug, Default)]
pub struct LocalPayouts(BTreeMap<CompanyId, LocalDistribution>);

impl LocalPayouts {
    fn distribution(&mut self, company: &CompanyId) -> Result<&mut LocalDistribution, ClientError> {
        self.0
            .get_mut(company)
            .ok_or_else(|| ClientError::Payout("unknown distribution".to_owned()))
    }
}

impl PayoutExecutor for LocalPayouts {
    fn open(
        &mut self,
        company: &CompanyId,
        enclave_key: &PublicKey,
        total: Money,
    ) -> Result<(), ClientError> {
        if self.0.contains_key(company) {
            return Err(ClientError::Payout(
                "distribution already exists".to_owned(),
            ));
        }
        self.0.insert(
            company.clone(),
            LocalDistribution {
                enclave_key: *enclave_key,
                total,
                funded: Money::ZERO,
                paid: Money::ZERO,
                paid_indexes: BTreeSet::new(),
            },
        );
        Ok(())
    }

    fn fund(&mut self, company: &CompanyId, amount: Money) -> Result<(), ClientError> {
        let distribution = self.distribution(company)?;
        let missing = distribution.total.checked_sub(distribution.funded)?;
        distribution.funded = distribution.funded.checked_add(amount.min(missing))?;
        Ok(())
    }

    fn pay(
        &mut self,
        company: &CompanyId,
        instruction: &PayoutInstruction,
    ) -> Result<(), ClientError> {
        let digest = PayoutInstruction::signing_digest(
            company,
            instruction.index,
            &instruction.recipient,
            instruction.amount,
        )?;
        let distribution = self.distribution(company)?;
        if distribution.paid_indexes.contains(&instruction.index) {
            return Err(ClientError::Payout("already paid".to_owned()));
        }
        if !distribution
            .enclave_key
            .verifies(&digest, &instruction.signature)
        {
            return Err(ClientError::Payout("invalid enclave signature".to_owned()));
        }
        let paid = distribution.paid.checked_add(instruction.amount)?;
        if paid > distribution.funded {
            return Err(ClientError::Payout("not enough funds".to_owned()));
        }
        distribution.paid = paid;
        distribution.paid_indexes.insert(instruction.index);
        Ok(())
    }
}

#[derive(Debug, Clone)]
pub struct NearPayoutsConfig {
    /// Signs, funds and relays.
    pub near: NearCli,
    pub contract: String,
    /// The NEP-141 token proceeds are paid in, with six decimals.
    pub token: String,
    pub rpc_url: String,
    pub call_gas: String,
    pub pay_gas: String,
    pub fund_gas: String,
    /// Attached for contract storage. Whatever is not used comes back.
    pub storage_deposit: String,
    /// Registers an account with the token contract.
    pub token_storage_deposit: String,
}

#[derive(Debug)]
pub struct NearPayouts(NearPayoutsConfig);

impl NearPayouts {
    #[must_use]
    pub const fn new(config: NearPayoutsConfig) -> Self {
        Self(config)
    }

    fn register_with_token(&self, account: &str) -> Result<(), ClientError> {
        self.0
            .near
            .call(&ContractCall {
                contract: &self.0.token,
                method: "storage_deposit",
                arguments: &json!({ "account_id": account, "registration_only": true }),
                gas: &self.0.call_gas,
                deposit: &self.0.token_storage_deposit,
            })
            .map(|_| ())
            .map_err(ClientError::Payout)
    }

    fn funded(&self, company: &CompanyId) -> Result<Money, ClientError> {
        let distribution = view(
            &self.0.rpc_url,
            &self.0.contract,
            "distribution",
            &json!({ "company_id": company.as_str() }),
        )?;
        distribution["funded"]
            .as_str()
            .and_then(|amount| amount.parse().ok())
            .map(Money::new)
            .ok_or_else(|| ClientError::Payout(format!("no such distribution: {distribution}")))
    }

    fn is_paid(&self, company: &CompanyId, index: u32) -> Result<bool, ClientError> {
        let paid = view(
            &self.0.rpc_url,
            &self.0.contract,
            "is_paid",
            &json!({ "company_id": company.as_str(), "index": index }),
        )?;
        Ok(paid.as_bool().unwrap_or(false))
    }
}

impl PayoutExecutor for NearPayouts {
    fn open(
        &mut self,
        company: &CompanyId,
        enclave_key: &PublicKey,
        total: Money,
    ) -> Result<(), ClientError> {
        self.register_with_token(&self.0.contract)?;
        let arguments = json!({
            "company_id": company.as_str(),
            "token": self.0.token,
            "enclave_key": format!("ed25519:{}", bs58::encode(enclave_key.as_bytes()).into_string()),
            "total": total.value().to_string(),
        });
        self.0
            .near
            .call(&ContractCall {
                contract: &self.0.contract,
                method: "open",
                arguments: &arguments,
                gas: &self.0.call_gas,
                deposit: &self.0.storage_deposit,
            })
            .map(|_| ())
            .map_err(ClientError::Payout)
    }

    /// Never retried blindly: a failure may still have moved the funds, so the
    /// contract is asked what actually arrived.
    fn fund(&mut self, company: &CompanyId, amount: Money) -> Result<(), ClientError> {
        let arguments = json!({
            "receiver_id": self.0.contract,
            "amount": amount.value().to_string(),
            "msg": company.as_str(),
        });
        let sent = self.0.near.call(&ContractCall {
            contract: &self.0.token,
            method: "ft_transfer_call",
            arguments: &arguments,
            gas: &self.0.fund_gas,
            deposit: ONE_YOCTO,
        });
        if self.funded(company)? >= amount {
            return Ok(());
        }
        Err(ClientError::Payout(sent.err().unwrap_or_else(|| {
            "the funds did not reach the distribution".to_owned()
        })))
    }

    fn pay(
        &mut self,
        company: &CompanyId,
        instruction: &PayoutInstruction,
    ) -> Result<(), ClientError> {
        self.register_with_token(&instruction.recipient)?;
        let arguments = json!({
            "company_id": company.as_str(),
            "index": instruction.index,
            "recipient": instruction.recipient,
            "amount": instruction.amount.value().to_string(),
            "signature": STANDARD.encode(instruction.signature.as_bytes()),
        });
        let sent = self.0.near.call(&ContractCall {
            contract: &self.0.contract,
            method: "pay",
            arguments: &arguments,
            gas: &self.0.pay_gas,
            deposit: &self.0.storage_deposit,
        });
        if self.is_paid(company, instruction.index)? {
            return Ok(());
        }
        Err(ClientError::Payout(sent.err().unwrap_or_else(|| {
            "the payment is not recorded".to_owned()
        })))
    }
}

#[cfg(test)]
mod tests {
    use ed25519_dalek::{Signer, SigningKey};
    use undiluted_ledger::SignatureBytes;

    use super::*;

    const COMPANY: &str = "acme";

    fn company() -> CompanyId {
        CompanyId::new(COMPANY).unwrap()
    }

    fn enclave() -> SigningKey {
        SigningKey::from_bytes(&[6; 32])
    }

    fn instruction(signer: &SigningKey, index: u32, amount: u128) -> PayoutInstruction {
        let amount = Money::new(amount);
        let digest =
            PayoutInstruction::signing_digest(&company(), index, "ana.near", amount).unwrap();
        PayoutInstruction {
            index,
            recipient: "ana.near".to_owned(),
            amount,
            signature: SignatureBytes::from_bytes(signer.sign(digest.as_bytes()).to_bytes()),
        }
    }

    fn funded(total: u128) -> LocalPayouts {
        let mut payouts = LocalPayouts::default();
        let key = PublicKey::from_bytes(enclave().verifying_key().to_bytes());
        payouts.open(&company(), &key, Money::new(total)).unwrap();
        payouts.fund(&company(), Money::new(total)).unwrap();
        payouts
    }

    fn rejected(result: Result<(), ClientError>) -> String {
        match result {
            Err(ClientError::Payout(reason)) => reason,
            other => panic!("expected a refusal, got {other:?}"),
        }
    }

    #[test]
    fn signed_payments_within_the_funds_go_through() {
        assert!(
            funded(10)
                .pay(&company(), &instruction(&enclave(), 0, 10))
                .is_ok()
        );
    }

    #[test]
    fn a_payment_cannot_repeat() {
        let mut payouts = funded(10);
        payouts
            .pay(&company(), &instruction(&enclave(), 0, 4))
            .unwrap();
        assert_eq!(
            rejected(payouts.pay(&company(), &instruction(&enclave(), 0, 4))),
            "already paid"
        );
    }

    #[test]
    fn payments_cannot_exceed_the_funds() {
        assert_eq!(
            rejected(funded(10).pay(&company(), &instruction(&enclave(), 0, 11))),
            "not enough funds"
        );
    }

    #[test]
    fn only_the_enclave_can_order_a_payment() {
        let forger = SigningKey::from_bytes(&[8; 32]);
        assert_eq!(
            rejected(funded(10).pay(&company(), &instruction(&forger, 0, 1))),
            "invalid enclave signature"
        );
    }
}
