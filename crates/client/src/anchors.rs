//! Where heads are published so a company cannot rewrite its history.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::process::Command;

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use undiluted_domain::CompanyId;
use undiluted_ledger::{Digest, PublicKey, SignatureBytes};
use undiluted_service::HeadAttestation;
use undiluted_service::wire::AnchorOutput;

use crate::error::ClientError;

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
    /// Path to the `near` command-line tool, which holds the signer's key.
    pub near_cli: PathBuf,
    pub contract: String,
    pub signer: String,
    /// The network as `near` knows it, e.g. `testnet`.
    pub network: String,
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
        let output = Command::new(&self.0.near_cli)
            .args(["contract", "call-function", "as-transaction"])
            .arg(&self.0.contract)
            .arg(method)
            .arg("json-args")
            .arg(arguments.to_string())
            .arg("prepaid-gas")
            .arg(&self.0.prepaid_gas)
            .arg("attached-deposit")
            .arg(&self.0.deposit)
            .arg("sign-as")
            .arg(&self.0.signer)
            .arg("network-config")
            .arg(&self.0.network)
            .args(["sign-with-keychain", "send"])
            .output()
            .map_err(|error| ClientError::Transport(error.to_string()))?;
        if output.status.success() {
            return Ok(());
        }
        Err(ClientError::Anchor(
            String::from_utf8_lossy(&output.stderr).trim().to_owned(),
        ))
    }

    fn view(
        &self,
        method: &str,
        arguments: &serde_json::Value,
    ) -> Result<serde_json::Value, ClientError> {
        let request = serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "query",
            "params": {
                "request_type": "call_function",
                "finality": "final",
                "account_id": self.0.contract,
                "method_name": method,
                "args_base64": STANDARD.encode(arguments.to_string()),
            },
        });
        let answer: serde_json::Value = ureq::post(&self.0.rpc_url)
            .send_json(request)
            .map_err(|error| ClientError::Transport(error.to_string()))?
            .into_json()
            .map_err(|error| ClientError::Transport(error.to_string()))?;
        let bytes: Vec<u8> = answer["result"]["result"]
            .as_array()
            .ok_or_else(|| ClientError::Anchor(format!("unexpected answer: {answer}")))?
            .iter()
            .filter_map(|byte| byte.as_u64().and_then(|byte| u8::try_from(byte).ok()))
            .collect();
        serde_json::from_slice(&bytes).map_err(|error| ClientError::Anchor(error.to_string()))
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
        let head = self.view(
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
