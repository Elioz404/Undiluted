//! Cap table rules with no I/O, clock or randomness.
//!
//! All arithmetic is integer based so every environment that replays the same
//! events (enclave, investor browser, tests) reaches exactly the same result.

pub mod cap_table;
pub mod conversion;
pub mod error;
pub mod ids;
pub mod math;
pub mod pro_forma;
pub mod quantity;
pub mod safe;
pub mod waterfall;

pub use cap_table::{CapTable, Holding, Position};
pub use conversion::{NewInvestment, PreferredIssuance, PricedRound, RoundOutcome};
pub use error::CapTableError;
pub use ids::{HolderId, InstrumentId};
pub use math::Ratio;
pub use pro_forma::{Ownership, OwnershipLine, ProForma};
pub use quantity::{BasisPoints, DiscountRate, Money, Shares};
pub use safe::{Safe, SafeTerms};
pub use waterfall::{Payout, PayoutBasis};
