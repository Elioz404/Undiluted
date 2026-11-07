//! The client side of the protocol. How a call reaches the enclave is a
//! [`Transport`], so the same flow runs locally and against a real deployment.

pub mod anchors;
pub mod client;
pub mod error;
pub mod local;
pub mod outlayer;
pub mod party;
pub mod transport;
mod wire_codec;

pub use anchors::{AnchorRegistry, LocalAnchors, NearAnchors, NearAnchorsConfig};
pub use client::Client;
pub use error::ClientError;
pub use local::LocalTransport;
pub use outlayer::{OutLayerConfig, OutLayerTransport};
pub use party::Party;
pub use transport::{EnclaveIdentity, Exchange, Transport};
