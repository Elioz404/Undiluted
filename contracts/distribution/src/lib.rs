//! Pays exit proceeds without trusting whoever relays the payments.
//!
//! The enclave decides who gets what but holds no funds. This contract holds the
//! funds but decides nothing: it pays only what the enclave signed, each payment
//! once, and never more than what was deposited.

use near_sdk::borsh::to_vec;
use near_sdk::json_types::{Base64VecU8, U128};
use near_sdk::store::{LookupMap, LookupSet};
use near_sdk::{
    env, ext_contract, near, AccountId, BorshStorageKey, CurveType, Gas, NearToken, PanicOnDefault,
    Promise, PromiseError, PromiseOrValue, PublicKey,
};

/// Must match the instruction the enclave signs in the service crate. A test
/// checks real instructions against this contract so the two cannot drift.
const INSTRUCTION_TAG: &str = "undiluted/service/payout/v1";
const FT_TRANSFER_GAS: Gas = Gas::from_tgas(15);
const ON_PAID_GAS: Gas = Gas::from_tgas(10);
const ONE_YOCTO: NearToken = NearToken::from_yoctonear(1);

const REFUSAL_MARKER: &str = "undiluted refused: ";

#[derive(BorshStorageKey)]
#[near]
enum StorageKey {
    Distributions,
    Paid,
}

#[derive(Clone, Debug, PartialEq, Eq)]
#[near(serializers = [borsh, json])]
pub struct Distribution {
    pub token: AccountId,
    pub enclave_key: PublicKey,
    pub total: U128,
    pub funded: U128,
    pub paid: U128,
}

#[near(event_json(standard = "undiluted_distribution"))]
pub enum DistributionEvent {
    #[event_version("1.0.0")]
    Opened { company_id: String, total: U128 },
    #[event_version("1.0.0")]
    Funded { company_id: String, amount: U128 },
    #[event_version("1.0.0")]
    Paid { company_id: String, index: u32 },
    #[event_version("1.0.0")]
    PaymentFailed { company_id: String, index: u32 },
}

#[ext_contract(ext_token)]
pub trait FungibleToken {
    fn ft_transfer(&mut self, receiver_id: AccountId, amount: U128);
}

#[ext_contract(ext_self)]
pub trait Callbacks {
    fn on_paid(&mut self, company_id: String, index: u32, amount: U128);
}

#[near(contract_state)]
#[derive(PanicOnDefault)]
pub struct DistributionRegistry {
    distributions: LookupMap<String, Distribution>,
    paid: LookupSet<(String, u32)>,
}

#[near]
impl DistributionRegistry {
    #[init]
    #[must_use]
    pub fn new() -> Self {
        Self {
            distributions: LookupMap::new(StorageKey::Distributions),
            paid: LookupSet::new(StorageKey::Paid),
        }
    }

    /// Starts a distribution. The token, enclave and total can never change.
    #[payable]
    pub fn open(
        &mut self,
        company_id: String,
        token: AccountId,
        enclave_key: PublicKey,
        total: U128,
    ) {
        let storage_before = env::storage_usage();
        ensure(total.0 > 0, "total must be positive");
        ensure(
            enclave_key.curve_type() == CurveType::ED25519,
            "enclave key must be ed25519",
        );
        ensure(
            !self.distributions.contains_key(&company_id),
            "distribution already exists",
        );
        self.distributions.insert(
            company_id.clone(),
            Distribution {
                token,
                enclave_key,
                total,
                funded: U128(0),
                paid: U128(0),
            },
        );
        self.distributions.flush();
        settle_storage(storage_before, env::attached_deposit());
        DistributionEvent::Opened { company_id, total }.emit();
    }

    /// Receives funds. `msg` names the company. Anything beyond the total goes
    /// straight back to the sender.
    pub fn ft_on_transfer(
        &mut self,
        sender_id: AccountId,
        amount: U128,
        msg: String,
    ) -> PromiseOrValue<U128> {
        let _ = sender_id;
        let distribution = self
            .distributions
            .get_mut(&msg)
            .unwrap_or_else(|| refuse("unknown distribution"));
        ensure(
            env::predecessor_account_id() == distribution.token,
            "wrong token",
        );
        let missing = distribution.total.0.saturating_sub(distribution.funded.0);
        let accepted = missing.min(amount.0);
        distribution.funded = U128(distribution.funded.0 + accepted);
        DistributionEvent::Funded {
            company_id: msg,
            amount: U128(accepted),
        }
        .emit();
        PromiseOrValue::Value(U128(amount.0 - accepted))
    }

    /// Makes one payment. Anyone may relay it, because only the enclave's
    /// signature makes it valid, and the signature covers recipient and amount.
    #[payable]
    pub fn pay(
        &mut self,
        company_id: String,
        index: u32,
        recipient: AccountId,
        amount: U128,
        signature: Base64VecU8,
    ) -> Promise {
        let storage_before = env::storage_usage();
        let key = (company_id.clone(), index);
        ensure(!self.paid.contains(&key), "already paid");

        let distribution = self
            .distributions
            .get_mut(&company_id)
            .unwrap_or_else(|| refuse("unknown distribution"));
        ensure(
            is_valid_instruction(
                &distribution.enclave_key,
                &company_id,
                index,
                &recipient,
                amount.0,
                &signature.0,
            ),
            "invalid enclave signature",
        );
        let available = distribution.funded.0.saturating_sub(distribution.paid.0);
        ensure(amount.0 <= available, "not enough funds");

        distribution.paid = U128(distribution.paid.0 + amount.0);
        let token = distribution.token.clone();
        self.distributions.flush();
        self.paid.insert(key);
        settle_storage(storage_before, env::attached_deposit());

        ext_token::ext(token)
            .with_attached_deposit(ONE_YOCTO)
            .with_static_gas(FT_TRANSFER_GAS)
            .ft_transfer(recipient, amount)
            .then(
                ext_self::ext(env::current_account_id())
                    .with_static_gas(ON_PAID_GAS)
                    .on_paid(company_id, index, amount),
            )
    }

    /// Reopens a payment whose transfer failed, so it can be relayed again.
    #[private]
    pub fn on_paid(
        &mut self,
        company_id: String,
        index: u32,
        amount: U128,
        #[callback_result] result: Result<(), PromiseError>,
    ) {
        if result.is_ok() {
            DistributionEvent::Paid { company_id, index }.emit();
            return;
        }
        if let Some(distribution) = self.distributions.get_mut(&company_id) {
            distribution.paid = U128(distribution.paid.0.saturating_sub(amount.0));
        }
        self.paid.remove(&(company_id.clone(), index));
        DistributionEvent::PaymentFailed { company_id, index }.emit();
    }

    #[must_use]
    pub fn distribution(&self, company_id: String) -> Option<Distribution> {
        self.distributions.get(&company_id).cloned()
    }

    #[must_use]
    pub fn is_paid(&self, company_id: String, index: u32) -> bool {
        self.paid.contains(&(company_id, index))
    }
}

/// Logs the reason before failing. A failed call's panic message does not reach
/// callers reliably, but its logs do.
fn refuse(reason: &str) -> ! {
    env::log_str(&format!("{REFUSAL_MARKER}{reason}"));
    env::panic_str(reason)
}

fn ensure(condition: bool, reason: &str) {
    if !condition {
        refuse(reason);
    }
}

fn is_valid_instruction(
    enclave_key: &PublicKey,
    company_id: &str,
    index: u32,
    recipient: &AccountId,
    amount: u128,
    signature: &[u8],
) -> bool {
    let Ok(signature) = <&[u8; 64]>::try_from(signature) else {
        return false;
    };
    let Ok(key) = <&[u8; 32]>::try_from(&enclave_key.as_bytes()[1..]) else {
        return false;
    };
    let Ok(payload) = to_vec(&(company_id, index, recipient.as_str(), amount)) else {
        return false;
    };

    let mut tagged = Vec::with_capacity(8 + INSTRUCTION_TAG.len() + payload.len());
    tagged.extend_from_slice(&(INSTRUCTION_TAG.len() as u64).to_le_bytes());
    tagged.extend_from_slice(INSTRUCTION_TAG.as_bytes());
    tagged.extend_from_slice(&payload);
    env::ed25519_verify(signature, env::sha256_array(&tagged), key)
}

/// Charges the caller for the storage just used and refunds the rest.
fn settle_storage(storage_before: u64, attached: NearToken) {
    let used = env::storage_usage().saturating_sub(storage_before);
    let cost = env::storage_byte_cost().saturating_mul(u128::from(used));
    ensure(attached >= cost, "attached deposit does not cover storage");

    let refund = attached.saturating_sub(cost);
    if refund > NearToken::from_yoctonear(0) {
        Promise::new(env::predecessor_account_id())
            .transfer(refund)
            .detach();
    }
}

#[cfg(test)]
mod tests {
    use ed25519_dalek::{Signer, SigningKey};
    use near_sdk::test_utils::{accounts, VMContextBuilder};
    use near_sdk::{testing_env, PromiseResult, RuntimeFeesConfig};
    use undiluted_domain::{CompanyId, Money};
    use undiluted_service::PayoutInstruction;

    use super::*;

    const COMPANY: &str = "acme";
    const TOTAL: u128 = 100;
    const DEPOSIT: NearToken = NearToken::from_millinear(100);
    const ENCLAVE_SEED: u8 = 7;
    const FORGER_SEED: u8 = 9;

    fn act_as(account: AccountId, deposit: NearToken) {
        testing_env!(VMContextBuilder::new()
            .current_account_id(accounts(5))
            .predecessor_account_id(account)
            .attached_deposit(deposit)
            .build());
    }

    fn token() -> AccountId {
        accounts(4)
    }

    fn recipient() -> AccountId {
        accounts(2)
    }

    fn public_key(seed: u8) -> PublicKey {
        let mut bytes = vec![0u8];
        bytes.extend_from_slice(
            SigningKey::from_bytes(&[seed; 32])
                .verifying_key()
                .as_bytes(),
        );
        PublicKey::try_from(bytes).unwrap()
    }

    /// Signs exactly what the enclave signs, using the service's own digest.
    fn sign(seed: u8, index: u32, to: &AccountId, amount: u128) -> Base64VecU8 {
        let company = CompanyId::new(COMPANY).unwrap();
        let digest =
            PayoutInstruction::signing_digest(&company, index, to.as_str(), Money::new(amount))
                .unwrap();
        let signature = SigningKey::from_bytes(&[seed; 32]).sign(digest.as_bytes());
        Base64VecU8::from(signature.to_bytes().to_vec())
    }

    fn opened() -> DistributionRegistry {
        act_as(accounts(0), DEPOSIT);
        let mut registry = DistributionRegistry::new();
        registry.open(
            COMPANY.to_owned(),
            token(),
            public_key(ENCLAVE_SEED),
            U128(TOTAL),
        );
        registry
    }

    fn fund(registry: &mut DistributionRegistry, amount: u128) -> u128 {
        act_as(token(), NearToken::from_yoctonear(0));
        let unused = registry.ft_on_transfer(accounts(0), U128(amount), COMPANY.to_owned());
        act_as(accounts(1), DEPOSIT);
        match unused {
            PromiseOrValue::Value(unused) => unused.0,
            PromiseOrValue::Promise(_) => panic!("expected a value"),
        }
    }

    fn funded() -> DistributionRegistry {
        let mut registry = opened();
        fund(&mut registry, TOTAL);
        registry
    }

    fn pay(registry: &mut DistributionRegistry, seed: u8, index: u32, amount: u128) {
        let signature = sign(seed, index, &recipient(), amount);
        drop(registry.pay(
            COMPANY.to_owned(),
            index,
            recipient(),
            U128(amount),
            signature,
        ));
    }

    fn paid_total(registry: &DistributionRegistry) -> u128 {
        registry.distribution(COMPANY.to_owned()).unwrap().paid.0
    }

    #[test]
    fn deposits_beyond_the_total_go_back() {
        let mut registry = opened();
        assert_eq!(fund(&mut registry, 70), 0);
        assert_eq!(fund(&mut registry, 70), 40);
        assert_eq!(
            registry.distribution(COMPANY.to_owned()).unwrap().funded.0,
            TOTAL
        );
    }

    #[test]
    #[should_panic(expected = "wrong token")]
    fn another_token_cannot_fund_it() {
        let mut registry = opened();
        act_as(accounts(3), NearToken::from_yoctonear(0));
        drop(registry.ft_on_transfer(accounts(0), U128(1), COMPANY.to_owned()));
    }

    #[test]
    fn an_enclave_signed_payment_can_be_relayed_by_anyone() {
        let mut registry = funded();
        pay(&mut registry, ENCLAVE_SEED, 0, 60);
        assert!(registry.is_paid(COMPANY.to_owned(), 0));
        assert_eq!(paid_total(&registry), 60);
    }

    #[test]
    #[should_panic(expected = "invalid enclave signature")]
    fn another_key_cannot_order_a_payment() {
        let mut registry = funded();
        pay(&mut registry, FORGER_SEED, 0, 60);
    }

    #[test]
    #[should_panic(expected = "invalid enclave signature")]
    fn a_relay_cannot_change_the_recipient() {
        let mut registry = funded();
        let signature = sign(ENCLAVE_SEED, 0, &recipient(), 60);
        drop(registry.pay(COMPANY.to_owned(), 0, accounts(3), U128(60), signature));
    }

    #[test]
    #[should_panic(expected = "invalid enclave signature")]
    fn a_relay_cannot_change_the_amount() {
        let mut registry = funded();
        let signature = sign(ENCLAVE_SEED, 0, &recipient(), 60);
        drop(registry.pay(COMPANY.to_owned(), 0, recipient(), U128(90), signature));
    }

    #[test]
    #[should_panic(expected = "already paid")]
    fn a_payment_cannot_be_made_twice() {
        let mut registry = funded();
        pay(&mut registry, ENCLAVE_SEED, 0, 10);
        pay(&mut registry, ENCLAVE_SEED, 0, 10);
    }

    #[test]
    #[should_panic(expected = "not enough funds")]
    fn it_never_pays_more_than_was_deposited() {
        let mut registry = funded();
        pay(&mut registry, ENCLAVE_SEED, 0, 60);
        pay(&mut registry, ENCLAVE_SEED, 1, 60);
    }

    #[test]
    fn a_failed_transfer_can_be_retried() {
        let mut registry = funded();
        pay(&mut registry, ENCLAVE_SEED, 0, 60);

        testing_env!(
            VMContextBuilder::new()
                .current_account_id(accounts(5))
                .predecessor_account_id(accounts(5))
                .build(),
            near_sdk::test_vm_config(),
            RuntimeFeesConfig::test(),
            std::collections::HashMap::default(),
            vec![PromiseResult::Failed],
        );
        registry.on_paid(COMPANY.to_owned(), 0, U128(60), Err(PromiseError::Failed));

        assert!(!registry.is_paid(COMPANY.to_owned(), 0));
        assert_eq!(paid_total(&registry), 0);
        act_as(accounts(1), DEPOSIT);
        pay(&mut registry, ENCLAVE_SEED, 0, 60);
        assert_eq!(paid_total(&registry), 60);
    }

    #[test]
    #[should_panic(expected = "distribution already exists")]
    fn a_distribution_cannot_be_replaced() {
        let mut registry = opened();
        registry.open(
            COMPANY.to_owned(),
            token(),
            public_key(FORGER_SEED),
            U128(TOTAL),
        );
    }
}
