use thiserror::Error;
use undiluted_domain::CapTableError;
use undiluted_envelope::EnvelopeError;
use undiluted_ledger::LedgerError;
use undiluted_service::ServiceError;

#[derive(Debug, Error)]
pub enum ClientError {
    #[error("the enclave could not be reached: {0}")]
    Transport(String),
    #[error("the enclave returned an unexpected answer: {0}")]
    Protocol(String),
    #[error("the enclave reported an error: {0}")]
    Enclave(String),
    #[error("the anchor registry refused: {0}")]
    Anchor(String),
    #[error("the payment failed: {0}")]
    Payment(String),
    #[error("the reply is not signed by the enclave")]
    UnauthenticReply,
    #[error("the reply answers a different request")]
    MismatchedReply,
    #[error("no randomness available: {0}")]
    Randomness(String),
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
