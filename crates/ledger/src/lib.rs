//! The tamper-evident history of a cap table.
//!
//! Current state is never stored on its own: it is the result of replaying
//! signed events. Every entry commits to the one before it, so publishing a
//! single digest (the head) commits to the entire history without revealing
//! any of it. A holder who kept the head from the moment they signed can later
//! tell whether anything before that point was rewritten.

pub mod crypto;
pub mod entry;
pub mod error;
pub mod event;
pub mod ledger;

pub use crypto::{Digest, PublicKey, SignatureBytes};
pub use entry::{Draft, Entry, HolderSignature};
pub use error::LedgerError;
pub use event::{Event, SettlementReference};
pub use ledger::{Ledger, LedgerConfig};
