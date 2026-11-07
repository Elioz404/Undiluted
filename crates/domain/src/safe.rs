//! Post-money SAFE terms and the ownership they imply.

use borsh::{BorshDeserialize, BorshSerialize};

use crate::error::CapTableError;
use crate::math::Ratio;
use crate::quantity::{DiscountRate, Money};

#[derive(Debug, Clone, Copy, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct SafeTerms {
    valuation_cap: Option<Money>,
    discount: Option<DiscountRate>,
    most_favored_nation: bool,
}

impl SafeTerms {
    pub fn new(
        valuation_cap: Option<Money>,
        discount: Option<DiscountRate>,
        most_favored_nation: bool,
    ) -> Result<Self, CapTableError> {
        if valuation_cap.is_some_and(Money::is_zero) {
            return Err(CapTableError::ZeroAmount);
        }
        if valuation_cap.is_none() && discount.is_none() && !most_favored_nation {
            return Err(CapTableError::SafeWithoutTerms);
        }
        Ok(Self {
            valuation_cap,
            discount,
            most_favored_nation,
        })
    }

    #[must_use]
    pub const fn valuation_cap(&self) -> Option<Money> {
        self.valuation_cap
    }

    #[must_use]
    pub const fn discount(&self) -> Option<DiscountRate> {
        self.discount
    }

    #[must_use]
    pub const fn has_mfn_clause(&self) -> bool {
        self.most_favored_nation
    }

    /// Whether these terms give a holder a lower cap or a deeper discount.
    #[must_use]
    pub fn improves_on(&self, other: &Self) -> bool {
        let better_cap = match (self.valuation_cap, other.valuation_cap) {
            (Some(mine), Some(theirs)) => mine < theirs,
            (Some(_), None) => true,
            (None, _) => false,
        };
        let better_discount = match (self.discount, other.discount) {
            (Some(mine), Some(theirs)) => mine > theirs,
            (Some(_), None) => true,
            (None, _) => false,
        };
        better_cap || better_discount
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct Safe {
    purchase_amount: Money,
    terms: SafeTerms,
}

impl Safe {
    pub fn new(purchase_amount: Money, terms: SafeTerms) -> Result<Self, CapTableError> {
        if purchase_amount.is_zero() {
            return Err(CapTableError::ZeroAmount);
        }
        Ok(Self {
            purchase_amount,
            terms,
        })
    }

    #[must_use]
    pub const fn purchase_amount(&self) -> Money {
        self.purchase_amount
    }

    #[must_use]
    pub const fn terms(&self) -> &SafeTerms {
        &self.terms
    }

    /// Replaces the terms, as happens when an MFN clause is exercised.
    pub(crate) fn adopt_terms(&mut self, terms: SafeTerms) {
        self.terms = terms;
    }

    /// Ownership locked in by the valuation cap, if there is one.
    pub fn ownership_at_cap(&self) -> Result<Option<Ratio>, CapTableError> {
        self.terms
            .valuation_cap
            .map(|cap| Ratio::from_fraction(self.purchase_amount.value(), cap.value()))
            .transpose()
    }

    /// Ownership of the pre-round capitalization once converted in a priced
    /// round. The holder gets whichever of round price, cap or discount yields
    /// the most shares.
    pub fn ownership_in_round(&self, pre_money: Money) -> Result<Ratio, CapTableError> {
        let purchase = self.purchase_amount.value();
        let mut best = Ratio::from_fraction(purchase, pre_money.value())?;

        if let Some(at_cap) = self.ownership_at_cap()? {
            best = best.max(at_cap);
        }
        if let Some(discount) = self.terms.discount {
            let (paid, whole) = discount.price_factor();
            let numerator = purchase.checked_mul(whole).ok_or(CapTableError::Overflow)?;
            let denominator = pre_money
                .value()
                .checked_mul(paid)
                .ok_or(CapTableError::Overflow)?;
            best = best.max(Ratio::from_fraction(numerator, denominator)?);
        }
        Ok(best)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::quantity::BasisPoints;

    fn usd(dollars: u128) -> Money {
        Money::new(dollars * 1_000_000)
    }

    fn discount(basis_points: u16) -> DiscountRate {
        DiscountRate::new(BasisPoints::new(basis_points).unwrap()).unwrap()
    }

    fn capped(cap: u128) -> SafeTerms {
        SafeTerms::new(Some(usd(cap)), None, false).unwrap()
    }

    #[test]
    fn terms_must_grant_something() {
        assert_eq!(
            SafeTerms::new(None, None, false),
            Err(CapTableError::SafeWithoutTerms)
        );
    }

    #[test]
    fn valuation_cap_cannot_be_zero() {
        assert_eq!(
            SafeTerms::new(Some(Money::ZERO), None, false),
            Err(CapTableError::ZeroAmount)
        );
    }

    #[test]
    fn cap_fixes_ownership_as_purchase_over_post_money() {
        let safe = Safe::new(usd(500_000), capped(5_000_000)).unwrap();
        assert_eq!(
            safe.ownership_at_cap().unwrap(),
            Some(Ratio::from_fraction(1, 10).unwrap())
        );
    }

    #[test]
    fn cap_wins_when_the_round_prices_above_it() {
        let safe = Safe::new(usd(500_000), capped(5_000_000)).unwrap();
        assert_eq!(
            safe.ownership_in_round(usd(20_000_000)).unwrap(),
            Ratio::from_fraction(1, 10).unwrap()
        );
    }

    #[test]
    fn round_price_wins_when_it_is_below_the_cap() {
        let safe = Safe::new(usd(1_000_000), capped(20_000_000)).unwrap();
        assert_eq!(
            safe.ownership_in_round(usd(10_000_000)).unwrap(),
            Ratio::from_fraction(1, 10).unwrap()
        );
    }

    #[test]
    fn discount_wins_when_it_yields_more_shares() {
        let terms = SafeTerms::new(Some(usd(20_000_000)), Some(discount(2_000)), false).unwrap();
        let safe = Safe::new(usd(1_000_000), terms).unwrap();
        assert_eq!(
            safe.ownership_in_round(usd(10_000_000)).unwrap(),
            Ratio::from_fraction(1, 8).unwrap()
        );
    }

    #[test]
    fn lower_cap_or_deeper_discount_counts_as_better() {
        let base = SafeTerms::new(Some(usd(10_000_000)), Some(discount(1_000)), true).unwrap();
        assert!(capped(5_000_000).improves_on(&base));
        assert!(!capped(10_000_000).improves_on(&base));

        let deeper = SafeTerms::new(None, Some(discount(2_000)), false).unwrap();
        assert!(deeper.improves_on(&base));

        let uncapped_mfn = SafeTerms::new(None, None, true).unwrap();
        assert!(capped(50_000_000).improves_on(&uncapped_mfn));
        assert!(!uncapped_mfn.improves_on(&base));
    }
}
