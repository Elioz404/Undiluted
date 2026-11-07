//! Splits exit proceeds between holders.
//!
//! Preferred stock and SAFEs rank equally and are non-participating: each
//! holder takes its money back or converts and shares with common, whichever
//! pays more. Holders with the lowest preference per share convert first,
//! because conversion only gets more attractive as others join common.
//!
//! Unissued options and token warrants receive nothing. A SAFE with no
//! valuation cap can only take its money back.

use crate::cap_table::{CapTable, Holding, Position};
use crate::error::CapTableError;
use crate::ids::{HolderId, InstrumentId};
use crate::math::{allocate_pro_rata, compare_fractions};
use crate::quantity::Money;
use core::cmp::Ordering;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PayoutBasis {
    Preference,
    AsConverted,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Payout {
    pub instrument: InstrumentId,
    pub holder: HolderId,
    pub amount: Money,
    pub basis: PayoutBasis,
}

#[derive(Debug, Clone, Copy)]
struct Claim<'a> {
    holding: &'a Holding,
    preference: u128,
    shares: u128,
}

impl CapTable {
    /// Computes every payout for an exit. Payouts always add up to `proceeds`.
    pub fn distribute(&self, proceeds: Money) -> Result<Vec<Payout>, CapTableError> {
        if proceeds.is_zero() {
            return Err(CapTableError::ZeroAmount);
        }
        let claims = self.exit_claims()?;
        let total_preference = claims.iter().try_fold(0u128, |total, claim| {
            total
                .checked_add(claim.preference)
                .ok_or(CapTableError::Overflow)
        })?;

        if proceeds.value() <= total_preference {
            return preferences_only(&claims, proceeds.value());
        }
        with_conversions(&claims, proceeds.value() - total_preference)
    }

    fn exit_claims(&self) -> Result<Vec<Claim<'_>>, CapTableError> {
        let capitalization = self.liquidity_capitalization()?;
        let mut claims = Vec::new();
        for holding in self.holdings() {
            let (preference, shares) = match holding.position() {
                Position::Common { shares } => (0, shares.value()),
                Position::Preferred {
                    shares,
                    liquidation_preference,
                } => (liquidation_preference.value(), shares.value()),
                Position::Safe(safe) => {
                    let shares = safe
                        .ownership_at_cap()?
                        .map_or(Ok(0), |ownership| ownership.apply(capitalization))?;
                    (safe.purchase_amount().value(), shares)
                }
                Position::TokenWarrant { .. } => continue,
            };
            claims.push(Claim {
                holding,
                preference,
                shares,
            });
        }
        Ok(claims)
    }

    /// Issued shares grossed up by the ownership locked in by capped SAFEs.
    fn liquidity_capitalization(&self) -> Result<u128, CapTableError> {
        self.capped_safe_ownership()?
            .gross_up(self.issued_shares()?.value())
    }
}

fn preferences_only(claims: &[Claim<'_>], proceeds: u128) -> Result<Vec<Payout>, CapTableError> {
    let weights: Vec<u128> = claims.iter().map(|claim| claim.preference).collect();
    let amounts = allocate_pro_rata(proceeds, &weights)?;
    Ok(claims
        .iter()
        .zip(amounts)
        .map(|(claim, amount)| payout(claim, amount, PayoutBasis::Preference))
        .collect())
}

fn with_conversions(claims: &[Claim<'_>], surplus: u128) -> Result<Vec<Payout>, CapTableError> {
    let converts = conversion_decisions(claims, surplus)?;

    let mut residual = surplus;
    let mut weights = Vec::with_capacity(claims.len());
    for (claim, converted) in claims.iter().zip(&converts) {
        if *converted {
            residual = residual
                .checked_add(claim.preference)
                .ok_or(CapTableError::Overflow)?;
            weights.push(claim.shares);
        } else {
            weights.push(0);
        }
    }
    if weights.iter().all(|weight| *weight == 0) {
        return Err(CapTableError::NoResidualClaimant);
    }
    let shares_of_residual = allocate_pro_rata(residual, &weights)?;

    Ok(claims
        .iter()
        .zip(&converts)
        .zip(shares_of_residual)
        .map(|((claim, converted), share)| {
            if *converted {
                payout(claim, share, PayoutBasis::AsConverted)
            } else {
                payout(claim, claim.preference, PayoutBasis::Preference)
            }
        })
        .collect())
}

/// Decides, per claim, whether it shares in the residual rather than taking
/// its preference. Common always shares; anything with no shares never can.
fn conversion_decisions(claims: &[Claim<'_>], surplus: u128) -> Result<Vec<bool>, CapTableError> {
    let mut converts: Vec<bool> = claims
        .iter()
        .map(|claim| claim.preference == 0 && claim.shares > 0)
        .collect();
    let mut residual = surplus;
    let mut shares = claims
        .iter()
        .zip(&converts)
        .filter(|(_, converted)| **converted)
        .try_fold(0u128, |total, (claim, _)| {
            total
                .checked_add(claim.shares)
                .ok_or(CapTableError::Overflow)
        })?;

    let mut candidates: Vec<usize> = (0..claims.len())
        .filter(|index| claims[*index].preference > 0 && claims[*index].shares > 0)
        .collect();
    candidates.sort_by(|a, b| {
        compare_fractions(
            claims[*a].preference,
            claims[*a].shares,
            claims[*b].preference,
            claims[*b].shares,
        )
    });

    for index in candidates {
        let claim = &claims[index];
        let next_residual = residual
            .checked_add(claim.preference)
            .ok_or(CapTableError::Overflow)?;
        let next_shares = shares
            .checked_add(claim.shares)
            .ok_or(CapTableError::Overflow)?;
        let per_share_if_converted =
            compare_fractions(next_residual, next_shares, claim.preference, claim.shares);
        if per_share_if_converted == Ordering::Less {
            break;
        }
        converts[index] = true;
        residual = next_residual;
        shares = next_shares;
    }
    Ok(converts)
}

fn payout(claim: &Claim<'_>, amount: u128, basis: PayoutBasis) -> Payout {
    Payout {
        instrument: claim.holding.id().clone(),
        holder: claim.holding.holder().clone(),
        amount: Money::new(amount),
        basis,
    }
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;
    use crate::conversion::{NewInvestment, PricedRound};
    use crate::quantity::{BasisPoints, Shares};
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

    /// 8M founder shares plus SAFEs of $500k and $1M, each locked at 10%.
    /// The liquidity capitalization is 10M, so each SAFE is worth 1M shares.
    fn seed_stage_table() -> CapTable {
        let mut table = CapTable::new();
        table
            .issue_common(
                instrument("founders"),
                holder("founder"),
                Shares::new(8_000_000),
            )
            .unwrap();
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
        table
    }

    fn paid(payouts: &[Payout], id: &str) -> (u128, PayoutBasis) {
        let payout = payouts
            .iter()
            .find(|payout| payout.instrument.as_str() == id)
            .unwrap();
        (payout.amount.value(), payout.basis)
    }

    #[test]
    fn large_exit_converts_everyone() {
        let payouts = seed_stage_table().distribute(usd(50_000_000)).unwrap();

        assert_eq!(
            paid(&payouts, "founders"),
            (40_000_000_000_000, PayoutBasis::AsConverted)
        );
        assert_eq!(
            paid(&payouts, "a"),
            (5_000_000_000_000, PayoutBasis::AsConverted)
        );
        assert_eq!(
            paid(&payouts, "b"),
            (5_000_000_000_000, PayoutBasis::AsConverted)
        );
    }

    /// At $6M, A converts: $5M over 9M shares is $0.56 against its $0.50
    /// preference per share. B would get $0.60 against $1.00, so it stays out.
    #[test]
    fn middle_exit_converts_only_the_holder_who_gains() {
        let payouts = seed_stage_table().distribute(usd(6_000_000)).unwrap();

        assert_eq!(
            paid(&payouts, "b"),
            (1_000_000_000_000, PayoutBasis::Preference)
        );
        assert_eq!(
            paid(&payouts, "a"),
            (555_555_555_556, PayoutBasis::AsConverted)
        );
        assert_eq!(
            paid(&payouts, "founders"),
            (4_444_444_444_444, PayoutBasis::AsConverted)
        );
    }

    #[test]
    fn small_exit_returns_preferences_and_leaves_the_rest_to_common() {
        let payouts = seed_stage_table().distribute(usd(2_000_000)).unwrap();

        assert_eq!(
            paid(&payouts, "a"),
            (500_000_000_000, PayoutBasis::Preference)
        );
        assert_eq!(
            paid(&payouts, "b"),
            (1_000_000_000_000, PayoutBasis::Preference)
        );
        assert_eq!(
            paid(&payouts, "founders"),
            (500_000_000_000, PayoutBasis::AsConverted)
        );
    }

    #[test]
    fn exit_below_preferences_is_shared_pro_rata_and_common_gets_nothing() {
        let payouts = seed_stage_table().distribute(usd(1_000_000)).unwrap();

        assert_eq!(
            paid(&payouts, "a"),
            (333_333_333_333, PayoutBasis::Preference)
        );
        assert_eq!(
            paid(&payouts, "b"),
            (666_666_666_667, PayoutBasis::Preference)
        );
        assert_eq!(paid(&payouts, "founders"), (0, PayoutBasis::Preference));
    }

    #[test]
    fn distribution_works_after_a_priced_round() {
        let mut table = seed_stage_table();
        table
            .close_round(&PricedRound {
                pre_money: usd(20_000_000),
                investments: vec![NewInvestment {
                    instrument: instrument("series-a"),
                    holder: holder("lead"),
                    amount: usd(5_000_000),
                }],
            })
            .unwrap();

        let payouts = table.distribute(usd(125_000_000)).unwrap();

        assert_eq!(
            paid(&payouts, "series-a"),
            (25_000_000_000_000, PayoutBasis::AsConverted)
        );
        assert_eq!(
            paid(&payouts, "founders"),
            (80_000_000_000_000, PayoutBasis::AsConverted)
        );
    }

    #[test]
    fn warrants_and_unissued_options_receive_nothing() {
        let mut table = seed_stage_table();
        table.set_option_pool(Shares::new(1_000_000));
        table
            .issue_token_warrant(
                instrument("warrant"),
                holder("ana"),
                BasisPoints::new(500).unwrap(),
            )
            .unwrap();

        let payouts = table.distribute(usd(50_000_000)).unwrap();

        assert_eq!(paid(&payouts, "founders").0, 40_000_000_000_000);
        assert!(payouts.iter().all(|p| p.instrument.as_str() != "warrant"));
    }

    #[test]
    fn uncapped_safe_only_gets_its_money_back() {
        let mut table = seed_stage_table();
        table
            .issue_safe(
                instrument("mfn"),
                holder("dan"),
                Safe::new(usd(100_000), SafeTerms::new(None, None, true).unwrap()).unwrap(),
            )
            .unwrap();

        let payouts = table.distribute(usd(50_000_000)).unwrap();

        assert_eq!(
            paid(&payouts, "mfn"),
            (100_000_000_000, PayoutBasis::Preference)
        );
    }

    #[test]
    fn surplus_with_nobody_to_receive_it_is_an_error() {
        let mut table = CapTable::new();
        table
            .issue_safe(
                instrument("mfn"),
                holder("dan"),
                Safe::new(usd(1), SafeTerms::new(None, None, true).unwrap()).unwrap(),
            )
            .unwrap();

        assert_eq!(
            table.distribute(usd(2)),
            Err(CapTableError::NoResidualClaimant)
        );
    }

    #[test]
    fn zero_proceeds_are_rejected() {
        assert_eq!(
            seed_stage_table().distribute(Money::ZERO),
            Err(CapTableError::ZeroAmount)
        );
    }

    proptest! {
        #[test]
        fn payouts_add_up_and_no_converter_regrets_it(
            proceeds in 1u128..1_000_000_000_000_000,
            founders in 1u128..100_000_000,
            first in 1u128..1_000_000,
            second in 1u128..1_000_000,
        ) {
            let mut table = CapTable::new();
            table.issue_common(instrument("c"), holder("f"), Shares::new(founders)).unwrap();
            table.issue_safe(instrument("a"), holder("a"), capped_safe(first, 10_000_000)).unwrap();
            table.issue_safe(instrument("b"), holder("b"), capped_safe(second, 20_000_000)).unwrap();

            let payouts = table.distribute(Money::new(proceeds)).unwrap();
            let total: u128 = payouts.iter().map(|p| p.amount.value()).sum();
            prop_assert_eq!(total, proceeds);

            let preference = |id: &str| match id {
                "a" => first * 1_000_000,
                _ => second * 1_000_000,
            };
            for payout in &payouts {
                let id = payout.instrument.as_str();
                if id != "c" && payout.basis == PayoutBasis::AsConverted {
                    // Rounding can cost a converter at most one micro dollar.
                    prop_assert!(payout.amount.value() + 1 >= preference(id));
                }
            }
        }
    }
}
