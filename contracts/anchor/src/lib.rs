//! Public commitments to private cap tables.
//!
//! The contract stores only digests. It knows nothing about instruments or
//! holders, yet a head is accepted only with the enclave's signature and can
//! never be replaced. So whoever relays it cannot forge a history, and a company
//! cannot show two different histories to two different investors.

use near_sdk::borsh::to_vec;
use near_sdk::json_types::{Base58CryptoHash, Base64VecU8};
use near_sdk::store::LookupMap;
use near_sdk::{
    env, near, BorshStorageKey, CryptoHash, CurveType, NearToken, PanicOnDefault, Promise,
    PublicKey,
};

/// Must match the attestation the enclave signs in the service crate. A test
/// checks real attestations against this contract so the two cannot drift.
const ATTESTATION_TAG: &str = "undiluted/service/anchor/v1";

const REFUSAL_MARKER: &str = "undiluted refused: ";

#[derive(BorshStorageKey)]
#[near]
enum StorageKey {
    Companies,
    Heads,
}

#[derive(Clone, Debug, PartialEq, Eq)]
#[near(serializers = [borsh, json])]
pub struct Company {
    /// The only key whose signature can extend this company's history. It cannot
    /// be changed, so nobody can take over a history later.
    pub enclave_key: PublicKey,
    pub next_sequence: u64,
}

#[near(event_json(standard = "undiluted_anchor"))]
pub enum AnchorEvent {
    #[event_version("1.0.0")]
    CompanyRegistered {
        company_id: String,
        enclave_key: PublicKey,
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

    /// Ties a company id to an enclave's signing key. Investors compare this
    /// key with the enclave they talk to.
    #[payable]
    pub fn register_company(&mut self, company_id: String, enclave_key: PublicKey) {
        let storage_before = env::storage_usage();
        ensure(!company_id.is_empty(), "company id must not be empty");
        ensure(
            enclave_key.curve_type() == CurveType::ED25519,
            "enclave key must be ed25519",
        );
        ensure(
            !self.companies.contains_key(&company_id),
            "company id is already registered",
        );

        self.companies.insert(
            company_id.clone(),
            Company {
                enclave_key: enclave_key.clone(),
                next_sequence: 0,
            },
        );
        self.companies.flush();
        settle_storage(storage_before);

        AnchorEvent::CompanyRegistered {
            company_id,
            enclave_key,
        }
        .emit();
    }

    /// Records the head for `sequence`. Anyone may relay it, because only the
    /// enclave's signature makes it valid. Sequences must arrive in order and a
    /// recorded head can never be changed.
    #[payable]
    pub fn anchor(
        &mut self,
        company_id: String,
        sequence: u64,
        head: Base58CryptoHash,
        signature: Base64VecU8,
    ) {
        let storage_before = env::storage_usage();
        let head_bytes = CryptoHash::from(head);
        let company = self
            .companies
            .get_mut(&company_id)
            .unwrap_or_else(|| refuse("unknown company"));
        ensure(sequence == company.next_sequence, "unexpected sequence");
        ensure(
            is_valid_attestation(
                &company.enclave_key,
                &company_id,
                sequence,
                &head_bytes,
                &signature.0,
            ),
            "invalid enclave signature",
        );

        company.next_sequence += 1;
        self.companies.flush();
        self.heads
            .insert((company_id.clone(), sequence), head_bytes);
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

fn is_valid_attestation(
    enclave_key: &PublicKey,
    company_id: &str,
    sequence: u64,
    head: &CryptoHash,
    signature: &[u8],
) -> bool {
    let Ok(signature) = <&[u8; 64]>::try_from(signature) else {
        return false;
    };
    let Ok(key) = <&[u8; 32]>::try_from(&enclave_key.as_bytes()[1..]) else {
        return false;
    };
    let Ok(payload) = to_vec(&(company_id, sequence, head)) else {
        return false;
    };

    let mut tagged = Vec::with_capacity(8 + ATTESTATION_TAG.len() + payload.len());
    tagged.extend_from_slice(&(ATTESTATION_TAG.len() as u64).to_le_bytes());
    tagged.extend_from_slice(ATTESTATION_TAG.as_bytes());
    tagged.extend_from_slice(&payload);
    env::ed25519_verify(signature, env::sha256_array(&tagged), key)
}

/// Charges the caller for the storage just used and refunds the rest.
fn settle_storage(storage_before: u64) {
    let used = env::storage_usage().saturating_sub(storage_before);
    let cost = env::storage_byte_cost().saturating_mul(u128::from(used));
    let attached = env::attached_deposit();
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
    use near_sdk::testing_env;
    use undiluted_domain::CompanyId;
    use undiluted_ledger::Digest;
    use undiluted_service::HeadAttestation;

    use super::*;

    const COMPANY: &str = "acme";
    const DEPOSIT: NearToken = NearToken::from_millinear(100);
    const ENCLAVE_SEED: u8 = 7;
    const FORGER_SEED: u8 = 9;

    fn act_as(account: usize, deposit: NearToken) {
        testing_env!(VMContextBuilder::new()
            .predecessor_account_id(accounts(account))
            .attached_deposit(deposit)
            .build());
    }

    fn signing_key(seed: u8) -> SigningKey {
        SigningKey::from_bytes(&[seed; 32])
    }

    fn public_key(seed: u8) -> PublicKey {
        let mut bytes = vec![0u8];
        bytes.extend_from_slice(signing_key(seed).verifying_key().as_bytes());
        PublicKey::try_from(bytes).unwrap()
    }

    /// Signs exactly what the enclave signs, using the service's own digest.
    fn attest(seed: u8, sequence: u64, head: [u8; 32]) -> (Base58CryptoHash, Base64VecU8) {
        let company = CompanyId::new(COMPANY).unwrap();
        let digest =
            HeadAttestation::signing_digest(&company, sequence, &Digest::from_bytes(head)).unwrap();
        let signature = signing_key(seed)
            .sign(digest.as_bytes())
            .to_bytes()
            .to_vec();
        (Base58CryptoHash::from(head), Base64VecU8::from(signature))
    }

    fn registered() -> AnchorRegistry {
        act_as(0, DEPOSIT);
        let mut registry = AnchorRegistry::new();
        registry.register_company(COMPANY.to_owned(), public_key(ENCLAVE_SEED));
        registry
    }

    fn anchor(registry: &mut AnchorRegistry, seed: u8, sequence: u64, byte: u8) {
        let (head, signature) = attest(seed, sequence, [byte; 32]);
        registry.anchor(COMPANY.to_owned(), sequence, head, signature);
    }

    #[test]
    fn registering_records_the_enclave_key() {
        let registry = registered();
        assert_eq!(
            registry.company(COMPANY.to_owned()),
            Some(Company {
                enclave_key: public_key(ENCLAVE_SEED),
                next_sequence: 0,
            })
        );
    }

    #[test]
    #[should_panic(expected = "company id is already registered")]
    fn company_ids_cannot_be_claimed_twice() {
        let mut registry = registered();
        registry.register_company(COMPANY.to_owned(), public_key(FORGER_SEED));
    }

    #[test]
    #[should_panic(expected = "attached deposit does not cover storage")]
    fn registering_requires_paying_for_storage() {
        act_as(0, NearToken::from_yoctonear(0));
        let mut registry = AnchorRegistry::new();
        registry.register_company(COMPANY.to_owned(), public_key(ENCLAVE_SEED));
    }

    #[test]
    fn attestations_signed_by_the_enclave_are_accepted_from_any_relay() {
        let mut registry = registered();
        anchor(&mut registry, ENCLAVE_SEED, 0, 1);

        act_as(1, DEPOSIT);
        anchor(&mut registry, ENCLAVE_SEED, 1, 2);

        let expected = Base58CryptoHash::from([2u8; 32]);
        assert_eq!(registry.head_at(COMPANY.to_owned(), 1), Some(expected));
        assert_eq!(registry.head_at(COMPANY.to_owned(), 2), None);
    }

    #[test]
    #[should_panic(expected = "invalid enclave signature")]
    fn another_key_cannot_write_history() {
        let mut registry = registered();
        anchor(&mut registry, FORGER_SEED, 0, 1);
    }

    #[test]
    #[should_panic(expected = "invalid enclave signature")]
    fn a_signature_cannot_be_reused_for_another_head() {
        let mut registry = registered();
        let (_, signature) = attest(ENCLAVE_SEED, 0, [1; 32]);
        registry.anchor(
            COMPANY.to_owned(),
            0,
            Base58CryptoHash::from([2u8; 32]),
            signature,
        );
    }

    #[test]
    #[should_panic(expected = "invalid enclave signature")]
    fn a_malformed_signature_is_rejected() {
        let mut registry = registered();
        registry.anchor(
            COMPANY.to_owned(),
            0,
            Base58CryptoHash::from([1u8; 32]),
            Base64VecU8::from(vec![0u8; 10]),
        );
    }

    #[test]
    #[should_panic(expected = "unexpected sequence")]
    fn a_recorded_head_cannot_be_overwritten() {
        let mut registry = registered();
        anchor(&mut registry, ENCLAVE_SEED, 0, 1);
        anchor(&mut registry, ENCLAVE_SEED, 0, 9);
    }

    #[test]
    #[should_panic(expected = "unexpected sequence")]
    fn sequences_cannot_be_skipped() {
        let mut registry = registered();
        anchor(&mut registry, ENCLAVE_SEED, 1, 1);
    }

    #[test]
    #[should_panic(expected = "unknown company")]
    fn unknown_companies_cannot_anchor() {
        let mut registry = registered();
        let (head, signature) = attest(ENCLAVE_SEED, 0, [1; 32]);
        registry.anchor("other".to_owned(), 0, head, signature);
    }
}
