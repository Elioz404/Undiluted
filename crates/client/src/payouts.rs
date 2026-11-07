//! Carries out an approved exit: holds the funds and makes the signed payments.

use std::collections::{BTreeMap, BTreeSet};
use std::thread;
use std::time::Duration;

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use serde_json::json;
use undiluted_domain::{CompanyId, Money};
use undiluted_ledger::PublicKey;
use undiluted_service::PayoutInstruction;

use crate::error::ClientError;
use crate::near_cli::{CallFailure, ContractCall, NearCli, view};

const ONE_YOCTO: &str = "1 yoctoNEAR";
const ALREADY_PAID: &str = "already paid";

fn payout_failure(failure: CallFailure) -> ClientError {
    match failure {
        CallFailure::Refused(reason) => ClientError::PayoutRefused(reason),
        CallFailure::Unclear(reason) => ClientError::Payout(reason),
    }
}

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
            return Err(ClientError::PayoutRefused(
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
            return Err(ClientError::PayoutRefused(ALREADY_PAID.to_owned()));
        }
        if !distribution
            .enclave_key
            .verifies(&digest, &instruction.signature)
        {
            return Err(ClientError::PayoutRefused(
                "invalid enclave signature".to_owned(),
            ));
        }
        let paid = distribution.paid.checked_add(instruction.amount)?;
        if paid > distribution.funded {
            return Err(ClientError::PayoutRefused("not enough funds".to_owned()));
        }
        distribution.paid = paid;
        distribution.paid_indexes.insert(instruction.index);
        Ok(())
    }
}

/// How long to wait on a network that can report a timeout for a transaction
/// that later lands.
#[derive(Debug, Clone, Copy)]
pub struct RetryPolicy {
    /// How many times to send something that is safe to send again.
    pub attempts: u32,
    /// How many times to look for the result of each attempt.
    pub checks: u32,
    pub check_interval: Duration,
}

impl RetryPolicy {
    /// Whether `check` becomes true in time. A check that fails counts as not
    /// yet, because reads can time out as well.
    fn eventually(&self, mut check: impl FnMut() -> Result<bool, ClientError>) -> bool {
        for attempt in 0..self.checks {
            if attempt > 0 {
                thread::sleep(self.check_interval);
            }
            if matches!(check(), Ok(true)) {
                return true;
            }
        }
        false
    }
}

#[derive(Debug, Clone)]
pub struct NearPayoutsConfig {
    /// Signs, funds and relays. Its account owns the distributions it opens.
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
    pub retry: RetryPolicy,
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
            .map_err(payout_failure)
    }

    fn funded(&self, company: &CompanyId) -> Result<Money, ClientError> {
        let distribution = view(
            &self.0.rpc_url,
            &self.0.contract,
            "distribution",
            &json!({ "funder": self.0.near.signer, "company_id": company.as_str() }),
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
            &json!({ "funder": self.0.near.signer, "company_id": company.as_str(), "index": index }),
        )?;
        Ok(paid.as_bool().unwrap_or(false))
    }

    fn exists(&self, company: &CompanyId) -> Result<bool, ClientError> {
        let distribution = view(
            &self.0.rpc_url,
            &self.0.contract,
            "distribution",
            &json!({ "funder": self.0.near.signer, "company_id": company.as_str() }),
        )?;
        Ok(!distribution.is_null())
    }

    /// A payment is marked as paid before its transfer finishes and reopened if
    /// the transfer fails, so it only counts once it stays paid.
    fn stays_paid(&self, company: &CompanyId, index: u32) -> bool {
        self.0.retry.eventually(|| self.is_paid(company, index)) && {
            thread::sleep(self.0.retry.check_interval);
            self.0.retry.eventually(|| self.is_paid(company, index))
        }
    }
}

impl PayoutExecutor for NearPayouts {
    fn open(
        &mut self,
        company: &CompanyId,
        enclave_key: &PublicKey,
        total: Money,
    ) -> Result<(), ClientError> {
        let arguments = json!({
            "company_id": company.as_str(),
            "token": self.0.token,
            "enclave_key": format!("ed25519:{}", bs58::encode(enclave_key.as_bytes()).into_string()),
            "total": total.value().to_string(),
        });
        let mut failure = None;
        for _ in 0..self.0.retry.attempts {
            let sent = self.register_with_token(&self.0.contract).and_then(|()| {
                self.0
                    .near
                    .call(&ContractCall {
                        contract: &self.0.contract,
                        method: "open",
                        arguments: &arguments,
                        gas: &self.0.call_gas,
                        deposit: &self.0.storage_deposit,
                    })
                    .map_err(payout_failure)
            });
            if self.0.retry.eventually(|| self.exists(company)) {
                return Ok(());
            }
            failure = sent.err();
        }
        Err(failure
            .unwrap_or_else(|| ClientError::Payout("the distribution was not opened".to_owned())))
    }

    /// Sent only once: a failure may still have moved the funds, so the contract
    /// is asked what actually arrived.
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
        if self
            .0
            .retry
            .eventually(|| Ok(self.funded(company)? >= amount))
        {
            return Ok(());
        }
        Err(sent.map_or_else(payout_failure, |_| {
            ClientError::Payout("the funds did not reach the distribution".to_owned())
        }))
    }

    /// Safe to send again: the contract refuses to pay the same index twice.
    fn pay(
        &mut self,
        company: &CompanyId,
        instruction: &PayoutInstruction,
    ) -> Result<(), ClientError> {
        let arguments = json!({
            "funder": self.0.near.signer,
            "company_id": company.as_str(),
            "index": instruction.index,
            "recipient": instruction.recipient,
            "amount": instruction.amount.value().to_string(),
            "signature": STANDARD.encode(instruction.signature.as_bytes()),
        });
        let mut failure = None;
        for _ in 0..self.0.retry.attempts {
            let sent = self
                .register_with_token(&instruction.recipient)
                .and_then(|()| {
                    self.0
                        .near
                        .call(&ContractCall {
                            contract: &self.0.contract,
                            method: "pay",
                            arguments: &arguments,
                            gas: &self.0.pay_gas,
                            deposit: &self.0.storage_deposit,
                        })
                        .map_err(payout_failure)
                });
            // A real refusal will not change by trying again. "Already paid" is
            // the one that means an earlier attempt landed.
            if let Err(ClientError::PayoutRefused(reason)) = &sent
                && reason != ALREADY_PAID
            {
                return sent.map(|_| ());
            }
            if self.stays_paid(company, instruction.index) {
                return Ok(());
            }
            failure = sent.err();
        }
        Err(failure
            .unwrap_or_else(|| ClientError::Payout("the payment did not go through".to_owned())))
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

    fn policy(checks: u32) -> RetryPolicy {
        RetryPolicy {
            attempts: 1,
            checks,
            check_interval: Duration::ZERO,
        }
    }

    #[test]
    fn waiting_stops_as_soon_as_the_result_shows_up() {
        let mut calls = 0;
        assert!(policy(5).eventually(|| {
            calls += 1;
            Ok(calls == 3)
        }));
        assert_eq!(calls, 3);
    }

    #[test]
    fn a_failed_read_means_not_yet() {
        let mut calls = 0;
        assert!(policy(3).eventually(|| {
            calls += 1;
            if calls == 1 {
                Err(ClientError::Transport("timeout".to_owned()))
            } else {
                Ok(true)
            }
        }));
    }

    #[test]
    fn waiting_gives_up_when_the_checks_run_out() {
        let mut calls = 0;
        assert!(!policy(3).eventually(|| {
            calls += 1;
            Ok(false)
        }));
        assert_eq!(calls, 3);
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
            Err(ClientError::PayoutRefused(reason)) => reason,
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
