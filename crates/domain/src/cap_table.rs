//! The aggregate that owns every issued instrument of one company.

use borsh::{BorshDeserialize, BorshSerialize};

use crate::error::CapTableError;
use crate::ids::{HolderId, InstrumentId};
use crate::math::Ratio;
use crate::quantity::{BasisPoints, Money, Shares};
use crate::safe::{Safe, SafeTerms};

#[derive(Debug, Clone, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub enum Position {
    Common {
        shares: Shares,
    },
    Preferred {
        shares: Shares,
        liquidation_preference: Money,
    },
    Safe(Safe),
    TokenWarrant {
        allocation: BasisPoints,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct Holding {
    id: InstrumentId,
    holder: HolderId,
    position: Position,
}

impl Holding {
    #[must_use]
    pub const fn id(&self) -> &InstrumentId {
        &self.id
    }

    #[must_use]
    pub const fn holder(&self) -> &HolderId {
        &self.holder
    }

    #[must_use]
    pub const fn position(&self) -> &Position {
        &self.position
    }

    pub(crate) fn replace_position(&mut self, position: Position) {
        self.position = position;
    }
}

/// Instruments are kept in issuance order, which MFN rights depend on.
#[derive(Debug, Clone, Default, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct CapTable {
    holdings: Vec<Holding>,
    option_pool: Shares,
}

impl CapTable {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    #[must_use]
    pub fn holdings(&self) -> &[Holding] {
        &self.holdings
    }

    #[must_use]
    pub const fn option_pool(&self) -> Shares {
        self.option_pool
    }

    pub fn holding(&self, id: &InstrumentId) -> Result<&Holding, CapTableError> {
        self.position_of(id).map(|index| &self.holdings[index])
    }

    pub fn holdings_of<'a>(&'a self, holder: &'a HolderId) -> impl Iterator<Item = &'a Holding> {
        self.holdings.iter().filter(move |h| &h.holder == holder)
    }

    /// Common and preferred shares already issued, excluding the option pool.
    pub fn issued_shares(&self) -> Result<Shares, CapTableError> {
        self.holdings
            .iter()
            .try_fold(Shares::ZERO, |total, holding| match holding.position {
                Position::Common { shares } | Position::Preferred { shares, .. } => {
                    total.checked_add(shares)
                }
                Position::Safe(_) | Position::TokenWarrant { .. } => Ok(total),
            })
    }

    /// Ownership already promised to SAFE holders through valuation caps.
    pub(crate) fn capped_safe_ownership(&self) -> Result<Ratio, CapTableError> {
        self.holdings
            .iter()
            .try_fold(Ratio::ZERO, |total, holding| match &holding.position {
                Position::Safe(safe) => match safe.ownership_at_cap()? {
                    Some(ownership) => total.checked_add(ownership),
                    None => Ok(total),
                },
                _ => Ok(total),
            })
    }

    pub fn issue_common(
        &mut self,
        id: InstrumentId,
        holder: HolderId,
        shares: Shares,
    ) -> Result<(), CapTableError> {
        if shares.is_zero() {
            return Err(CapTableError::ZeroAmount);
        }
        self.push(id, holder, Position::Common { shares })
    }

    pub fn issue_safe(
        &mut self,
        id: InstrumentId,
        holder: HolderId,
        safe: Safe,
    ) -> Result<(), CapTableError> {
        self.push(id, holder, Position::Safe(safe))
    }

    pub fn issue_token_warrant(
        &mut self,
        id: InstrumentId,
        holder: HolderId,
        allocation: BasisPoints,
    ) -> Result<(), CapTableError> {
        if allocation.value() == 0 {
            return Err(CapTableError::ZeroAmount);
        }
        let allocated = self
            .holdings
            .iter()
            .filter_map(|holding| match holding.position {
                Position::TokenWarrant { allocation } => Some(u32::from(allocation.value())),
                _ => None,
            })
            .sum::<u32>();
        if allocated + u32::from(allocation.value()) > u32::from(BasisPoints::WHOLE.value()) {
            return Err(CapTableError::WarrantsOverAllocated);
        }
        self.push(id, holder, Position::TokenWarrant { allocation })
    }

    pub fn set_option_pool(&mut self, shares: Shares) {
        self.option_pool = shares;
    }

    /// SAFEs entitled to adopt the terms of the SAFE identified by `source`.
    pub fn mfn_beneficiaries(&self, source: &InstrumentId) -> Result<Vec<&Holding>, CapTableError> {
        let source_index = self.position_of(source)?;
        let source_terms = *self.safe_terms_at(source_index)?;
        Ok(self.holdings[..source_index]
            .iter()
            .filter(|holding| match &holding.position {
                Position::Safe(safe) => {
                    safe.terms().has_mfn_clause() && source_terms.improves_on(safe.terms())
                }
                _ => false,
            })
            .collect())
    }

    /// Exercises the MFN clause of `target`, adopting the terms of `source`.
    pub fn elect_mfn(
        &mut self,
        target: &InstrumentId,
        source: &InstrumentId,
    ) -> Result<(), CapTableError> {
        let target_index = self.position_of(target)?;
        let source_index = self.position_of(source)?;
        let source_terms = *self.safe_terms_at(source_index)?;
        let target_terms = self.safe_terms_at(target_index)?;

        if !target_terms.has_mfn_clause() {
            return Err(CapTableError::NoMfnClause(target.clone()));
        }
        if source_index <= target_index {
            return Err(CapTableError::MfnSourceNotLater {
                target: target.clone(),
                source_id: source.clone(),
            });
        }
        if !source_terms.improves_on(target_terms) {
            return Err(CapTableError::MfnTermsNotBetter {
                target: target.clone(),
                source_id: source.clone(),
            });
        }

        if let Position::Safe(safe) = &mut self.holdings[target_index].position {
            safe.adopt_terms(source_terms);
        }
        Ok(())
    }

    pub(crate) fn holdings_mut(&mut self) -> &mut [Holding] {
        &mut self.holdings
    }

    pub(crate) fn push(
        &mut self,
        id: InstrumentId,
        holder: HolderId,
        position: Position,
    ) -> Result<(), CapTableError> {
        if self.position_of(&id).is_ok() {
            return Err(CapTableError::DuplicateInstrument(id));
        }
        self.holdings.push(Holding {
            id,
            holder,
            position,
        });
        Ok(())
    }

    fn position_of(&self, id: &InstrumentId) -> Result<usize, CapTableError> {
        self.holdings
            .iter()
            .position(|holding| &holding.id == id)
            .ok_or_else(|| CapTableError::UnknownInstrument(id.clone()))
    }

    fn safe_terms_at(&self, index: usize) -> Result<&SafeTerms, CapTableError> {
        let holding = &self.holdings[index];
        match &holding.position {
            Position::Safe(safe) => Ok(safe.terms()),
            _ => Err(CapTableError::NotASafe(holding.id.clone())),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn usd(dollars: u128) -> Money {
        Money::new(dollars * 1_000_000)
    }

    fn holder(name: &str) -> HolderId {
        HolderId::new(name).unwrap()
    }

    fn instrument(name: &str) -> InstrumentId {
        InstrumentId::new(name).unwrap()
    }

    fn safe(purchase: u128, cap: Option<u128>, mfn: bool) -> Safe {
        let terms = SafeTerms::new(cap.map(usd), None, mfn).unwrap();
        Safe::new(usd(purchase), terms).unwrap()
    }

    fn terms_of(table: &CapTable, id: &str) -> SafeTerms {
        match table.holding(&instrument(id)).unwrap().position() {
            Position::Safe(safe) => *safe.terms(),
            other => panic!("expected a SAFE, found {other:?}"),
        }
    }

    #[test]
    fn instrument_ids_are_unique() {
        let mut table = CapTable::new();
        table
            .issue_common(instrument("a"), holder("ana"), Shares::new(1))
            .unwrap();
        assert_eq!(
            table.issue_common(instrument("a"), holder("ben"), Shares::new(1)),
            Err(CapTableError::DuplicateInstrument(instrument("a")))
        );
    }

    #[test]
    fn issued_shares_ignore_safes_and_the_pool() {
        let mut table = CapTable::new();
        table
            .issue_common(instrument("c"), holder("ana"), Shares::new(100))
            .unwrap();
        table
            .issue_safe(instrument("s"), holder("ben"), safe(1, Some(10), false))
            .unwrap();
        table.set_option_pool(Shares::new(50));
        assert_eq!(table.issued_shares(), Ok(Shares::new(100)));
    }

    #[test]
    fn token_warrants_cannot_exceed_the_whole_allocation() {
        let mut table = CapTable::new();
        table
            .issue_token_warrant(
                instrument("w1"),
                holder("ana"),
                BasisPoints::new(9_000).unwrap(),
            )
            .unwrap();
        assert_eq!(
            table.issue_token_warrant(
                instrument("w2"),
                holder("ben"),
                BasisPoints::new(1_001).unwrap()
            ),
            Err(CapTableError::WarrantsOverAllocated)
        );
    }

    #[test]
    fn better_later_safe_makes_earlier_mfn_holder_a_beneficiary() {
        let mut table = CapTable::new();
        table
            .issue_safe(instrument("mfn"), holder("ana"), safe(100, None, true))
            .unwrap();
        table
            .issue_safe(
                instrument("plain"),
                holder("ben"),
                safe(100, Some(20), false),
            )
            .unwrap();
        table
            .issue_safe(
                instrument("later"),
                holder("eve"),
                safe(100, Some(10), false),
            )
            .unwrap();

        let beneficiaries = table.mfn_beneficiaries(&instrument("later")).unwrap();
        let ids: Vec<&str> = beneficiaries.iter().map(|h| h.id().as_str()).collect();
        assert_eq!(ids, ["mfn"]);
    }

    #[test]
    fn electing_mfn_adopts_the_later_terms() {
        let mut table = CapTable::new();
        table
            .issue_safe(instrument("mfn"), holder("ana"), safe(100, None, true))
            .unwrap();
        table
            .issue_safe(
                instrument("later"),
                holder("eve"),
                safe(100, Some(10), false),
            )
            .unwrap();

        table
            .elect_mfn(&instrument("mfn"), &instrument("later"))
            .unwrap();
        assert_eq!(terms_of(&table, "mfn"), terms_of(&table, "later"));
    }

    #[test]
    fn mfn_election_is_validated() {
        let mut table = CapTable::new();
        table
            .issue_safe(
                instrument("plain"),
                holder("ana"),
                safe(100, Some(50), false),
            )
            .unwrap();
        table
            .issue_safe(instrument("mfn"), holder("ben"), safe(100, Some(40), true))
            .unwrap();
        table
            .issue_safe(
                instrument("worse"),
                holder("eve"),
                safe(100, Some(60), false),
            )
            .unwrap();
        table
            .issue_common(instrument("stock"), holder("ana"), Shares::new(1))
            .unwrap();

        assert_eq!(
            table.elect_mfn(&instrument("plain"), &instrument("worse")),
            Err(CapTableError::NoMfnClause(instrument("plain")))
        );
        assert!(matches!(
            table.elect_mfn(&instrument("mfn"), &instrument("plain")),
            Err(CapTableError::MfnSourceNotLater { .. })
        ));
        assert!(matches!(
            table.elect_mfn(&instrument("mfn"), &instrument("worse")),
            Err(CapTableError::MfnTermsNotBetter { .. })
        ));
        assert_eq!(
            table.elect_mfn(&instrument("mfn"), &instrument("stock")),
            Err(CapTableError::NotASafe(instrument("stock")))
        );
    }
}
