use thiserror::Error;

use crate::ids::InstrumentId;

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum CapTableError {
    #[error("identifier must not be empty")]
    EmptyIdentifier,
    #[error("amount must be greater than zero")]
    ZeroAmount,
    #[error("arithmetic overflow")]
    Overflow,
    #[error("division by zero")]
    DivisionByZero,
    #[error("basis points must be between 0 and 10000, got {0}")]
    InvalidBasisPoints(u16),
    #[error("discount must be above 0% and below 100%")]
    InvalidDiscount,
    #[error("a SAFE needs a valuation cap, a discount or an MFN clause")]
    SafeWithoutTerms,
    #[error("instrument {0} already exists")]
    DuplicateInstrument(InstrumentId),
    #[error("instrument {0} does not exist")]
    UnknownInstrument(InstrumentId),
    #[error("instrument {0} is not a SAFE")]
    NotASafe(InstrumentId),
    #[error("SAFE {0} has no MFN clause")]
    NoMfnClause(InstrumentId),
    #[error("{source_id} was not issued after {target}")]
    MfnSourceNotLater {
        target: InstrumentId,
        source_id: InstrumentId,
    },
    #[error("{source_id} does not improve the terms of {target}")]
    MfnTermsNotBetter {
        target: InstrumentId,
        source_id: InstrumentId,
    },
    #[error("token warrants would exceed 100% of the token allocation")]
    WarrantsOverAllocated,
    #[error("the company has no shares to capitalize")]
    NoShares,
    #[error("converting securities would claim 100% or more of the company")]
    SafesExceedCapitalization,
    #[error("a priced round needs at least one new investment")]
    RoundWithoutInvestment,
    #[error("proceeds exceed preferences and no holder can receive the remainder")]
    NoResidualClaimant,
}
