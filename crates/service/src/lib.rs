//! What the enclave does, independent of the platform it runs on.
//!
//! Storage and signing are ports, so the same code runs inside the enclave and,
//! with in-memory adapters, in tests and in a local demo.

pub mod chain;
pub mod error;
pub mod gateway;
pub mod memory;
pub mod ports;
pub mod protocol;
pub mod service;
pub mod wire;

pub use chain::{ChainError, ChainReader, TokenTransfer, parse_transfers};
pub use error::ServiceError;
pub use gateway::{Gateway, GatewayOutput};
pub use memory::MemoryStore;
pub use ports::{EnclaveSigner, Store, StoreError};
pub use protocol::{
    Actor, Command, CompanyView, HeadAttestation, HolderExit, PayoutInstruction, PayoutPlan,
    ProposalView, Reply, RequestId, Response, SignedRequest, Statement,
};
pub use service::Service;
