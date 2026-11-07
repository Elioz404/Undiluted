//! The browser protocol against the real service, in memory.

use ed25519_dalek::{Signer, SigningKey};
use serde_json::{Value, json};
use undiluted_domain::Money;
use undiluted_envelope::EnclaveSecret;
use undiluted_ledger::{PublicKey, SignatureBytes};
use undiluted_service::wire::{Input, Output, run};
use undiluted_service::{
    ChainError, ChainReader, EnclaveSigner, Gateway, MemoryStore, Service, StoreError,
    TokenTransfer,
};
use undiluted_web::{ResponseDto, open_reply, party_public_key, prepare_request};

const COMPANY: &str = "acme";
const TREASURY: &str = "treasury.near";
const TOKEN: &str = "usdc.near";

#[derive(Debug)]
struct Signer42(SigningKey);

impl EnclaveSigner for Signer42 {
    fn public_key(&self) -> Result<PublicKey, StoreError> {
        Ok(PublicKey::from_bytes(self.0.verifying_key().to_bytes()))
    }

    fn sign(&self, message: &[u8]) -> Result<SignatureBytes, StoreError> {
        Ok(SignatureBytes::from_bytes(self.0.sign(message).to_bytes()))
    }
}

/// Every reference pays its trailing number of micro dollars to the treasury.
#[derive(Debug)]
struct Chain;

impl ChainReader for Chain {
    fn token_transfers(&self, reference: &str) -> Result<Vec<TokenTransfer>, ChainError> {
        let amount = reference
            .rsplit(':')
            .next()
            .and_then(|amount| amount.parse().ok())
            .ok_or_else(|| ChainError::Unavailable("no such transaction".to_owned()))?;
        Ok(vec![TokenTransfer {
            token: TOKEN.to_owned(),
            to: TREASURY.to_owned(),
            amount: Money::new(amount),
            memo: None,
        }])
    }
}

struct Browser {
    gateway: Gateway<MemoryStore, Signer42, Chain>,
    identity: String,
}

impl Browser {
    fn new() -> Self {
        let service = Service::new(
            MemoryStore::default(),
            Signer42(SigningKey::from_bytes(&[42; 32])),
            Chain,
        );
        let mut gateway = Gateway::new(EnclaveSecret::from_bytes([7; 32]), service);
        let identity = serde_json::to_string(&run(&mut gateway, &Input::Identity)).unwrap();
        Self { gateway, identity }
    }

    fn send(&mut self, actor: &Value, command: &Value, secret: &str) -> ResponseDto {
        let prepared = prepare_request(
            &self.identity,
            COMPANY,
            &actor.to_string(),
            &command.to_string(),
            secret,
        )
        .unwrap();
        let output = run(
            &mut self.gateway,
            &Input::Request {
                envelope: prepared.envelope.clone(),
            },
        );
        let Output::Reply { envelope, .. } = output else {
            panic!("expected a reply, got {output:?}");
        };
        open_reply(&self.identity, &prepared, &envelope).unwrap()
    }

    /// Proposes as the company, then signs as every party listed.
    fn commit(&mut self, company: &str, event: &Value, signers: &[(&Value, &str)]) -> ResponseDto {
        let proposal = self.send(
            &company_actor(),
            &json!({ "kind": "propose", "event": event }),
            company,
        );
        let ResponseDto::Proposal(view) = proposal else {
            panic!("expected a proposal, got {proposal:?}");
        };
        let mut last = None;
        for (actor, secret) in signers {
            last = Some(self.send(
                actor,
                &json!({ "kind": "sign", "digest": view.digest }),
                secret,
            ));
        }
        last.unwrap()
    }
}

fn company_actor() -> Value {
    json!({ "role": "company" })
}

fn holder_actor(name: &str) -> Value {
    json!({ "role": "holder", "holder": name })
}

fn secret(byte: u8) -> String {
    hex::encode([byte; 32])
}

#[test]
fn a_whole_raise_runs_through_the_browser_protocol() {
    let mut browser = Browser::new();
    let company = secret(1);
    let ana = secret(2);
    let ana_actor = holder_actor("ana");

    let created = browser.send(
        &company_actor(),
        &json!({ "kind": "create_company" }),
        &company,
    );
    assert!(matches!(created, ResponseDto::CompanyCreated { .. }));

    let treasury = json!({ "kind": "treasury_designated", "account": TREASURY, "token": TOKEN });
    let committed = browser.commit(&company, &treasury, &[(&company_actor(), &company)]);
    assert!(
        matches!(committed, ResponseDto::Committed { .. }),
        "{committed:?}"
    );

    let registration = json!({
        "kind": "holder_registered",
        "holder": "ana",
        "key": party_public_key(&ana).unwrap(),
    });
    let committed = browser.commit(
        &company,
        &registration,
        &[(&company_actor(), &company), (&ana_actor, &ana)],
    );
    assert!(
        matches!(committed, ResponseDto::Committed { .. }),
        "{committed:?}"
    );

    let common = json!({

        "kind": "common_issued",

        "instrument": "stock-ana",

        "holder": "ana",

        "shares": "9000000",

    });

    let committed = browser.commit(
        &company,
        &common,
        &[(&company_actor(), &company), (&ana_actor, &ana)],
    );

    assert!(
        matches!(committed, ResponseDto::Committed { .. }),
        "{committed:?}"
    );

    let safe = json!({
        "kind": "safe_issued",
        "instrument": "safe-ana",
        "holder": "ana",
        "purchase_micros": "2000000",
        "valuation_cap_micros": "20000000",
        "discount_basis_points": null,
        "mfn": false,
        "settlement": "tx:2000000",
    });
    let committed = browser.commit(
        &company,
        &safe,
        &[(&company_actor(), &company), (&ana_actor, &ana)],
    );
    assert!(
        matches!(committed, ResponseDto::Committed { .. }),
        "{committed:?}"
    );

    let statement = browser.send(&ana_actor, &json!({ "kind": "get_statement" }), &ana);
    let ResponseDto::Statement(statement) = statement else {
        panic!("expected a statement, got {statement:?}");
    };
    assert_eq!(statement.lines.len(), 2);
    assert_eq!(
        statement.lines[1].ownership_basis_points.as_deref(),
        Some("1000")
    );
}

#[test]
fn a_holder_reviews_exactly_what_they_are_asked_to_sign() {
    let mut browser = Browser::new();
    let company = secret(1);
    let ana = secret(2);
    browser.send(
        &company_actor(),
        &json!({ "kind": "create_company" }),
        &company,
    );

    let registration = json!({
        "kind": "holder_registered",
        "holder": "ana",
        "key": party_public_key(&ana).unwrap(),
    });
    browser.send(
        &company_actor(),
        &json!({ "kind": "propose", "event": registration }),
        &company,
    );

    let seen = browser.send(
        &holder_actor("ana"),
        &json!({ "kind": "get_proposal" }),
        &ana,
    );
    let ResponseDto::Proposal(view) = seen else {
        panic!("expected a proposal, got {seen:?}");
    };
    assert_eq!(serde_json::to_value(&view.event).unwrap(), registration);
}

#[test]
fn a_reply_meant_for_another_request_is_refused() {
    let mut browser = Browser::new();
    let company = secret(1);
    let first = prepare_request(
        &browser.identity,
        COMPANY,
        &company_actor().to_string(),
        &json!({ "kind": "create_company" }).to_string(),
        &company,
    )
    .unwrap();
    let second = prepare_request(
        &browser.identity,
        COMPANY,
        &company_actor().to_string(),
        &json!({ "kind": "get_cap_table" }).to_string(),
        &company,
    )
    .unwrap();

    let output = run(
        &mut browser.gateway,
        &Input::Request {
            envelope: first.envelope.clone(),
        },
    );
    let Output::Reply { envelope, .. } = output else {
        panic!("expected a reply");
    };
    // The second request's key cannot even decrypt the first reply.
    assert!(open_reply(&browser.identity, &second, &envelope).is_err());
}

#[test]
fn bad_input_is_an_error_not_a_panic() {
    let browser = Browser::new();
    let bad = [
        (
            "not json",
            json!({ "kind": "create_company" }).to_string(),
            secret(1),
        ),
        (r#"{"role":"company"}"#, "{".to_owned(), secret(1)),
        (
            r#"{"role":"company"}"#,
            json!({ "kind": "create_company" }).to_string(),
            "zz".to_owned(),
        ),
    ];
    for (actor, command, secret) in bad {
        assert!(prepare_request(&browser.identity, COMPANY, actor, &command, &secret).is_err());
    }
}

#[test]
fn an_identity_with_a_swapped_encryption_key_is_refused() {
    let browser = Browser::new();
    let mut identity: serde_json::Value = serde_json::from_str(&browser.identity).unwrap();
    identity["encryption_key"] = json!("11".repeat(32));
    let swapped = identity.to_string();
    let request = prepare_request(
        &swapped,
        COMPANY,
        &company_actor().to_string(),
        &json!({ "kind": "create_company" }).to_string(),
        &secret(1),
    );
    assert!(request.is_err());
}
