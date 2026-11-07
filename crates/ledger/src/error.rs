use thiserror::Error;
use undiluted_domain::{CapTableError, HolderId};

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum LedgerError {
    #[error(transparent)]
    CapTable(#[from] CapTableError),
    #[error("expected sequence {expected}, got {actual}")]
    SequenceMismatch { expected: u64, actual: u64 },
    #[error("entry {sequence} does not follow the current head")]
    BrokenChain { sequence: u64 },
    #[error("holder {0} is not registered")]
    UnknownHolder(HolderId),
    #[error("holder {0} is already registered")]
    HolderAlreadyRegistered(HolderId),
    #[error("entry {sequence} is missing the signature of holder {holder}")]
    MissingHolderSignature { sequence: u64, holder: HolderId },
    #[error("entry {sequence} has an invalid signature from holder {holder}")]
    InvalidHolderSignature { sequence: u64, holder: HolderId },
    #[error("entry {sequence} has an invalid company signature")]
    InvalidCompanySignature { sequence: u64 },
    #[error("settlement reference must not be empty")]
    EmptySettlementReference,
    #[error("payout destination must not be empty")]
    EmptyPayoutDestination,
    #[error("holder {0} has not chosen where to be paid")]
    MissingPayoutDestination(HolderId),
    #[error("this company has already approved an exit")]
    AlreadyExited,
    #[error("this payment was already used for another instrument")]
    SettlementReused,
    #[error("treasury account and token must not be empty")]
    EmptyTreasury,
    #[error("the treasury is already set")]
    TreasuryAlreadySet,
    #[error("failed to encode an event")]
    Encoding,
}
