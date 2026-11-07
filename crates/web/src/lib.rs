//! The client protocol for browsers.
//!
//! Keys, signatures and encryption stay in this code, which is the same that
//! the tests and the command-line client run. JavaScript only moves JSON and
//! base64 around, so it never handles a key in a way that could go wrong.

mod dto;
mod error;
mod protocol;

use wasm_bindgen::prelude::wasm_bindgen;

pub use dto::{ActorDto, CommandDto, EventDto, ResponseDto};
pub use error::WebError;
pub use protocol::{
    PreparedRequest, open_reply, party_public_key, prepare_request, subscription_memo,
};

/// A new signing key for a company or a holder, as hex. Keep it secret.
#[wasm_bindgen(js_name = newParty)]
pub fn new_party() -> Result<String, String> {
    protocol::new_party().map_err(|error| error.to_string())
}

/// The public key of a party, as hex.
#[wasm_bindgen(js_name = partyPublicKey)]
pub fn party_public_key_js(secret: &str) -> Result<String, String> {
    party_public_key(secret).map_err(|error| error.to_string())
}

/// Signs and encrypts a request. Returns JSON with the envelope to send and
/// what is needed to read the reply.
#[wasm_bindgen(js_name = prepareRequest)]
pub fn prepare_request_js(
    identity: &str,
    company: &str,
    actor: &str,
    command: &str,
    secret: &str,
) -> Result<String, String> {
    let prepared = prepare_request(identity, company, actor, command, secret)
        .map_err(|error| error.to_string())?;
    serde_json::to_string(&prepared).map_err(|error| error.to_string())
}

/// Decrypts a reply, checks the enclave signed it for this request, and returns
/// the response as JSON.
#[wasm_bindgen(js_name = openReply)]
pub fn open_reply_js(identity: &str, prepared: &str, envelope: &str) -> Result<String, String> {
    let prepared: PreparedRequest =
        serde_json::from_str(prepared).map_err(|error| error.to_string())?;
    let response = open_reply(identity, &prepared, envelope).map_err(|error| error.to_string())?;
    serde_json::to_string(&response).map_err(|error| error.to_string())
}

/// The memo a payment must carry to subscribe to `company`'s open round with
/// the holder key `key` (hex). Without it the enclave refuses the payment.
#[wasm_bindgen(js_name = subscriptionMemo)]
pub fn subscription_memo_js(company: &str, key: &str) -> Result<String, String> {
    subscription_memo(company, key).map_err(|error| error.to_string())
}
