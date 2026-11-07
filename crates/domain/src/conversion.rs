//! Priced rounds: converting SAFEs and issuing preferred stock to new money.
//!
//! Post-money SAFEs claim a fixed share of the pre-round capitalization, which
//! includes the SAFEs themselves. That makes the capitalization a closed form:
//! `existing shares / (1 - sum of SAFE ownership)`.

use crate::cap_table::{CapTable, Holding, Position};
use crate::error::CapTableError;
use crate::ids::{HolderId, InstrumentId};
use crate::math::{Ratio, mul_div_floor};
use crate::quantity::{Money, Shares};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewInvestment {
    pub instrument: InstrumentId,
    pub holder: HolderId,
    pub amount: Money,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PricedRound {
    pub pre_money: Money,
    pub investments: Vec<NewInvestment>,
}

/// The preferred stock one instrument receives in a round.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreferredIssuance {
    pub instrument: InstrumentId,
    pub holder: HolderId,
    pub shares: Shares,
    pub liquidation_preference: Money,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RoundOutcome {
    /// Shares outstanding right before new money, SAFEs already converted.
    pub pre_round_capitalization: Shares,
    pub converted_safes: Vec<PreferredIssuance>,
    pub new_money: Vec<PreferredIssuance>,
}

impl CapTable {
    /// Works out a round without touching the cap table.
    pub fn price_round(&self, round: &PricedRound) -> Result<RoundOutcome, CapTableError> {
        if round.pre_money.is_zero() {
            return Err(CapTableError::ZeroAmount);
        }
        if round.investments.is_empty() {
            return Err(CapTableError::RoundWithoutInvestment);
        }

        let existing = self.issued_shares()?.checked_add(self.option_pool())?;
        if existing.is_zero() {
            return Err(CapTableError::NoShares);
        }

        let claims = self.safe_claims(round.pre_money)?;
        let claimed = claims
            .iter()
            .try_fold(Ratio::ZERO, |total, (_, _, ownership)| {
                total.checked_add(*ownership)
            })?;
        let capitalization = claimed.gross_up(existing.value())?;

        let converted_safes = claims
            .into_iter()
            .map(|(holding, purchase, ownership)| {
                Ok(PreferredIssuance {
                    instrument: holding.id().clone(),
                    holder: holding.holder().clone(),
                    shares: Shares::new(ownership.apply(capitalization)?),
                    liquidation_preference: purchase,
                })
            })
            .collect::<Result<Vec<_>, CapTableError>>()?;

        let new_money = round
            .investments
            .iter()
            .map(|investment| {
                if investment.amount.is_zero() {
                    return Err(CapTableError::ZeroAmount);
                }
                Ok(PreferredIssuance {
                    instrument: investment.instrument.clone(),
                    holder: investment.holder.clone(),
                    shares: Shares::new(mul_div_floor(
                        investment.amount.value(),
                        capitalization,
                        round.pre_money.value(),
                    )?),
                    liquidation_preference: investment.amount,
                })
            })
            .collect::<Result<Vec<_>, CapTableError>>()?;

        Ok(RoundOutcome {
            pre_round_capitalization: Shares::new(capitalization),
            converted_safes,
            new_money,
        })
    }

    /// Closes a round: every SAFE becomes preferred stock under the same id
    /// and new investors are added. Nothing changes if the round is invalid.
    pub fn close_round(&mut self, round: &PricedRound) -> Result<RoundOutcome, CapTableError> {
        let outcome = self.price_round(round)?;

        let mut next = self.clone();
        for issuance in &outcome.new_money {
            next.push(
                issuance.instrument.clone(),
                issuance.holder.clone(),
                preferred_position(issuance),
            )?;
        }
        for issuance in &outcome.converted_safes {
            let holding = next
                .holdings_mut()
                .iter_mut()
                .find(|holding| holding.id() == &issuance.instrument)
                .ok_or_else(|| CapTableError::UnknownInstrument(issuance.instrument.clone()))?;
            holding.replace_position(preferred_position(issuance));
        }

        *self = next;
        Ok(outcome)
    }

    fn safe_claims(
        &self,
        pre_money: Money,
    ) -> Result<Vec<(&Holding, Money, Ratio)>, CapTableError> {
        self.holdings()
            .iter()
            .filter_map(|holding| match holding.position() {
                Position::Safe(safe) => Some(
                    safe.ownership_in_round(pre_money)
                        .map(|ownership| (holding, safe.purchase_amount(), ownership)),
                ),
                _ => None,
            })
            .collect()
    }
}

fn preferred_position(issuance: &PreferredIssuance) -> Position {
    Position::Preferred {
        shares: issuance.shares,
        liquidation_preference: issuance.liquidation_preference,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::quantity::{BasisPoints, DiscountRate};
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

    fn capped_safe(purchase: u128, cap: u128) -> Safe {
        Safe::new(
            usd(purchase),
            SafeTerms::new(Some(usd(cap)), None, false).unwrap(),
        )
        .unwrap()
    }

    fn table_with_founders(shares: u128) -> CapTable {
        let mut table = CapTable::new();
        table
            .issue_common(
                instrument("founders"),
                holder("founder"),
                Shares::new(shares),
            )
            .unwrap();
        table
    }

    fn round(pre_money: u128, amount: u128) -> PricedRound {
        PricedRound {
            pre_money: usd(pre_money),
            investments: vec![NewInvestment {
                instrument: instrument("series-a"),
                holder: holder("lead"),
                amount: usd(amount),
            }],
        }
    }

    /// 8M founder shares, 10% and 10% SAFEs: capitalization is 8M / 0.8 = 10M.
    /// At $20M pre-money the price is $2, so $5M buys 2.5M shares.
    #[test]
    fn capped_safes_convert_at_their_locked_ownership() {
        let mut table = table_with_founders(8_000_000);
        table
            .issue_safe(
                instrument("a"),
                holder("ana"),
                capped_safe(500_000, 5_000_000),
            )
            .unwrap();
        table
            .issue_safe(
                instrument("b"),
                holder("ben"),
                capped_safe(1_000_000, 10_000_000),
            )
            .unwrap();

        let outcome = table.price_round(&round(20_000_000, 5_000_000)).unwrap();

        assert_eq!(outcome.pre_round_capitalization, Shares::new(10_000_000));
        assert_eq!(outcome.converted_safes[0].shares, Shares::new(1_000_000));
        assert_eq!(outcome.converted_safes[1].shares, Shares::new(1_000_000));
        assert_eq!(
            outcome.converted_safes[0].liquidation_preference,
            usd(500_000)
        );
        assert_eq!(outcome.new_money[0].shares, Shares::new(2_500_000));
    }

    /// $1M at a 20% discount on a $10M pre-money round claims 12.5%. With 7M
    /// existing shares the capitalization is 8M, the round price $1.25 and the
    /// discount price $1.00, which buys exactly 1M shares.
    #[test]
    fn discount_safe_converts_at_the_discounted_price() {
        let mut table = table_with_founders(7_000_000);
        let rate = DiscountRate::new(BasisPoints::new(2_000).unwrap()).unwrap();
        let terms = SafeTerms::new(None, Some(rate), false).unwrap();
        table
            .issue_safe(
                instrument("d"),
                holder("dan"),
                Safe::new(usd(1_000_000), terms).unwrap(),
            )
            .unwrap();

        let outcome = table.price_round(&round(10_000_000, 1_000_000)).unwrap();

        assert_eq!(outcome.pre_round_capitalization, Shares::new(8_000_000));
        assert_eq!(outcome.converted_safes[0].shares, Shares::new(1_000_000));
        assert_eq!(outcome.new_money[0].shares, Shares::new(800_000));
    }

    #[test]
    fn option_pool_counts_towards_the_capitalization() {
        let mut table = table_with_founders(7_000_000);
        table.set_option_pool(Shares::new(1_000_000));
        table
            .issue_safe(
                instrument("a"),
                holder("ana"),
                capped_safe(500_000, 2_500_000),
            )
            .unwrap();

        let outcome = table.price_round(&round(20_000_000, 1_000_000)).unwrap();

        assert_eq!(outcome.pre_round_capitalization, Shares::new(10_000_000));
        assert_eq!(outcome.converted_safes[0].shares, Shares::new(2_000_000));
    }

    #[test]
    fn closing_replaces_safes_and_adds_new_money() {
        let mut table = table_with_founders(8_000_000);
        table
            .issue_safe(
                instrument("a"),
                holder("ana"),
                capped_safe(2_000_000, 10_000_000),
            )
            .unwrap();

        table.close_round(&round(20_000_000, 5_000_000)).unwrap();

        assert_eq!(
            table.holding(&instrument("a")).unwrap().position(),
            &Position::Preferred {
                shares: Shares::new(2_000_000),
                liquidation_preference: usd(2_000_000),
            }
        );
        assert_eq!(table.issued_shares(), Ok(Shares::new(12_500_000)));
    }

    #[test]
    fn failed_close_leaves_the_table_untouched() {
        let mut table = table_with_founders(8_000_000);
        table
            .issue_safe(
                instrument("a"),
                holder("ana"),
                capped_safe(2_000_000, 10_000_000),
            )
            .unwrap();
        let before = table.clone();

        let duplicate = PricedRound {
            pre_money: usd(20_000_000),
            investments: vec![NewInvestment {
                instrument: instrument("founders"),
                holder: holder("lead"),
                amount: usd(1),
            }],
        };

        assert_eq!(
            table.close_round(&duplicate),
            Err(CapTableError::DuplicateInstrument(instrument("founders")))
        );
        assert_eq!(table, before);
    }

    #[test]
    fn safes_cannot_claim_the_whole_company() {
        let mut table = table_with_founders(1_000);
        table
            .issue_safe(instrument("a"), holder("ana"), capped_safe(600, 1_000))
            .unwrap();
        table
            .issue_safe(instrument("b"), holder("ben"), capped_safe(400, 1_000))
            .unwrap();

        assert_eq!(
            table.price_round(&round(20_000_000, 1)),
            Err(CapTableError::SafesExceedCapitalization)
        );
    }

    #[test]
    fn round_needs_shares_and_investment() {
        assert_eq!(
            CapTable::new().price_round(&round(1, 1)),
            Err(CapTableError::NoShares)
        );
        let no_money = PricedRound {
            pre_money: usd(1),
            investments: vec![],
        };
        assert_eq!(
            table_with_founders(1).price_round(&no_money),
            Err(CapTableError::RoundWithoutInvestment)
        );
    }
}
