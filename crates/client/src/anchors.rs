//! Where heads are published so a company cannot rewrite its history.

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use std::collections::BTreeMap;
use undiluted_domain::CompanyId;
use undiluted_ledger::{Digest, PublicKey, SignatureBytes};
use undiluted_service::HeadAttestation;
use undiluted_service::wire::AnchorOutput;

use crate::error::ClientError;
use crate::near_cli::{ContractCall, NearCli, view};

pub trait AnchorRegistry {
    /// Ties a company to the enclave whose signature can extend its history.
    fn register(&mut self, company: &CompanyId, enclave_key: &PublicKey)
    -> Result<(), ClientError>;

    /// Publishes a head. A registry refuses anything the enclave did not sign.
    fn publish(&mut self, anchor: &AnchorOutput) -> Result<(), ClientError>;

    /// The head recorded for `sequence`, as lowercase hex.
    fn head_at(
        &mut self,
        company: &CompanyId,
        sequence: u64,
    ) -> Result<Option<String>, ClientError>;
}

fn decode_fixed<const N: usize>(value: &str) -> Result<[u8; N], ClientError> {
    hex::decode(value)
        .ok()
        .and_then(|bytes| <[u8; N]>::try_from(bytes).ok())
        .ok_or_else(|| ClientError::Protocol("invalid hex in anchor".to_owned()))
}

/// Follows the contract's rules in memory, for the local demo and tests.
#[derive(Debug, Default)]
pub struct LocalAnchors {
    keys: BTreeMap<CompanyId, PublicKey>,
    heads: BTreeMap<CompanyId, Vec<String>>,
}

impl AnchorRegistry for LocalAnchors {
    fn register(
        &mut self,
        company: &CompanyId,
        enclave_key: &PublicKey,
    ) -> Result<(), ClientError> {
        if self.keys.contains_key(company) {
            return Err(ClientError::Anchor(
                "company is already registered".to_owned(),
            ));
        }
        self.keys.insert(company.clone(), *enclave_key);
        Ok(())
    }

    fn publish(&mut self, anchor: &AnchorOutput) -> Result<(), ClientError> {
        let company = CompanyId::new(anchor.company.clone())?;
        let key = self
            .keys
            .get(&company)
            .ok_or_else(|| ClientError::Anchor("unknown company".to_owned()))?;
        let heads = self.heads.entry(company.clone()).or_default();
        if anchor.sequence != heads.len() as u64 {
            return Err(ClientError::Anchor("unexpected sequence".to_owned()));
        }

        let head = Digest::from_bytes(decode_fixed(&anchor.head)?);
        let signature = SignatureBytes::from_bytes(decode_fixed(&anchor.signature)?);
        let digest = HeadAttestation::signing_digest(&company, anchor.sequence, &head)?;
        if !key.verifies(&digest, &signature) {
            return Err(ClientError::Anchor("invalid enclave signature".to_owned()));
        }
        heads.push(anchor.head.clone());
        Ok(())
    }

    fn head_at(
        &mut self,
        company: &CompanyId,
        sequence: u64,
    ) -> Result<Option<String>, ClientError> {
        let index = usize::try_from(sequence).map_err(|_| ClientError::Encoding)?;
        Ok(self
            .heads
            .get(company)
            .and_then(|heads| heads.get(index))
            .cloned())
    }
}

#[derive(Debug, Clone)]
pub struct NearAnchorsConfig {
    /// Signs and pays for publishing.
    pub near: NearCli,
    pub contract: String,
    pub rpc_url: String,
    /// NEAR to attach for storage. Whatever is not used comes back.
    pub deposit: String,
    pub prepaid_gas: String,
}

/// Publishes through the `near` tool and reads directly from an RPC node.
#[derive(Debug)]
pub struct NearAnchors(NearAnchorsConfig);

impl NearAnchors {
    #[must_use]
    pub const fn new(config: NearAnchorsConfig) -> Self {
        Self(config)
    }

    fn call(&self, method: &str, arguments: &serde_json::Value) -> Result<(), ClientError> {
        self.0
            .near
            .call(&ContractCall {
                contract: &self.0.contract,
                method,
                arguments,
                gas: &self.0.prepaid_gas,
                deposit: &self.0.deposit,
            })
            .map(|_| ())
            .map_err(ClientError::Anchor)
    }
}

impl AnchorRegistry for NearAnchors {
    fn register(
        &mut self,
        company: &CompanyId,
        enclave_key: &PublicKey,
    ) -> Result<(), ClientError> {
        self.call(
            "register_company",
            &serde_json::json!({
                "company_id": company.as_str(),
                "enclave_key": format!("ed25519:{}", bs58::encode(enclave_key.as_bytes()).into_string()),
            }),
        )
    }

    fn publish(&mut self, anchor: &AnchorOutput) -> Result<(), ClientError> {
        let head: [u8; 32] = decode_fixed(&anchor.head)?;
        let signature: [u8; 64] = decode_fixed(&anchor.signature)?;
        self.call(
            "anchor",
            &serde_json::json!({
                "company_id": anchor.company,
                "sequence": anchor.sequence,
                "head": bs58::encode(head).into_string(),
                "signature": STANDARD.encode(signature),
            }),
        )
    }

    fn head_at(
        &mut self,
        company: &CompanyId,
        sequence: u64,
    ) -> Result<Option<String>, ClientError> {
        let head = view(
            &self.0.rpc_url,
            &self.0.contract,
            "head_at",
            &serde_json::json!({ "company_id": company.as_str(), "sequence": sequence }),
        )?;
        let Some(encoded) = head.as_str() else {
            return Ok(None);
        };
        let bytes = bs58::decode(encoded)
            .into_vec()
            .map_err(|error| ClientError::Anchor(error.to_string()))?;
        Ok(Some(hex::encode(bytes)))
    }
}

#[cfg(test)]
mod tests {
    use ed25519_dalek::{Signer, SigningKey};

    use super::*;

    const COMPANY: &str = "acme";

    fn key() -> SigningKey {
        SigningKey::from_bytes(&[4; 32])
    }

    fn company() -> CompanyId {
        CompanyId::new(COMPANY).unwrap()
    }

    fn anchor(signer: &SigningKey, sequence: u64, byte: u8) -> AnchorOutput {
        let head = Digest::from_bytes([byte; 32]);
        let digest = HeadAttestation::signing_digest(&company(), sequence, &head).unwrap();
        AnchorOutput {
            company: COMPANY.to_owned(),
            sequence,
            head: hex::encode(head.as_bytes()),
            signature: hex::encode(signer.sign(digest.as_bytes()).to_bytes()),
        }
    }

    fn registry() -> LocalAnchors {
        let mut registry = LocalAnchors::default();
        let public = PublicKey::from_bytes(key().verifying_key().to_bytes());
        registry.register(&company(), &public).unwrap();
        registry
    }

    #[test]
    fn signed_heads_can_be_read_back() {
        let mut registry = registry();
        registry.publish(&anchor(&key(), 0, 1)).unwrap();
        assert_eq!(
            registry.head_at(&company(), 0).unwrap(),
            Some(hex::encode([1u8; 32]))
        );
        assert_eq!(registry.head_at(&company(), 1).unwrap(), None);
    }

    #[test]
    fn a_head_signed_by_anyone_else_is_refused() {
        let forged = anchor(&SigningKey::from_bytes(&[5; 32]), 0, 1);
        assert!(matches!(
            registry().publish(&forged),
            Err(ClientError::Anchor(_))
        ));
    }

    #[test]
    fn a_recorded_head_cannot_be_replaced() {
        let mut registry = registry();
        registry.publish(&anchor(&key(), 0, 1)).unwrap();
        assert!(matches!(
            registry.publish(&anchor(&key(), 0, 2)),
            Err(ClientError::Anchor(_))
        ));
    }
}
