//! End-to-end behaviour of the service through signed requests.

use ed25519_dalek::{Signer, SigningKey};
use undiluted_domain::{
    CompanyId, HolderId, InstrumentId, Money, Ownership, Ratio, Safe, SafeTerms, Shares,
};
use undiluted_ledger::{
    Digest, Event, PayoutDestination, PublicKey, SettlementReference, SignatureBytes, Treasury,
};
use undiluted_service::{
    Actor, ChainError, ChainReader, Command, EnclaveSigner, MemoryStore, PayoutInstruction, Reply,
    Response, Service, SignedRequest, StoreError, TokenTransfer,
};

const COMPANY_SEED: u8 = 1;
const ANA_SEED: u8 = 2;
const BEN_SEED: u8 = 3;
const MALLORY_SEED: u8 = 9;

#[derive(Debug)]
struct TestSigner(SigningKey);

impl EnclaveSigner for TestSigner {
    fn public_key(&self) -> Result<PublicKey, StoreError> {
        Ok(PublicKey::from_bytes(self.0.verifying_key().to_bytes()))
    }

    fn sign(&self, message: &[u8]) -> Result<SignatureBytes, StoreError> {
        Ok(SignatureBytes::from_bytes(self.0.sign(message).to_bytes()))
    }
}

const TREASURY: &str = "treasury.near";
const TOKEN: &str = "usdc.near";

/// A chain where a reference spells out what it paid: `<token>:<to>:<amount>:<id>`.
#[derive(Debug)]
struct TestChain;

impl ChainReader for TestChain {
    fn token_transfers(&self, reference: &str) -> Result<Vec<TokenTransfer>, ChainError> {
        let parts: Vec<&str> = reference.split(':').collect();
        let [token, to, amount, _] = parts[..] else {
            return Err(ChainError("no such transaction".to_owned()));
        };
        Ok(vec![TokenTransfer {
            token: token.to_owned(),
            to: to.to_owned(),
            amount: Money::new(amount.parse().unwrap()),
        }])
    }
}

fn key(seed: u8) -> SigningKey {
    SigningKey::from_bytes(&[seed; 32])
}

fn public(seed: u8) -> PublicKey {
    PublicKey::from_bytes(key(seed).verifying_key().to_bytes())
}

fn sign_digest(seed: u8, digest: &Digest) -> SignatureBytes {
    SignatureBytes::from_bytes(key(seed).sign(digest.as_bytes()).to_bytes())
}

fn holder(name: &str) -> HolderId {
    HolderId::new(name).unwrap()
}

fn instrument(name: &str) -> InstrumentId {
    InstrumentId::new(name).unwrap()
}

fn usd(dollars: u128) -> Money {
    Money::new(dollars * 1_000_000)
}

struct Harness {
    service: Service<MemoryStore, TestSigner, TestChain>,
    company: CompanyId,
    next_id: u8,
}

impl Harness {
    fn new() -> Self {
        let mut harness = Self {
            service: Service::new(MemoryStore::default(), TestSigner(key(42)), TestChain),
            company: CompanyId::new("acme").unwrap(),
            next_id: 0,
        };
        let reply = harness.send(
            COMPANY_SEED,
            Actor::Company,
            Command::CreateCompany {
                key: public(COMPANY_SEED),
            },
        );
        assert!(matches!(reply.response, Response::CompanyCreated { .. }));
        harness
    }

    fn request(&mut self, seed: u8, actor: Actor, command: Command) -> SignedRequest {
        self.next_id += 1;
        let id = [self.next_id; 16];
        let digest = SignedRequest::signing_digest(&id, &self.company, &actor, &command).unwrap();
        SignedRequest {
            id,
            company: self.company.clone(),
            actor,
            command,
            signature: sign_digest(seed, &digest),
        }
    }

    fn send(&mut self, seed: u8, actor: Actor, command: Command) -> Reply {
        let request = self.request(seed, actor, command);
        self.service.handle(&request).unwrap()
    }

    /// Runs an event through proposal and every signature it needs.
    fn commit(&mut self, event: Event, holders: &[(&str, u8)]) -> Reply {
        let digest = self.propose(event);
        self.send(
            COMPANY_SEED,
            Actor::Company,
            Command::Sign {
                digest,
                signature: sign_digest(COMPANY_SEED, &digest),
            },
        );
        let mut last = None;
        for (name, seed) in holders {
            last = Some(self.send(
                *seed,
                Actor::Holder(holder(name)),
                Command::Sign {
                    digest,
                    signature: sign_digest(*seed, &digest),
                },
            ));
        }
        let reply = last.expect("events in these tests have a holder");
        assert!(
            matches!(reply.response, Response::Committed { .. }),
            "{reply:?}"
        );
        reply
    }

    fn propose(&mut self, event: Event) -> Digest {
        let reply = self.send(COMPANY_SEED, Actor::Company, Command::Propose { event });
        match reply.response {
            Response::Proposal(view) => view.digest,
            other => panic!("expected a proposal, got {other:?}"),
        }
    }

    fn register(&mut self, name: &str, seed: u8) {
        self.commit(
            Event::HolderRegistered {
                holder: holder(name),
                key: public(seed),
            },
            &[(name, seed)],
        );
    }

    fn statement(&mut self, name: &str, seed: u8) -> undiluted_service::Statement {
        match self
            .send(seed, Actor::Holder(holder(name)), Command::GetStatement)
            .response
        {
            Response::Statement(statement) => statement,
            other => panic!("expected a statement, got {other:?}"),
        }
    }
}

fn paid_reference(id: &str, purchase: u128) -> String {
    format!("{TOKEN}:{TREASURY}:{}:{id}", usd(purchase).value())
}

fn safe(id: &str, owner: &str, purchase: u128, cap: Option<u128>, mfn: bool) -> Event {
    Event::SafeIssued {
        instrument: instrument(id),
        holder: holder(owner),
        safe: Safe::new(
            usd(purchase),
            SafeTerms::new(cap.map(usd), None, mfn).unwrap(),
        )
        .unwrap(),
        settlement: SettlementReference::new(paid_reference(id, purchase)).unwrap(),
    }
}

fn rejected(reply: &Reply) -> &str {
    match &reply.response {
        Response::Rejected { reason } => reason,
        other => panic!("expected a rejection, got {other:?}"),
    }
}

/// Ana holds the founders' shares and an MFN SAFE, Ben a capped SAFE.
fn seeded() -> Harness {
    let mut harness = Harness::new();
    let digest = harness.propose(Event::TreasuryDesignated {
        treasury: Treasury::new(TREASURY, TOKEN).unwrap(),
    });
    harness.send(
        COMPANY_SEED,
        Actor::Company,
        Command::Sign {
            digest,
            signature: sign_digest(COMPANY_SEED, &digest),
        },
    );
    harness.register("ana", ANA_SEED);
    harness.register("ben", BEN_SEED);
    harness.commit(
        Event::CommonIssued {
            instrument: instrument("founders"),
            holder: holder("ana"),
            shares: Shares::new(8_000_000),
        },
        &[("ana", ANA_SEED)],
    );
    harness.commit(safe("mfn", "ana", 100, None, true), &[("ana", ANA_SEED)]);
    harness.commit(
        safe("ben-safe", "ben", 500_000, Some(5_000_000), false),
        &[("ben", BEN_SEED)],
    );
    harness
}

#[test]
fn a_statement_shows_only_the_holders_own_lines() {
    let mut harness = seeded();

    let statement = harness.statement("ben", BEN_SEED);

    assert_eq!(statement.lines.len(), 1);
    assert_eq!(statement.lines[0].instrument, instrument("ben-safe"));
    assert_eq!(
        statement.lines[0].ownership,
        Ownership::Determined {
            shares: Shares::new(888_888),
            ratio: Ratio::from_fraction(1, 10).unwrap(),
        }
    );
    assert!(statement.reconciles);
}

#[test]
fn a_better_later_safe_reaches_the_mfn_holder_who_elects_alone() {
    let mut harness = seeded();

    let offers = harness.statement("ana", ANA_SEED).mfn_offers;
    assert_eq!(offers.len(), 1);
    assert_eq!(offers[0].source, instrument("ben-safe"));

    let event = Event::MfnElected {
        target: instrument("mfn"),
        source: instrument("ben-safe"),
    };
    let actor = Actor::Holder(holder("ana"));
    let digest = match harness
        .send(ANA_SEED, actor.clone(), Command::Propose { event })
        .response
    {
        Response::Proposal(view) => {
            assert!(!view.company_signature_missing);
            view.digest
        }
        other => panic!("expected a proposal, got {other:?}"),
    };
    let reply = harness.send(
        ANA_SEED,
        actor,
        Command::Sign {
            digest,
            signature: sign_digest(ANA_SEED, &digest),
        },
    );

    assert!(matches!(reply.response, Response::Committed { .. }));
    assert_eq!(harness.statement("ana", ANA_SEED).mfn_offers.len(), 0);
}

#[test]
fn only_the_company_sees_the_whole_table() {
    let mut harness = seeded();

    let reply = harness.send(COMPANY_SEED, Actor::Company, Command::GetCapTable);
    assert!(matches!(reply.response, Response::CapTable(_)));

    let reply = harness.send(BEN_SEED, Actor::Holder(holder("ben")), Command::GetCapTable);
    assert_eq!(rejected(&reply), "this actor may not do that");
}

#[test]
fn a_holder_cannot_propose_issuing_shares_or_read_others_proposals() {
    let mut harness = seeded();
    let ben = Actor::Holder(holder("ben"));

    let event = Event::CommonIssued {
        instrument: instrument("free"),
        holder: holder("ben"),
        shares: Shares::new(1),
    };
    let reply = harness.send(BEN_SEED, ben.clone(), Command::Propose { event });
    assert_eq!(rejected(&reply), "this actor may not do that");

    harness.propose(safe("for-ana", "ana", 1, Some(10), false));
    let reply = harness.send(BEN_SEED, ben, Command::GetProposal);
    assert_eq!(rejected(&reply), "this actor may not do that");
}

#[test]
fn nobody_can_act_as_someone_else() {
    let mut harness = seeded();

    let reply = harness.send(MALLORY_SEED, Actor::Company, Command::GetCapTable);
    assert_eq!(rejected(&reply), "request signature is invalid");

    let reply = harness.send(
        MALLORY_SEED,
        Actor::Holder(holder("ben")),
        Command::GetStatement,
    );
    assert_eq!(rejected(&reply), "request signature is invalid");
}

#[test]
fn an_invited_holder_can_read_the_proposal_before_registering() {
    let mut harness = seeded();
    harness.propose(Event::HolderRegistered {
        holder: holder("eve"),
        key: public(MALLORY_SEED),
    });

    let reply = harness.send(
        MALLORY_SEED,
        Actor::Holder(holder("eve")),
        Command::GetProposal,
    );

    assert!(matches!(reply.response, Response::Proposal(_)));
}

#[test]
fn an_instrument_waits_for_its_holder() {
    let mut harness = seeded();
    let digest = harness.propose(safe("new", "ben", 1, Some(10), false));

    let reply = harness.send(
        COMPANY_SEED,
        Actor::Company,
        Command::Sign {
            digest,
            signature: sign_digest(COMPANY_SEED, &digest),
        },
    );

    match reply.response {
        Response::Proposal(view) => {
            assert_eq!(view.holder_signatures_missing, vec![holder("ben")]);
        }
        other => panic!("expected a pending proposal, got {other:?}"),
    }
}

#[test]
fn a_signature_for_another_digest_is_refused() {
    let mut harness = seeded();
    harness.propose(safe("new", "ben", 1, Some(10), false));
    let other = Digest::tagged("other", b"proposal");

    let reply = harness.send(
        BEN_SEED,
        Actor::Holder(holder("ben")),
        Command::Sign {
            digest: other,
            signature: sign_digest(BEN_SEED, &other),
        },
    );

    assert_eq!(
        rejected(&reply),
        "this signature is not for the pending proposal"
    );
}

#[test]
fn only_one_proposal_can_be_open_until_it_is_cancelled() {
    let mut harness = seeded();
    harness.propose(safe("first", "ben", 1, Some(10), false));

    let event = safe("second", "ben", 1, Some(10), false);
    let reply = harness.send(
        COMPANY_SEED,
        Actor::Company,
        Command::Propose {
            event: event.clone(),
        },
    );
    assert_eq!(
        rejected(&reply),
        "another proposal is waiting for signatures"
    );

    let reply = harness.send(COMPANY_SEED, Actor::Company, Command::CancelProposal);
    assert!(matches!(reply.response, Response::Cancelled));
    harness.propose(event);
}

#[test]
fn retrying_a_request_returns_the_first_answer_without_acting_twice() {
    let mut harness = seeded();
    let digest = harness.propose(safe("new", "ben", 1, Some(10), false));
    harness.send(
        COMPANY_SEED,
        Actor::Company,
        Command::Sign {
            digest,
            signature: sign_digest(COMPANY_SEED, &digest),
        },
    );
    let request = harness.request(
        BEN_SEED,
        Actor::Holder(holder("ben")),
        Command::Sign {
            digest,
            signature: sign_digest(BEN_SEED, &digest),
        },
    );

    let first = harness.service.handle(&request).unwrap();
    let retry = harness.service.handle(&request).unwrap();

    assert!(matches!(first.response, Response::Committed { .. }));
    assert_eq!(first, retry);
    assert_eq!(harness.statement("ben", BEN_SEED).entries, 7);
}

#[test]
fn a_rejection_is_not_remembered_so_a_retry_can_succeed() {
    let mut harness = seeded();
    harness.propose(safe("first", "ben", 1, Some(10), false));
    let event = safe("second", "ben", 1, Some(10), false);
    let request = harness.request(COMPANY_SEED, Actor::Company, Command::Propose { event });

    assert_eq!(
        rejected(&harness.service.handle(&request).unwrap()),
        "another proposal is waiting for signatures"
    );

    harness.send(COMPANY_SEED, Actor::Company, Command::CancelProposal);
    let retry = harness.service.handle(&request).unwrap();
    assert!(matches!(retry.response, Response::Proposal(_)));
}

#[test]
fn replies_are_signed_by_the_enclave_and_bound_to_the_request() {
    let mut harness = seeded();
    let enclave = PublicKey::from_bytes(key(42).verifying_key().to_bytes());
    let request = harness.request(COMPANY_SEED, Actor::Company, Command::GetCapTable);

    let reply = harness.service.handle(&request).unwrap();

    assert!(reply.is_authentic(&enclave));
    assert!(!reply.is_authentic(&public(MALLORY_SEED)));
    assert_eq!(reply.request, request.digest().unwrap());

    let mut forged = reply;
    forged.response = Response::Cancelled;
    assert!(!forged.is_authentic(&enclave));
}

#[test]
fn committing_an_event_produces_an_attested_head() {
    let mut harness = seeded();

    let reply = harness.commit(safe("new", "ben", 1, Some(10), false), &[("ben", BEN_SEED)]);

    let anchor = reply.anchor.expect("a commit has an anchor");
    let Response::Committed { sequence, head } = reply.response else {
        panic!("expected a commit");
    };
    assert_eq!(anchor.sequence, sequence);
    assert_eq!(anchor.head, head);
}

#[test]
fn a_company_cannot_be_created_twice() {
    let mut harness = Harness::new();
    let reply = harness.send(
        MALLORY_SEED,
        Actor::Company,
        Command::CreateCompany {
            key: public(MALLORY_SEED),
        },
    );
    assert_eq!(rejected(&reply), "this company already exists");
}

#[test]
fn an_unknown_company_is_refused() {
    let mut harness = Harness::new();
    harness.company = CompanyId::new("ghost").unwrap();
    let reply = harness.send(COMPANY_SEED, Actor::Company, Command::GetCapTable);
    assert_eq!(rejected(&reply), "this company does not exist");
}

fn holder_sets_destination(harness: &mut Harness, name: &str, seed: u8) {
    let actor = Actor::Holder(holder(name));
    let event = Event::PayoutDestinationSet {
        holder: holder(name),
        destination: PayoutDestination::new(format!("{name}.near")).unwrap(),
    };
    let digest = match harness
        .send(seed, actor.clone(), Command::Propose { event })
        .response
    {
        Response::Proposal(view) => view.digest,
        other => panic!("expected a proposal, got {other:?}"),
    };
    let reply = harness.send(
        seed,
        actor,
        Command::Sign {
            digest,
            signature: sign_digest(seed, &digest),
        },
    );
    assert!(
        matches!(reply.response, Response::Committed { .. }),
        "{reply:?}"
    );
}

fn exited() -> Harness {
    let mut harness = seeded();
    holder_sets_destination(&mut harness, "ana", ANA_SEED);
    holder_sets_destination(&mut harness, "ben", BEN_SEED);
    let digest = harness.propose(Event::ExitApproved {
        proceeds: usd(50_000_000),
    });
    let reply = harness.send(
        COMPANY_SEED,
        Actor::Company,
        Command::Sign {
            digest,
            signature: sign_digest(COMPANY_SEED, &digest),
        },
    );
    assert!(
        matches!(reply.response, Response::Committed { .. }),
        "{reply:?}"
    );
    harness
}

#[test]
fn a_holder_cannot_choose_where_someone_else_gets_paid() {
    let mut harness = seeded();
    let event = Event::PayoutDestinationSet {
        holder: holder("ana"),
        destination: PayoutDestination::new("ben.near").unwrap(),
    };
    let reply = harness.send(
        BEN_SEED,
        Actor::Holder(holder("ben")),
        Command::Propose { event },
    );
    assert_eq!(rejected(&reply), "this actor may not do that");
}

#[test]
fn payout_instructions_are_signed_and_add_up_to_the_proceeds() {
    let mut harness = exited();
    let enclave = PublicKey::from_bytes(key(42).verifying_key().to_bytes());

    let reply = harness.send(COMPANY_SEED, Actor::Company, Command::GetPayoutPlan);
    let Response::PayoutPlan(plan) = reply.response else {
        panic!("expected a plan");
    };

    let total: u128 = plan.instructions.iter().map(|i| i.amount.value()).sum();
    assert_eq!(total, usd(50_000_000).value());
    for instruction in &plan.instructions {
        let digest = PayoutInstruction::signing_digest(
            &harness.company,
            instruction.index,
            &instruction.recipient,
            instruction.amount,
        )
        .unwrap();
        assert!(enclave.verifies(&digest, &instruction.signature));
    }
    assert_eq!(plan.instructions[0].index, 0);
    assert_eq!(plan.instructions[0].recipient, "ana.near");
}

#[test]
fn only_the_company_gets_the_full_payout_plan() {
    let mut harness = exited();
    let reply = harness.send(
        BEN_SEED,
        Actor::Holder(holder("ben")),
        Command::GetPayoutPlan,
    );
    assert_eq!(rejected(&reply), "this actor may not do that");
}

#[test]
fn a_plan_needs_an_approved_exit() {
    let mut harness = seeded();
    let reply = harness.send(COMPANY_SEED, Actor::Company, Command::GetPayoutPlan);
    assert_eq!(rejected(&reply), "no exit has been approved");
}

#[test]
fn a_holder_sees_only_their_own_payout_and_that_the_total_matches() {
    let mut harness = exited();

    let exit = harness.statement("ben", BEN_SEED).exit.unwrap();

    assert!(exit.total_matches_proceeds);
    assert_eq!(exit.payouts.len(), 1);
    assert_eq!(exit.payouts[0].holder, holder("ben"));
}

#[test]
fn nothing_can_be_issued_after_an_exit() {
    let mut harness = exited();
    let event = safe("late", "ben", 1, Some(10), false);
    let reply = harness.send(COMPANY_SEED, Actor::Company, Command::Propose { event });
    assert_eq!(
        rejected(&reply),
        "this company has already approved an exit"
    );
}

fn safe_paid_with(id: &str, purchase: u128, reference: &str) -> Event {
    Event::SafeIssued {
        instrument: instrument(id),
        holder: holder("ben"),
        safe: Safe::new(
            usd(purchase),
            SafeTerms::new(Some(usd(purchase * 10)), None, false).unwrap(),
        )
        .unwrap(),
        settlement: SettlementReference::new(reference).unwrap(),
    }
}

fn proposal_rejection(harness: &mut Harness, event: Event) -> String {
    let reply = harness.send(COMPANY_SEED, Actor::Company, Command::Propose { event });
    rejected(&reply).to_owned()
}

#[test]
fn a_payment_that_does_not_exist_is_refused() {
    let mut harness = seeded();
    let reason = proposal_rejection(&mut harness, safe_paid_with("fake", 5, "invented"));
    assert_eq!(
        reason,
        "the payment could not be confirmed on chain: no such transaction"
    );
}

#[test]
fn a_payment_to_someone_else_is_refused() {
    let mut harness = seeded();
    let elsewhere = format!("{TOKEN}:other.near:{}:x", usd(5).value());
    let reason = proposal_rejection(&mut harness, safe_paid_with("elsewhere", 5, &elsewhere));
    assert!(reason.starts_with("the payment does not match"), "{reason}");
}

#[test]
fn a_payment_in_another_token_is_refused() {
    let mut harness = seeded();
    let other_token = format!("fake.near:{TREASURY}:{}:x", usd(5).value());
    let reason = proposal_rejection(&mut harness, safe_paid_with("token", 5, &other_token));
    assert!(reason.starts_with("the payment does not match"), "{reason}");
}

#[test]
fn a_smaller_payment_cannot_back_a_larger_safe() {
    let mut harness = seeded();
    let small = format!("{TOKEN}:{TREASURY}:{}:x", usd(1).value());
    let reason = proposal_rejection(&mut harness, safe_paid_with("large", 500, &small));
    assert!(reason.starts_with("the payment does not match"), "{reason}");
}

#[test]
fn one_payment_cannot_back_two_safes() {
    let mut harness = seeded();
    let reused = paid_reference("ben-safe", 500_000);
    let reason = proposal_rejection(&mut harness, safe_paid_with("again", 500_000, &reused));
    assert_eq!(
        reason,
        "this payment was already used for another instrument"
    );
}

#[test]
fn a_payment_used_by_one_company_cannot_back_another() {
    let mut harness = seeded();
    harness.company = CompanyId::new("globex").unwrap();
    harness.send(
        COMPANY_SEED,
        Actor::Company,
        Command::CreateCompany {
            key: public(COMPANY_SEED),
        },
    );
    let digest = harness.propose(Event::TreasuryDesignated {
        treasury: Treasury::new(TREASURY, TOKEN).unwrap(),
    });
    harness.send(
        COMPANY_SEED,
        Actor::Company,
        Command::Sign {
            digest,
            signature: sign_digest(COMPANY_SEED, &digest),
        },
    );

    let reused = paid_reference("ben-safe", 500_000);
    let reason = proposal_rejection(&mut harness, safe_paid_with("again", 500_000, &reused));
    assert_eq!(
        reason,
        "this payment was already used for another instrument"
    );
}

#[test]
fn a_safe_needs_a_treasury_to_check_against() {
    let mut harness = Harness::new();
    let reason = proposal_rejection(&mut harness, safe("early", "ben", 1, Some(10), false));
    assert_eq!(
        reason,
        "no treasury has been set, so the payment cannot be checked"
    );
}
