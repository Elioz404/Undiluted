use thiserror::Error;
use undiluted_domain::CapTableError;
use undiluted_envelope::EnvelopeError;
use undiluted_ledger::LedgerError;
use undiluted_service::ServiceError;

#[derive(Debug, Error)]
pub enum WebError {
    #[error("invalid input: {0}")]
    Input(String),
    #[error("the reply is not signed by the enclave")]
    UnauthenticReply,
    #[error("the reply answers a different request")]
    MismatchedReply,
    #[error("no randomness available")]
    Randomness,
    #[error("data could not be encoded")]
    Encoding,
    #[error(transparent)]
    Envelope(#[from] EnvelopeError),
    #[error(transparent)]
    Service(#[from] ServiceError),
    #[error(transparent)]
    CapTable(#[from] CapTableError),
    #[error(transparent)]
    Ledger(#[from] LedgerError),
}

impl WebError {
    pub(crate) fn input(error: &impl ToString) -> Self {
        Self::Input(error.to_string())
    }
}
