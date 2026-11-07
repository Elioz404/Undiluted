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
    #[error("you can only review and sign your invitation until it is recorded")]
    NotYetRegistered,
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
    #[error("no treasury has been set, so the payment cannot be checked")]
    NoTreasury,
    #[error("this payment was already used for another instrument")]
    SettlementReused,
    #[error("the payment could not be confirmed on chain: {0}")]
    SettlementUnconfirmed(String),
    #[error("the payment does not match: the treasury did not receive this amount of this token")]
    SettlementMismatch,
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
