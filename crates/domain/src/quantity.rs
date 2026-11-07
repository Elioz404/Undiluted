use borsh::{BorshDeserialize, BorshSerialize};

use crate::error::CapTableError;

const BASIS_POINTS_PER_WHOLE: u16 = 10_000;

macro_rules! quantity {
    ($(#[$meta:meta])* $name:ident) => {
        $(#[$meta])*
        #[derive(
            Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Hash,
            BorshSerialize, BorshDeserialize,
        )]
        pub struct $name(u128);

        impl $name {
            pub const ZERO: Self = Self(0);

            #[must_use]
            pub const fn new(value: u128) -> Self {
                Self(value)
            }

            #[must_use]
            pub const fn value(self) -> u128 {
                self.0
            }

            #[must_use]
            pub const fn is_zero(self) -> bool {
                self.0 == 0
            }

            pub fn checked_add(self, other: Self) -> Result<Self, CapTableError> {
                self.0.checked_add(other.0).map(Self).ok_or(CapTableError::Overflow)
            }

            pub fn checked_sub(self, other: Self) -> Result<Self, CapTableError> {
                self.0.checked_sub(other.0).map(Self).ok_or(CapTableError::Overflow)
            }
        }
    };
}

quantity!(
    /// US dollars in micro units, matching the six decimals of USDC and USDT.
    Money
);
quantity!(
    /// A whole number of shares.
    Shares
);

/// A share of a whole expressed in hundredths of a percent.
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
pub struct BasisPoints(u16);

impl BasisPoints {
    pub const WHOLE: Self = Self(BASIS_POINTS_PER_WHOLE);

    pub fn new(value: u16) -> Result<Self, CapTableError> {
        if value > BASIS_POINTS_PER_WHOLE {
            return Err(CapTableError::InvalidBasisPoints(value));
        }
        Ok(Self(value))
    }

    #[must_use]
    pub const fn value(self) -> u16 {
        self.0
    }
}

/// The reduction a SAFE holder gets on the price paid by new money.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, BorshSerialize, BorshDeserialize,
)]
pub struct DiscountRate(BasisPoints);

impl DiscountRate {
    pub fn new(discount: BasisPoints) -> Result<Self, CapTableError> {
        if discount.value() == 0 || discount == BasisPoints::WHOLE {
            return Err(CapTableError::InvalidDiscount);
        }
        Ok(Self(discount))
    }

    #[must_use]
    pub const fn discount(self) -> BasisPoints {
        self.0
    }

    /// Fraction of the round price the holder pays, as `(numerator, denominator)`.
    #[must_use]
    pub fn price_factor(self) -> (u128, u128) {
        (
            u128::from(BASIS_POINTS_PER_WHOLE - self.0.value()),
            u128::from(BASIS_POINTS_PER_WHOLE),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn basis_points_stop_at_one_hundred_percent() {
        assert!(BasisPoints::new(10_000).is_ok());
        assert_eq!(
            BasisPoints::new(10_001),
            Err(CapTableError::InvalidBasisPoints(10_001))
        );
    }

    #[test]
    fn discount_rejects_the_extremes() {
        let zero = BasisPoints::new(0).unwrap();
        assert_eq!(DiscountRate::new(zero), Err(CapTableError::InvalidDiscount));
        assert_eq!(
            DiscountRate::new(BasisPoints::WHOLE),
            Err(CapTableError::InvalidDiscount)
        );
    }

    #[test]
    fn twenty_percent_discount_pays_eighty_percent() {
        let rate = DiscountRate::new(BasisPoints::new(2_000).unwrap()).unwrap();
        assert_eq!(rate.price_factor(), (8_000, 10_000));
    }

    #[test]
    fn subtraction_cannot_go_negative() {
        assert_eq!(
            Money::new(1).checked_sub(Money::new(2)),
            Err(CapTableError::Overflow)
        );
    }
}
