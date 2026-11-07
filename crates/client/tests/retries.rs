//! A relay can report a timeout for a call that actually ran. Retrying must not
//! apply anything twice.

use undiluted_client::{
    Client, ClientError, EnclaveIdentity, Exchange, LocalTransport, Party, Transport,
};
use undiluted_domain::{CompanyId, HolderId, InstrumentId, Shares};
use undiluted_ledger::Event;
use undiluted_service::{Actor, Command, Response};

const MAX_ATTEMPTS: u32 = 3;

/// Runs every call on the enclave, but loses the answer of the first attempt.
struct LosesEveryOtherAnswer {
    inner: LocalTransport,
    lose_next: bool,
    identity_failures: u32,
}

impl LosesEveryOtherAnswer {
    fn new(identity_failures: u32) -> Self {
        Self {
            inner: LocalTransport::new().unwrap(),
            lose_next: true,
            identity_failures,
        }
    }
}

impl Transport for LosesEveryOtherAnswer {
    fn identity(&mut self) -> Result<EnclaveIdentity, ClientError> {
        if self.identity_failures > 0 {
            self.identity_failures -= 1;
            return Err(ClientError::Transport("timeout".to_owned()));
        }
        self.inner.identity()
    }

    fn submit(&mut self, envelope: &[u8]) -> Result<Exchange, ClientError> {
        let exchange = self.inner.submit(envelope)?;
        self.lose_next = !self.lose_next;
        if self.lose_next {
            Ok(exchange)
        } else {
            Err(ClientError::Transport("timeout".to_owned()))
        }
    }
}

fn company() -> CompanyId {
    CompanyId::new("acme").unwrap()
}

#[test]
fn connecting_survives_a_timeout() {
    let transport = LosesEveryOtherAnswer::new(MAX_ATTEMPTS - 1);
    assert!(Client::connect(transport, company(), MAX_ATTEMPTS).is_ok());
}

#[test]
fn connecting_gives_up_after_the_last_attempt() {
    let transport = LosesEveryOtherAnswer::new(MAX_ATTEMPTS);
    assert!(matches!(
        Client::connect(transport, company(), MAX_ATTEMPTS),
        Err(ClientError::Transport(_))
    ));
}

#[test]
fn lost_answers_do_not_apply_events_twice() {
    let transport = LosesEveryOtherAnswer::new(0);
    let mut client = Client::connect(transport, company(), MAX_ATTEMPTS).unwrap();
    let founders = Party::generate().unwrap();
    let ana = Party::generate().unwrap();
    let ana_id = HolderId::new("ana").unwrap();
    let ana_actor = Actor::Holder(ana_id.clone());

    let created = client
        .send(
            &founders,
            Actor::Company,
            Command::CreateCompany {
                key: founders.public_key(),
            },
        )
        .unwrap();
    assert!(matches!(created, Response::CompanyCreated { .. }));

    let events = [
        Event::HolderRegistered {
            holder: ana_id.clone(),
            key: ana.public_key(),
        },
        Event::CommonIssued {
            instrument: InstrumentId::new("stock").unwrap(),
            holder: ana_id,
            shares: Shares::new(100),
        },
    ];
    for event in events {
        let response = client
            .commit(
                (&founders, Actor::Company),
                event,
                &[(&founders, Actor::Company), (&ana, ana_actor.clone())],
            )
            .unwrap();
        assert!(matches!(response, Response::Committed { .. }));
    }

    let statement = client.statement(&ana, ana_actor).unwrap();
    assert_eq!(statement.entries, 2);
    assert_eq!(statement.capitalization, Shares::new(100));
}
