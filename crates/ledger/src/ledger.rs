use std::collections::BTreeMap;

use undiluted_domain::{CapTable, CompanyId, HolderId};

use crate::crypto::{Digest, PublicKey};
use crate::entry::{Draft, Entry, entry_digest, genesis_digest};
use crate::error::LedgerError;
use crate::event::Event;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LedgerConfig {
    pub company: CompanyId,
    pub company_key: PublicKey,
}

#[derive(Debug, Clone)]
pub struct Ledger {
    config: LedgerConfig,
    entries: Vec<Entry>,
    heads: Vec<Digest>,
    genesis: Digest,
    holder_keys: BTreeMap<HolderId, PublicKey>,
    table: CapTable,
}

impl Ledger {
    pub fn new(config: LedgerConfig) -> Result<Self, LedgerError> {
        let genesis = genesis_digest(&config.company, &config.company_key)?;
        Ok(Self {
            config,
            entries: Vec::new(),
            heads: Vec::new(),
            genesis,
            holder_keys: BTreeMap::new(),
            table: CapTable::new(),
        })
    }

    /// Rebuilds a ledger from its entries, verifying every link and signature.
    pub fn replay(
        config: LedgerConfig,
        entries: impl IntoIterator<Item = Entry>,
    ) -> Result<Self, LedgerError> {
        let mut ledger = Self::new(config)?;
        for entry in entries {
            ledger.append(entry)?;
        }
        Ok(ledger)
    }

    #[must_use]
    pub const fn table(&self) -> &CapTable {
        &self.table
    }

    #[must_use]
    pub fn entries(&self) -> &[Entry] {
        &self.entries
    }

    /// Digest committing to the whole history so far.
    #[must_use]
    pub fn head(&self) -> Digest {
        self.heads.last().copied().unwrap_or(self.genesis)
    }

    /// Digest committing to the history up to and including `sequence`.
    #[must_use]
    pub fn head_at(&self, sequence: u64) -> Option<Digest> {
        usize::try_from(sequence)
            .ok()
            .and_then(|index| self.heads.get(index))
            .copied()
    }

    /// Prepares `event` for signing, rejecting it early if it could not apply.
    pub fn draft(&self, event: Event) -> Result<Draft, LedgerError> {
        self.state_after(&event)?;
        let required_holders = event.affected_holders(&self.table)?;
        let sequence = self.next_sequence();
        let previous = self.head();
        let digest = entry_digest(&self.config.company, sequence, &previous, &event)?;
        Ok(Draft {
            sequence,
            previous,
            event,
            digest,
            required_holders,
        })
    }

    /// Verifies and records an entry. On any error the ledger is unchanged.
    pub fn append(&mut self, entry: Entry) -> Result<Digest, LedgerError> {
        let expected = self.next_sequence();
        if entry.sequence != expected {
            return Err(LedgerError::SequenceMismatch {
                expected,
                actual: entry.sequence,
            });
        }
        if entry.previous != self.head() {
            return Err(LedgerError::BrokenChain {
                sequence: entry.sequence,
            });
        }

        let digest = entry_digest(
            &self.config.company,
            entry.sequence,
            &entry.previous,
            &entry.event,
        )?;
        self.verify_signatures(&entry, &digest)?;
        let (table, new_holder) = self.state_after(&entry.event)?;

        self.table = table;
        if let Some((holder, key)) = new_holder {
            self.holder_keys.insert(holder, key);
        }
        self.entries.push(entry);
        self.heads.push(digest);
        Ok(digest)
    }

    fn next_sequence(&self) -> u64 {
        self.entries.len() as u64
    }

    fn state_after(
        &self,
        event: &Event,
    ) -> Result<(CapTable, Option<(HolderId, PublicKey)>), LedgerError> {
        let new_holder = match event {
            Event::HolderRegistered { holder, key } => {
                if self.holder_keys.contains_key(holder) {
                    return Err(LedgerError::HolderAlreadyRegistered(holder.clone()));
                }
                Some((holder.clone(), *key))
            }
            _ => None,
        };
        let mut table = self.table.clone();
        event.apply_to(&mut table)?;
        Ok((table, new_holder))
    }

    fn verify_signatures(&self, entry: &Entry, digest: &Digest) -> Result<(), LedgerError> {
        if !self
            .config
            .company_key
            .verifies(digest, &entry.company_signature)
        {
            return Err(LedgerError::InvalidCompanySignature {
                sequence: entry.sequence,
            });
        }
        for (holder, key) in self.required_signers(&entry.event)? {
            let signature = entry
                .holder_signatures
                .iter()
                .find(|candidate| candidate.holder == holder)
                .ok_or_else(|| LedgerError::MissingHolderSignature {
                    sequence: entry.sequence,
                    holder: holder.clone(),
                })?;
            if !key.verifies(digest, &signature.signature) {
                return Err(LedgerError::InvalidHolderSignature {
                    sequence: entry.sequence,
                    holder,
                });
            }
        }
        Ok(())
    }

    fn required_signers(&self, event: &Event) -> Result<Vec<(HolderId, PublicKey)>, LedgerError> {
        // A new holder proves control of the key being registered.
        if let Event::HolderRegistered { holder, key } = event {
            return Ok(vec![(holder.clone(), *key)]);
        }
        event
            .affected_holders(&self.table)?
            .into_iter()
            .map(|holder| {
                let key = self
                    .holder_keys
                    .get(&holder)
                    .copied()
                    .ok_or_else(|| LedgerError::UnknownHolder(holder.clone()))?;
                Ok((holder, key))
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use ed25519_dalek::{Signer, SigningKey};
    use undiluted_domain::{
        InstrumentId, Money, NewInvestment, Position, PricedRound, Safe, SafeTerms, Shares,
    };

    use super::*;
    use crate::crypto::SignatureBytes;
    use crate::entry::HolderSignature;
    use crate::event::SettlementReference;

    const COMPANY_SEED: u8 = 1;
    const ANA_SEED: u8 = 2;
    const BEN_SEED: u8 = 3;
    const MALLORY_SEED: u8 = 9;

    fn signing_key(seed: u8) -> SigningKey {
        SigningKey::from_bytes(&[seed; 32])
    }

    fn public_key(seed: u8) -> PublicKey {
        PublicKey::from_bytes(signing_key(seed).verifying_key().to_bytes())
    }

    fn sign(seed: u8, digest: &Digest) -> SignatureBytes {
        SignatureBytes::from_bytes(signing_key(seed).sign(digest.as_bytes()).to_bytes())
    }

    fn config() -> LedgerConfig {
        LedgerConfig {
            company: CompanyId::new("acme").unwrap(),
            company_key: public_key(COMPANY_SEED),
        }
    }

    fn holder(name: &str) -> HolderId {
        HolderId::new(name).unwrap()
    }

    fn instrument(name: &str) -> InstrumentId {
        InstrumentId::new(name).unwrap()
    }

    fn usd(dollars: u128) -> Money {
        Money::new(dollars * 1_000_000)
    }

    fn safe_event(id: &str, owner: &str, purchase: u128, cap: u128) -> Event {
        Event::SafeIssued {
            instrument: instrument(id),
            holder: holder(owner),
            safe: Safe::new(
                usd(purchase),
                SafeTerms::new(Some(usd(cap)), None, false).unwrap(),
            )
            .unwrap(),
            settlement: SettlementReference::new("tx-1").unwrap(),
        }
    }

    /// Signs with the company and the given holders, then appends.
    fn commit(ledger: &mut Ledger, event: Event, holders: &[(&str, u8)]) -> Digest {
        let draft = ledger.draft(event).unwrap();
        let company = sign(COMPANY_SEED, &draft.digest);
        let signatures = holders
            .iter()
            .map(|(name, seed)| HolderSignature {
                holder: holder(name),
                signature: sign(*seed, &draft.digest),
            })
            .collect();
        ledger.append(draft.seal(company, signatures)).unwrap()
    }

    fn register(ledger: &mut Ledger, name: &str, seed: u8) {
        commit(
            ledger,
            Event::HolderRegistered {
                holder: holder(name),
                key: public_key(seed),
            },
            &[(name, seed)],
        );
    }

    fn funded_ledger() -> Ledger {
        let mut ledger = Ledger::new(config()).unwrap();
        register(&mut ledger, "ana", ANA_SEED);
        register(&mut ledger, "ben", BEN_SEED);
        commit(
            &mut ledger,
            Event::CommonIssued {
                instrument: instrument("founders"),
                holder: holder("ana"),
                shares: Shares::new(8_000_000),
            },
            &[("ana", ANA_SEED)],
        );
        commit(
            &mut ledger,
            safe_event("safe-ben", "ben", 500_000, 5_000_000),
            &[("ben", BEN_SEED)],
        );
        ledger
    }

    #[test]
    fn events_rebuild_the_cap_table() {
        let ledger = funded_ledger();
        assert_eq!(ledger.table().issued_shares(), Ok(Shares::new(8_000_000)));
        assert!(matches!(
            ledger
                .table()
                .holding(&instrument("safe-ben"))
                .unwrap()
                .position(),
            Position::Safe(_)
        ));
    }

    #[test]
    fn replay_reaches_the_same_state_and_head() {
        let original = funded_ledger();
        let replayed = Ledger::replay(config(), original.entries().to_vec()).unwrap();
        assert_eq!(replayed.head(), original.head());
        assert_eq!(replayed.table(), original.table());
    }

    #[test]
    fn each_company_starts_from_its_own_head() {
        let other = LedgerConfig {
            company: CompanyId::new("globex").unwrap(),
            company_key: public_key(COMPANY_SEED),
        };
        assert_ne!(
            Ledger::new(config()).unwrap().head(),
            Ledger::new(other).unwrap().head()
        );
    }

    #[test]
    fn changing_a_past_event_breaks_replay() {
        let mut entries = funded_ledger().entries().to_vec();
        entries[3].event = safe_event("safe-ben", "ben", 1, 5_000_000);

        assert_eq!(
            Ledger::replay(config(), entries).unwrap_err(),
            LedgerError::InvalidCompanySignature { sequence: 3 }
        );
    }

    #[test]
    fn removing_a_past_entry_breaks_replay() {
        let mut entries = funded_ledger().entries().to_vec();
        entries.remove(1);

        assert_eq!(
            Ledger::replay(config(), entries).unwrap_err(),
            LedgerError::SequenceMismatch {
                expected: 1,
                actual: 2
            }
        );
    }

    /// Even a dishonest company that re-signs everything cannot rewrite the
    /// past unnoticed: the head a holder kept no longer matches.
    #[test]
    fn a_company_rewriting_history_changes_the_heads_holders_kept() {
        let honest = funded_ledger();
        let kept_by_ben = honest.head_at(3).unwrap();

        let mut rewritten = Ledger::new(config()).unwrap();
        register(&mut rewritten, "ana", ANA_SEED);
        register(&mut rewritten, "ben", BEN_SEED);
        commit(
            &mut rewritten,
            Event::CommonIssued {
                instrument: instrument("founders"),
                holder: holder("ana"),
                shares: Shares::new(80_000_000),
            },
            &[("ana", ANA_SEED)],
        );
        commit(
            &mut rewritten,
            safe_event("safe-ben", "ben", 500_000, 5_000_000),
            &[("ben", BEN_SEED)],
        );

        assert_ne!(rewritten.head_at(3).unwrap(), kept_by_ben);
    }

    #[test]
    fn an_instrument_cannot_be_issued_without_its_holder() {
        let mut ledger = funded_ledger();
        let draft = ledger.draft(safe_event("s2", "ben", 1, 10)).unwrap();
        let company = sign(COMPANY_SEED, &draft.digest);
        let sequence = draft.sequence;

        assert_eq!(
            ledger.append(draft.seal(company, vec![])).unwrap_err(),
            LedgerError::MissingHolderSignature {
                sequence,
                holder: holder("ben"),
            }
        );
    }

    #[test]
    fn a_holder_signature_from_the_wrong_key_is_rejected() {
        let mut ledger = funded_ledger();
        let draft = ledger.draft(safe_event("s2", "ben", 1, 10)).unwrap();
        let company = sign(COMPANY_SEED, &draft.digest);
        let forged = HolderSignature {
            holder: holder("ben"),
            signature: sign(MALLORY_SEED, &draft.digest),
        };
        let sequence = draft.sequence;

        assert_eq!(
            ledger
                .append(draft.seal(company, vec![forged]))
                .unwrap_err(),
            LedgerError::InvalidHolderSignature {
                sequence,
                holder: holder("ben"),
            }
        );
    }

    #[test]
    fn only_the_company_key_can_sign_entries() {
        let mut ledger = funded_ledger();
        let draft = ledger
            .draft(Event::OptionPoolResized {
                shares: Shares::new(1),
            })
            .unwrap();
        let forged = sign(MALLORY_SEED, &draft.digest);
        let sequence = draft.sequence;

        assert_eq!(
            ledger.append(draft.seal(forged, vec![])).unwrap_err(),
            LedgerError::InvalidCompanySignature { sequence }
        );
    }

    #[test]
    fn a_new_holder_must_prove_their_key() {
        let mut ledger = Ledger::new(config()).unwrap();
        let draft = ledger
            .draft(Event::HolderRegistered {
                holder: holder("ana"),
                key: public_key(ANA_SEED),
            })
            .unwrap();
        let company = sign(COMPANY_SEED, &draft.digest);
        let wrong = HolderSignature {
            holder: holder("ana"),
            signature: sign(MALLORY_SEED, &draft.digest),
        };

        assert!(matches!(
            ledger.append(draft.seal(company, vec![wrong])),
            Err(LedgerError::InvalidHolderSignature { .. })
        ));
    }

    #[test]
    fn holders_cannot_be_registered_twice() {
        let ledger = funded_ledger();
        assert_eq!(
            ledger
                .draft(Event::HolderRegistered {
                    holder: holder("ana"),
                    key: public_key(MALLORY_SEED),
                })
                .unwrap_err(),
            LedgerError::HolderAlreadyRegistered(holder("ana"))
        );
    }

    #[test]
    fn unregistered_holders_cannot_sign_for_instruments() {
        let mut ledger = funded_ledger();
        let draft = ledger.draft(safe_event("s2", "stranger", 1, 10)).unwrap();
        let company = sign(COMPANY_SEED, &draft.digest);

        assert_eq!(
            ledger.append(draft.seal(company, vec![])).unwrap_err(),
            LedgerError::UnknownHolder(holder("stranger"))
        );
    }

    #[test]
    fn invalid_events_are_rejected_when_drafting() {
        let ledger = funded_ledger();
        assert!(matches!(
            ledger.draft(safe_event("safe-ben", "ben", 1, 10)),
            Err(LedgerError::CapTable(_))
        ));
    }

    #[test]
    fn a_failed_append_changes_nothing() {
        let mut ledger = funded_ledger();
        let head = ledger.head();
        let draft = ledger
            .draft(Event::OptionPoolResized {
                shares: Shares::new(1),
            })
            .unwrap();
        let forged = sign(MALLORY_SEED, &draft.digest);

        assert!(ledger.append(draft.seal(forged, vec![])).is_err());
        assert_eq!(ledger.head(), head);
        assert_eq!(ledger.table().option_pool(), Shares::ZERO);
    }

    #[test]
    fn closing_a_round_needs_the_new_investors() {
        let mut ledger = funded_ledger();
        let round = Event::RoundClosed {
            round: PricedRound {
                pre_money: usd(20_000_000),
                investments: vec![NewInvestment {
                    instrument: instrument("series-a"),
                    holder: holder("ana"),
                    amount: usd(1_000_000),
                }],
            },
        };

        let draft = ledger.draft(round.clone()).unwrap();
        assert_eq!(draft.required_holders, vec![holder("ana")]);

        commit(&mut ledger, round, &[("ana", ANA_SEED)]);
        assert!(matches!(
            ledger
                .table()
                .holding(&instrument("safe-ben"))
                .unwrap()
                .position(),
            Position::Preferred { .. }
        ));
    }
}
