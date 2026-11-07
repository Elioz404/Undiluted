//! Integer arithmetic that never loses a unit of money or a share.

use core::cmp::Ordering;

use borsh::{BorshDeserialize, BorshSerialize};
use primitive_types::U256;

use crate::error::CapTableError;

const RATIO_SCALE: u128 = 1_000_000_000_000_000_000;

fn product(a: u128, b: u128) -> U256 {
    U256::from(a) * U256::from(b)
}

fn narrow(value: U256) -> Result<u128, CapTableError> {
    if value > U256::from(u128::MAX) {
        return Err(CapTableError::Overflow);
    }
    Ok(value.low_u128())
}

/// Computes `a * b / divisor` rounding down, without intermediate overflow.
pub fn mul_div_floor(a: u128, b: u128, divisor: u128) -> Result<u128, CapTableError> {
    if divisor == 0 {
        return Err(CapTableError::DivisionByZero);
    }
    narrow(product(a, b) / U256::from(divisor))
}

/// Compares `a / b` with `c / d`. Both denominators must be non-zero.
#[must_use]
pub fn compare_fractions(a: u128, b: u128, c: u128, d: u128) -> Ordering {
    product(a, d).cmp(&product(c, b))
}

/// A portion of a whole, stored with eighteen decimals.
#[derive(
    Debug,
    Clone,
    Copy,
    Default,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Hash,
    BorshSerialize,
    BorshDeserialize,
)]
pub struct Ratio(u128);

impl Ratio {
    pub const ZERO: Self = Self(0);
    pub const ONE: Self = Self(RATIO_SCALE);

    pub fn from_fraction(numerator: u128, denominator: u128) -> Result<Self, CapTableError> {
        mul_div_floor(numerator, RATIO_SCALE, denominator).map(Self)
    }

    #[must_use]
    pub const fn scaled(self) -> u128 {
        self.0
    }

    pub fn checked_add(self, other: Self) -> Result<Self, CapTableError> {
        self.0
            .checked_add(other.0)
            .map(Self)
            .ok_or(CapTableError::Overflow)
    }

    /// Applies the ratio to an amount, rounding down.
    pub fn apply(self, amount: u128) -> Result<u128, CapTableError> {
        mul_div_floor(amount, self.0, RATIO_SCALE)
    }

    /// The total that leaves `base` holding the remainder once this ratio is
    /// claimed by others, e.g. 8M shares with 20% claimed gross up to 10M.
    pub fn gross_up(self, base: u128) -> Result<u128, CapTableError> {
        if self >= Self::ONE {
            return Err(CapTableError::SafesExceedCapitalization);
        }
        mul_div_floor(base, RATIO_SCALE, RATIO_SCALE - self.0)
    }
}

/// Splits `total` proportionally to `weights` so the parts add up exactly.
///
/// Units lost to rounding go to the largest remainders; ties favour the
/// earliest weight, so the result depends only on the input order.
pub fn allocate_pro_rata(total: u128, weights: &[u128]) -> Result<Vec<u128>, CapTableError> {
    let weight_sum = weights.iter().try_fold(0u128, |sum, weight| {
        sum.checked_add(*weight).ok_or(CapTableError::Overflow)
    })?;
    if weight_sum == 0 {
        return Err(CapTableError::DivisionByZero);
    }

    let divisor = U256::from(weight_sum);
    let mut parts = Vec::with_capacity(weights.len());
    let mut remainders = Vec::with_capacity(weights.len());
    for (index, weight) in weights.iter().enumerate() {
        let numerator = product(total, *weight);
        parts.push(narrow(numerator / divisor)?);
        remainders.push((numerator % divisor, index));
    }

    let assigned = parts.iter().sum::<u128>();
    let leftover = usize::try_from(total - assigned).map_err(|_| CapTableError::Overflow)?;
    remainders.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
    for (_, index) in remainders.into_iter().take(leftover) {
        parts[index] += 1;
    }
    Ok(parts)
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;

    #[test]
    fn mul_div_survives_values_above_u128() {
        assert_eq!(mul_div_floor(u128::MAX, 4, 8), Ok(u128::MAX / 2));
    }

    #[test]
    fn mul_div_reports_results_too_large() {
        assert_eq!(mul_div_floor(u128::MAX, 2, 1), Err(CapTableError::Overflow));
    }

    #[test]
    fn mul_div_rejects_zero_divisor() {
        assert_eq!(mul_div_floor(1, 1, 0), Err(CapTableError::DivisionByZero));
    }

    #[test]
    fn fractions_compare_without_rounding() {
        assert_eq!(compare_fractions(1, 3, 2, 6), Ordering::Equal);
        assert_eq!(compare_fractions(1, 3, 1, 2), Ordering::Less);
        assert_eq!(compare_fractions(u128::MAX, 1, 1, 1), Ordering::Greater);
    }

    #[test]
    fn gross_up_restores_the_full_capitalization() {
        let claimed = Ratio::from_fraction(1, 5).unwrap();
        assert_eq!(claimed.gross_up(8_000_000), Ok(10_000_000));
    }

    #[test]
    fn gross_up_rejects_a_fully_claimed_company() {
        assert_eq!(
            Ratio::ONE.gross_up(1),
            Err(CapTableError::SafesExceedCapitalization)
        );
    }

    #[test]
    fn leftovers_go_to_the_largest_remainders() {
        assert_eq!(allocate_pro_rata(10, &[1, 1, 1]), Ok(vec![4, 3, 3]));
        assert_eq!(allocate_pro_rata(100, &[1, 2, 2]), Ok(vec![20, 40, 40]));
    }

    #[test]
    fn allocation_needs_a_positive_weight() {
        assert_eq!(
            allocate_pro_rata(5, &[0, 0]),
            Err(CapTableError::DivisionByZero)
        );
    }

    proptest! {
        #[test]
        fn allocation_is_always_exact(
            total in 0u128..1_000_000_000_000_000,
            weights in prop::collection::vec(1u128..1_000_000_000_000, 1..30),
        ) {
            let parts = allocate_pro_rata(total, &weights).unwrap();
            prop_assert_eq!(parts.iter().sum::<u128>(), total);
        }
    }
}
