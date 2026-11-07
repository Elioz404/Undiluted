//! Public commitments to private cap tables.
//!
//! The contract stores only digests. It knows nothing about instruments or
//! holders, yet because a head can never be replaced, a company cannot show
//! two different histories to two different investors.

use near_sdk::json_types::Base58CryptoHash;
use near_sdk::store::LookupMap;
use near_sdk::{
    env, near, require, AccountId, BorshStorageKey, CryptoHash, NearToken, PanicOnDefault, Promise,
};

#[derive(BorshStorageKey)]
#[near]
enum StorageKey {
    Companies,
    Heads,
}

#[derive(Clone, Debug, PartialEq, Eq)]
#[near(serializers = [borsh, json])]
pub struct Company {
    pub authority: AccountId,
    pub next_sequence: u64,
}

#[near(event_json(standard = "undiluted_anchor"))]
pub enum AnchorEvent {
    #[event_version("1.0.0")]
    CompanyRegistered {
        company_id: String,
        authority: AccountId,
    },
    #[event_version("1.0.0")]
    AuthorityTransferred {
        company_id: String,
        authority: AccountId,
    },
    #[event_version("1.0.0")]
    HeadAnchored {
        company_id: String,
        sequence: u64,
        head: Base58CryptoHash,
    },
}

#[near(contract_state)]
#[derive(PanicOnDefault)]
pub struct AnchorRegistry {
    companies: LookupMap<String, Company>,
    heads: LookupMap<(String, u64), CryptoHash>,
}

#[near]
impl AnchorRegistry {
    #[init]
    #[must_use]
    pub fn new() -> Self {
        Self {
            companies: LookupMap::new(StorageKey::Companies),
            heads: LookupMap::new(StorageKey::Heads),
        }
    }

    /// Claims a company id for the caller, who becomes its authority.
    #[payable]
    pub fn register_company(&mut self, company_id: String) {
        let storage_before = env::storage_usage();
        require!(!company_id.is_empty(), "company id must not be empty");
        require!(
            !self.companies.contains_key(&company_id),
            "company id is already registered"
        );

        let authority = env::predecessor_account_id();
        self.companies.insert(
            company_id.clone(),
            Company {
                authority: authority.clone(),
                next_sequence: 0,
            },
        );
        self.companies.flush();
        settle_storage(storage_before);

        AnchorEvent::CompanyRegistered {
            company_id,
            authority,
        }
        .emit();
    }

    /// Hands control over to another account, e.g. a key held by an enclave.
    #[payable]
    pub fn transfer_authority(&mut self, company_id: String, authority: AccountId) {
        near_sdk::assert_one_yocto();
        let company = self.company_for_authority(&company_id);
        company.authority = authority.clone();

        AnchorEvent::AuthorityTransferred {
            company_id,
            authority,
        }
        .emit();
    }

    /// Records the head for `sequence`. Sequences must arrive in order and a
    /// recorded head can never be changed.
    #[payable]
    pub fn anchor(&mut self, company_id: String, sequence: u64, head: Base58CryptoHash) {
        let storage_before = env::storage_usage();
        let company = self.company_for_authority(&company_id);
        require!(sequence == company.next_sequence, "unexpected sequence");
        company.next_sequence += 1;
        self.companies.flush();

        self.heads
            .insert((company_id.clone(), sequence), CryptoHash::from(head));
        self.heads.flush();
        settle_storage(storage_before);

        AnchorEvent::HeadAnchored {
            company_id,
            sequence,
            head,
        }
        .emit();
    }

    #[must_use]
    pub fn company(&self, company_id: String) -> Option<Company> {
        self.companies.get(&company_id).cloned()
    }

    #[must_use]
    pub fn head_at(&self, company_id: String, sequence: u64) -> Option<Base58CryptoHash> {
        self.heads
            .get(&(company_id, sequence))
            .copied()
            .map(Base58CryptoHash::from)
    }

    fn company_for_authority(&mut self, company_id: &str) -> &mut Company {
        let company = self
            .companies
            .get_mut(company_id)
            .unwrap_or_else(|| env::panic_str("unknown company"));
        require!(
            company.authority == env::predecessor_account_id(),
            "only the company authority can do this"
        );
        company
    }
}

/// Charges the caller for the storage just used and refunds the rest.
fn settle_storage(storage_before: u64) {
    let used = env::storage_usage().saturating_sub(storage_before);
    let cost = env::storage_byte_cost().saturating_mul(u128::from(used));
    let attached = env::attached_deposit();
    require!(attached >= cost, "attached deposit does not cover storage");

    let refund = attached.saturating_sub(cost);
    if refund > NearToken::from_yoctonear(0) {
        Promise::new(env::predecessor_account_id())
            .transfer(refund)
            .detach();
    }
}

#[cfg(test)]
mod tests {
    use near_sdk::test_utils::{accounts, VMContextBuilder};
    use near_sdk::testing_env;

    use super::*;

    const COMPANY: &str = "acme";
    const DEPOSIT: NearToken = NearToken::from_millinear(100);

    fn act_as(account: usize, deposit: NearToken) {
        testing_env!(VMContextBuilder::new()
            .predecessor_account_id(accounts(account))
            .attached_deposit(deposit)
            .build());
    }

    fn head(byte: u8) -> Base58CryptoHash {
        Base58CryptoHash::from([byte; 32])
    }

    fn registered() -> AnchorRegistry {
        act_as(0, DEPOSIT);
        let mut registry = AnchorRegistry::new();
        registry.register_company(COMPANY.to_owned());
        registry
    }

    #[test]
    fn registering_makes_the_caller_the_authority() {
        let registry = registered();
        assert_eq!(
            registry.company(COMPANY.to_owned()),
            Some(Company {
                authority: accounts(0),
                next_sequence: 0,
            })
        );
    }

    #[test]
    #[should_panic(expected = "company id is already registered")]
    fn company_ids_cannot_be_claimed_twice() {
        let mut registry = registered();
        act_as(1, DEPOSIT);
        registry.register_company(COMPANY.to_owned());
    }

    #[test]
    #[should_panic(expected = "attached deposit does not cover storage")]
    fn registering_requires_paying_for_storage() {
        act_as(0, NearToken::from_yoctonear(0));
        let mut registry = AnchorRegistry::new();
        registry.register_company(COMPANY.to_owned());
    }

    #[test]
    fn anchored_heads_can_be_read_back() {
        let mut registry = registered();
        registry.anchor(COMPANY.to_owned(), 0, head(1));
        registry.anchor(COMPANY.to_owned(), 1, head(2));

        assert_eq!(registry.head_at(COMPANY.to_owned(), 0), Some(head(1)));
        assert_eq!(registry.head_at(COMPANY.to_owned(), 1), Some(head(2)));
        assert_eq!(registry.head_at(COMPANY.to_owned(), 2), None);
    }

    #[test]
    #[should_panic(expected = "unexpected sequence")]
    fn a_recorded_head_cannot_be_overwritten() {
        let mut registry = registered();
        registry.anchor(COMPANY.to_owned(), 0, head(1));
        registry.anchor(COMPANY.to_owned(), 0, head(9));
    }

    #[test]
    #[should_panic(expected = "unexpected sequence")]
    fn sequences_cannot_be_skipped() {
        let mut registry = registered();
        registry.anchor(COMPANY.to_owned(), 1, head(1));
    }

    #[test]
    #[should_panic(expected = "only the company authority can do this")]
    fn only_the_authority_can_anchor() {
        let mut registry = registered();
        act_as(1, DEPOSIT);
        registry.anchor(COMPANY.to_owned(), 0, head(1));
    }

    #[test]
    #[should_panic(expected = "unknown company")]
    fn unknown_companies_cannot_anchor() {
        let mut registry = registered();
        registry.anchor("other".to_owned(), 0, head(1));
    }

    #[test]
    fn authority_can_be_transferred() {
        let mut registry = registered();
        act_as(0, NearToken::from_yoctonear(1));
        registry.transfer_authority(COMPANY.to_owned(), accounts(1));

        act_as(1, DEPOSIT);
        registry.anchor(COMPANY.to_owned(), 0, head(1));
        assert_eq!(registry.head_at(COMPANY.to_owned(), 0), Some(head(1)));
    }

    #[test]
    #[should_panic(expected = "only the company authority can do this")]
    fn the_previous_authority_loses_access() {
        let mut registry = registered();
        act_as(0, NearToken::from_yoctonear(1));
        registry.transfer_authority(COMPANY.to_owned(), accounts(1));

        act_as(0, DEPOSIT);
        registry.anchor(COMPANY.to_owned(), 0, head(1));
    }
}
