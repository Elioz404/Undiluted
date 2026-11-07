//! Who owns what today, as if every capped SAFE had already converted.

use borsh::{BorshDeserialize, BorshSerialize};

use crate::cap_table::{CapTable, Position};
use crate::error::CapTableError;
use crate::ids::{HolderId, InstrumentId};
use crate::math::Ratio;
use crate::quantity::Shares;

#[derive(Debug, Clone, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct OwnershipLine {
    pub instrument: InstrumentId,
    pub holder: HolderId,
    pub ownership: Ownership,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub enum Ownership {
    Determined {
        shares: Shares,
        ratio: Ratio,
    },
    /// A SAFE with no cap has no fixed ownership until a round prices it.
    PendingRound,
}

#[derive(Debug, Clone, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct ProForma {
    pub capitalization: Shares,
    pub option_pool: Shares,
    pub lines: Vec<OwnershipLine>,
}

impl ProForma {
    /// What a single holder may see of the table.
    pub fn lines_of<'a>(&'a self, holder: &'a HolderId) -> impl Iterator<Item = &'a OwnershipLine> {
        self.lines.iter().filter(move |line| &line.holder == holder)
    }

    /// Whether the determined lines and the pool account for the whole
    /// capitalization, allowing one share of rounding per line plus one for
    /// the capitalization itself.
    pub fn reconciles(&self) -> Result<bool, CapTableError> {
        let accounted = self
            .lines
            .iter()
            .try_fold(self.option_pool, |total, line| match line.ownership {
                Ownership::Determined { shares, .. } => total.checked_add(shares),
                Ownership::PendingRound => Ok(total),
            })?;
        let tolerance = u128::try_from(self.lines.len()).map_err(|_| CapTableError::Overflow)? + 1;
        Ok(self.capitalization.value().abs_diff(accounted.value()) <= tolerance)
    }
}

impl CapTable {
    pub fn pro_forma(&self) -> Result<ProForma, CapTableError> {
        let existing = self.issued_shares()?.checked_add(self.option_pool())?;
        if existing.is_zero() {
            return Err(CapTableError::NoShares);
        }

        let capitalization = self.capped_safe_ownership()?.gross_up(existing.value())?;

        let mut lines = Vec::with_capacity(self.holdings().len());
        for holding in self.holdings() {
            let ownership = match holding.position() {
                Position::Common { shares } | Position::Preferred { shares, .. } => {
                    Ownership::Determined {
                        shares: *shares,
                        ratio: Ratio::from_fraction(shares.value(), capitalization)?,
                    }
                }
                Position::Safe(safe) => match safe.ownership_at_cap()? {
                    Some(ratio) => Ownership::Determined {
                        shares: Shares::new(ratio.apply(capitalization)?),
                        ratio,
                    },
                    None => Ownership::PendingRound,
                },
                Position::TokenWarrant { .. } => continue,
            };
            lines.push(OwnershipLine {
                instrument: holding.id().clone(),
                holder: holding.holder().clone(),
                ownership,
            });
        }

        Ok(ProForma {
            capitalization: Shares::new(capitalization),
            option_pool: self.option_pool(),
            lines,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::quantity::Money;
    use crate::safe::{Safe, SafeTerms};

    fn usd(dollars: u128) -> Money {
        Money::new(dollars * 1_000_000)
    }

    fn holder(name: &str) -> HolderId {
        HolderId::new(name).unwrap()
    }

    fn instrument(name: &str) -> InstrumentId {
        InstrumentId::new(name).unwrap()
    }

    fn table() -> CapTable {
        let mut table = CapTable::new();
        table
            .issue_common(instrument("c"), holder("founder"), Shares::new(7_000_000))
            .unwrap();
        table.set_option_pool(Shares::new(1_000_000));
        table
            .issue_safe(
                instrument("a"),
                holder("ana"),
                Safe::new(
                    usd(500_000),
                    SafeTerms::new(Some(usd(2_500_000)), None, false).unwrap(),
                )
                .unwrap(),
            )
            .unwrap();
        table
    }

    #[test]
    fn capped_safe_shows_its_locked_ownership() {
        let table = table();
        let view = table.pro_forma().unwrap();

        assert_eq!(view.capitalization, Shares::new(10_000_000));
        let ana = holder("ana");
        let line = view.lines_of(&ana).next().unwrap();
        assert_eq!(
            line.ownership,
            Ownership::Determined {
                shares: Shares::new(2_000_000),
                ratio: Ratio::from_fraction(1, 5).unwrap(),
            }
        );
    }

    #[test]
    fn a_holder_only_sees_their_own_lines() {
        let table = table();
        let view = table.pro_forma().unwrap();
        let ana = holder("ana");
        assert_eq!(view.lines_of(&ana).count(), 1);
    }

    #[test]
    fn determined_lines_reconcile_with_the_capitalization() {
        assert_eq!(table().pro_forma().unwrap().reconciles(), Ok(true));
    }

    #[test]
    fn a_line_changed_behind_the_totals_stops_reconciling() {
        let mut view = table().pro_forma().unwrap();
        view.capitalization = Shares::new(view.capitalization.value() + 100);
        assert_eq!(view.reconciles(), Ok(false));
    }

    #[test]
    fn uncapped_safe_stays_pending_until_a_round() {
        let mut table = table();
        table
            .issue_safe(
                instrument("mfn"),
                holder("dan"),
                Safe::new(usd(1), SafeTerms::new(None, None, true).unwrap()).unwrap(),
            )
            .unwrap();
        let view = table.pro_forma().unwrap();
        let dan = holder("dan");
        assert_eq!(
            view.lines_of(&dan).next().unwrap().ownership,
            Ownership::PendingRound
        );
    }
}
