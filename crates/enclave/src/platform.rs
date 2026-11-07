//! `OutLayer`'s implementations of the service's ports.

use outlayer::storage::sealed;
use outlayer::{env, signing_keys};
use serde_json::json;
use undiluted_ledger::{PublicKey, SignatureBytes};
use undiluted_service::{
    ChainError, ChainReader, EnclaveSigner, Store, StoreError, TokenTransfer, parse_transfers,
};

wit_bindgen::generate!({
    world: "undiluted-enclave",
    path: "wit",
});

const ENCRYPTION_KEY_PATH: &str = "ledger";
const SIGNING_KEY_PATH: &str = "enclave";
/// Asking without waiting answers at once, even for a transaction that does not
/// exist. Waiting is only worth it for one that exists but is not final yet.
const NO_WAIT: &str = "NONE";
const UNTIL_FINAL: &str = "FINAL";

fn failure(error: impl core::fmt::Debug) -> StoreError {
    StoreError(format!("{error:?}"))
}

/// Records encrypted under a key only this project's code can use, so the
/// operator sees neither names nor values.
#[derive(Debug)]
pub struct SealedStore;

impl Store for SealedStore {
    fn get(&self, key: &str) -> Result<Option<Vec<u8>>, StoreError> {
        let record = sealed::get(ENCRYPTION_KEY_PATH, None, key).map_err(failure)?;
        Ok(record.map(|record| record.plaintext().to_vec()))
    }

    fn put(&mut self, key: &str, value: &[u8]) -> Result<(), StoreError> {
        sealed::set(ENCRYPTION_KEY_PATH, None, key, value).map_err(failure)
    }

    fn put_new(&mut self, key: &str, value: &[u8]) -> Result<bool, StoreError> {
        sealed::set_if_absent(ENCRYPTION_KEY_PATH, None, key, value).map_err(failure)
    }

    fn delete(&mut self, key: &str) -> Result<(), StoreError> {
        sealed::delete(ENCRYPTION_KEY_PATH, None, key)
            .map(|_| ())
            .map_err(failure)
    }
}

/// A key derived inside the TEE. The code signs with it but never sees it.
#[derive(Debug)]
pub struct TeeSigner;

impl EnclaveSigner for TeeSigner {
    fn public_key(&self) -> Result<PublicKey, StoreError> {
        let bytes = signing_keys::public_key(SIGNING_KEY_PATH, None).map_err(failure)?;
        let bytes: [u8; 32] = bytes.try_into().map_err(failure)?;
        Ok(PublicKey::from_bytes(bytes))
    }

    fn sign(&self, message: &[u8]) -> Result<SignatureBytes, StoreError> {
        let bytes = signing_keys::sign(SIGNING_KEY_PATH, None, message).map_err(failure)?;
        let bytes: [u8; 64] = bytes.try_into().map_err(failure)?;
        Ok(SignatureBytes::from_bytes(bytes))
    }
}

/// Reads the chain through the worker's RPC proxy. It holds no key, so it can
/// look but never send anything.
#[derive(Debug)]
pub struct NearChain;

impl ChainReader for NearChain {
    fn token_transfers(&self, reference: &str) -> Result<Vec<TokenTransfer>, ChainError> {
        // The sender only routes the query to a shard; any account finds it.
        let sender = env::signer_account_id().unwrap_or_default();
        match lookup(reference, &sender, NO_WAIT) {
            Err(ChainError::NotFinal) => lookup(reference, &sender, UNTIL_FINAL),
            result => result,
        }
    }
}

/// Asks through a raw JSON-RPC call, because the dedicated host call does not
/// pass `wait_until` on and so waits for a transaction that may never exist.
fn lookup(
    reference: &str,
    sender: &str,
    wait_until: &str,
) -> Result<Vec<TokenTransfer>, ChainError> {
    let params = json!({
        "tx_hash": reference,
        "sender_account_id": sender,
        "wait_until": wait_until,
    });
    let (response, error) = near::rpc::api::raw("tx", &params.to_string());
    if !error.is_empty() {
        return Err(ChainError::Unavailable(error));
    }
    parse_transfers(&response)
}
