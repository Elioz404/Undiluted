use thiserror::Error;
use undiluted_domain::CapTableError;
use undiluted_ledger::LedgerError;

use crate::ports::StoreError;

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ServiceError {
    #[error(transparent)]
    Store(#[from] StoreError),
    #[error(transparent)]
    Ledger(#[from] LedgerError),
    #[error(transparent)]
    CapTable(#[from] CapTableError),
    #[error("request signature is invalid")]
    InvalidSignature,
    #[error("this company already exists")]
    CompanyExists,
    #[error("this company does not exist")]
    UnknownCompany,
    #[error("this actor may not do that")]
    Forbidden,
    #[error("only a holder can request a statement")]
    StatementNeedsHolder,
    #[error("another proposal is waiting for signatures")]
    ProposalPending,
    #[error("there is no pending proposal")]
    NoProposal,
    #[error("this signature is not for the pending proposal")]
    WrongProposal,
    #[error("another run changed this company; retry")]
    Conflict,
    #[error("no exit has been approved")]
    NoExit,
    #[error("a payout has no destination")]
    NoDestination,
    #[error("too many payouts")]
    TooManyPayouts,
    #[error("stored data could not be read")]
    CorruptData,
    #[error("data could not be encoded")]
    Encoding,
}
