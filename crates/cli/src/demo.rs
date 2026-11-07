//! A fictional raise: a founder, two investors and an MFN clause that enforces
//! itself. Every party signs with its own key and sees only what it may see.

use undiluted_client::{AnchorRegistry, Client, ClientError, Party, Transport};
use undiluted_domain::{
    CompanyId, HolderId, InstrumentId, MfnOffer, Money, Ownership, Ratio, Safe, SafeTerms, Shares,
};
use undiluted_ledger::{Event, SettlementReference};
use undiluted_service::wire::AnchorOutput;
use undiluted_service::{Actor, Command, Response, Statement};

const MICROS_PER_DOLLAR: u128 = 1_000_000;
const RATIO_PER_BASIS_POINT: u128 = 100_000_000_000_000;
const MAX_ATTEMPTS: u32 = 8;

const FOUNDER_SHARES: u128 = 8_000_000;
const ANA_INVESTMENT: u128 = 250_000;
const BEN_INVESTMENT: u128 = 500_000;
const BEN_POST_MONEY_CAP: u128 = 5_000_000;
const DEMO_SETTLEMENT: &str = "demo-settlement";
const FORGED_HEAD: [u8; 32] = [0xab; 32];
const FORGED_SIGNATURE: [u8; 64] = [0xcd; 64];

/// A holder in the scenario, with the identifier and key they act with.
struct Member {
    id: HolderId,
    party: Party,
}

impl Member {
    fn new(name: &str) -> Result<Self, ClientError> {
        Ok(Self {
            id: HolderId::new(name)?,
            party: Party::generate()?,
        })
    }

    fn actor(&self) -> Actor {
        Actor::Holder(self.id.clone())
    }
}

struct Demo<T, R> {
    client: Client<T>,
    anchors: R,
    company_id: CompanyId,
    company: Party,
    founder: Member,
    ana: Member,
    ben: Member,
}

pub fn run<T: Transport, R: AnchorRegistry>(transport: T, anchors: R) -> Result<(), ClientError> {
    let mut demo = Demo::connect(transport, anchors)?;
    demo.incorporate()?;
    demo.raise()?;
    let offer = demo.discover_better_terms()?;
    demo.exercise_mfn(offer)?;
    demo.check_privacy()?;
    demo.check_history()?;
    println!("\nDone. Every reply was signed by the enclave and checked against its key.");
    Ok(())
}

impl<T: Transport, R: AnchorRegistry> Demo<T, R> {
    fn connect(transport: T, anchors: R) -> Result<Self, ClientError> {
        let company = Party::generate()?;
        let suffix = hex::encode(&company.public_key().as_bytes()[..4]);
        let company_id = CompanyId::new(format!("demo-{suffix}"))?;
        let client = Client::connect(transport, company_id.clone(), MAX_ATTEMPTS)?;
        println!(
            "Connected to the enclave. Signing key: {}",
            hex::encode(client.identity().signing_key.as_bytes())
        );
        Ok(Self {
            client,
            anchors,
            company_id,
            company,
            founder: Member::new("founder")?,
            ana: Member::new("ana")?,
            ben: Member::new("ben")?,
        })
    }

    fn incorporate(&mut self) -> Result<(), ClientError> {
        step("The company registers its signing key");
        let created = self.client.send(
            &self.company,
            Actor::Company,
            Command::CreateCompany {
                key: self.company.public_key(),
            },
        )?;
        expect(matches!(created, Response::CompanyCreated { .. }), &created)?;
        let enclave_key = self.client.identity().signing_key;
        self.anchors.register(&self.company_id, &enclave_key)?;

        step("The founder and two investors register, each proving their own key");
        let members = [
            (self.founder.id.clone(), self.founder.party.clone()),
            (self.ana.id.clone(), self.ana.party.clone()),
            (self.ben.id.clone(), self.ben.party.clone()),
        ];
        for (id, party) in members {
            let event = Event::HolderRegistered {
                holder: id.clone(),
                key: party.public_key(),
            };
            self.commit(event, &party, Actor::Holder(id))?;
        }

        step("The founder receives common stock");
        let event = Event::CommonIssued {
            instrument: InstrumentId::new("founder-stock")?,
            holder: self.founder.id.clone(),
            shares: Shares::new(FOUNDER_SHARES),
        };
        let (party, actor) = (self.founder.party.clone(), self.founder.actor());
        self.commit(event, &party, actor)
    }

    fn raise(&mut self) -> Result<(), ClientError> {
        step("Ana invests with an uncapped SAFE that has an MFN clause");
        let event = issue_safe("safe-ana", &self.ana, ANA_INVESTMENT, None, true)?;
        let (party, actor) = (self.ana.party.clone(), self.ana.actor());
        self.commit(event, &party, actor)?;
        print_statement(
            "Ana",
            &self.client.statement(&self.ana.party, self.ana.actor())?,
        );

        step("Ben invests later at a $5M post-money cap: better terms than Ana has");
        let event = issue_safe(
            "safe-ben",
            &self.ben,
            BEN_INVESTMENT,
            Some(BEN_POST_MONEY_CAP),
            false,
        )?;
        let (party, actor) = (self.ben.party.clone(), self.ben.actor());
        self.commit(event, &party, actor)
    }

    fn discover_better_terms(&mut self) -> Result<MfnOffer, ClientError> {
        step("Ana's statement now offers her those terms, without naming Ben or his amount");
        let statement = self.client.statement(&self.ana.party, self.ana.actor())?;
        print_statement("Ana", &statement);
        statement
            .mfn_offers
            .into_iter()
            .next()
            .ok_or_else(|| ClientError::Protocol("expected an MFN offer".to_owned()))
    }

    fn exercise_mfn(&mut self, offer: MfnOffer) -> Result<(), ClientError> {
        step("Ana exercises the clause alone: the company cannot block it");
        let election = self.client.propose(
            &self.ana.party,
            self.ana.actor(),
            Event::MfnElected {
                target: offer.target,
                source: offer.source,
            },
        )?;
        let committed = self.client.send(
            &self.ana.party,
            self.ana.actor(),
            Command::Sign {
                digest: election.digest,
                signature: self.ana.party.sign(&election.digest),
            },
        )?;
        expect(matches!(committed, Response::Committed { .. }), &committed)?;
        self.publish_anchors()?;
        print_statement(
            "Ana",
            &self.client.statement(&self.ana.party, self.ana.actor())?,
        );
        Ok(())
    }

    fn check_privacy(&mut self) -> Result<(), ClientError> {
        step("Ben asks for the whole table and is refused");
        let refused = self
            .client
            .send(&self.ben.party, self.ben.actor(), Command::GetCapTable)?;
        print_refusal(&refused)?;

        step("Someone uses Ben's name with another key and is refused");
        let impostor = Party::generate()?;
        let refused = self
            .client
            .send(&impostor, self.ben.actor(), Command::GetStatement)?;
        print_refusal(&refused)?;

        step("Ben sees only his own line, and that the total reconciles");
        print_statement(
            "Ben",
            &self.client.statement(&self.ben.party, self.ben.actor())?,
        );
        Ok(())
    }

    fn check_history(&mut self) -> Result<(), ClientError> {
        step("Ben checks the head in his statement against the one published on chain");
        let statement = self.client.statement(&self.ben.party, self.ben.actor())?;
        let sequence = statement.entries - 1;
        let shown = hex::encode(statement.head.as_bytes());
        let published = self.anchors.head_at(&self.company_id, sequence)?;
        if published.as_deref() != Some(shown.as_str()) {
            return Err(ClientError::Anchor(format!(
                "the head at {sequence} does not match: {published:?}"
            )));
        }
        println!("  company  {}", self.company_id);
        println!("  sequence {sequence}");
        println!("  head     {shown}");
        println!("  They match, so this is the one history everyone else sees too.");

        step("A forged head for the next entry, not signed by the enclave, is refused");
        let forged = AnchorOutput {
            company: self.company_id.to_string(),
            sequence: statement.entries,
            head: hex::encode(FORGED_HEAD),
            signature: hex::encode(FORGED_SIGNATURE),
        };
        match self.anchors.publish(&forged) {
            Err(ClientError::Anchor(_)) => println!("  Refused by the registry."),
            other => {
                return Err(ClientError::Protocol(format!(
                    "the forged head was not refused: {other:?}"
                )));
            }
        }
        Ok(())
    }

    fn commit(&mut self, event: Event, holder: &Party, actor: Actor) -> Result<(), ClientError> {
        let company = self.company.clone();
        let response = self.client.commit(
            (&company, Actor::Company),
            event,
            &[(&company, Actor::Company), (holder, actor)],
        )?;
        expect(matches!(response, Response::Committed { .. }), &response)?;
        self.publish_anchors()
    }

    fn publish_anchors(&mut self) -> Result<(), ClientError> {
        for anchor in self.client.take_anchors() {
            self.publish_once(&anchor)?;
        }
        Ok(())
    }

    /// Publishing can report a failure for a call that did land, so a failure
    /// only counts if the head really is not there afterwards.
    fn publish_once(&mut self, anchor: &AnchorOutput) -> Result<(), ClientError> {
        let mut attempts = 0;
        loop {
            attempts += 1;
            let result = self.anchors.publish(anchor);
            if result.is_ok() {
                return result;
            }
            let recorded = self.anchors.head_at(&self.company_id, anchor.sequence)?;
            if recorded.as_deref() == Some(anchor.head.as_str()) {
                return Ok(());
            }
            if attempts >= MAX_ATTEMPTS {
                return result;
            }
        }
    }
}

fn usd(dollars: u128) -> Money {
    Money::new(dollars * MICROS_PER_DOLLAR)
}

fn issue_safe(
    id: &str,
    holder: &Member,
    purchase: u128,
    cap: Option<u128>,
    mfn: bool,
) -> Result<Event, ClientError> {
    let terms = SafeTerms::new(cap.map(usd), None, mfn)?;
    Ok(Event::SafeIssued {
        instrument: InstrumentId::new(id)?,
        holder: holder.id.clone(),
        safe: Safe::new(usd(purchase), terms)?,
        settlement: SettlementReference::new(DEMO_SETTLEMENT)?,
    })
}

fn step(title: &str) {
    println!("\n> {title}");
}

fn expect(condition: bool, response: &Response) -> Result<(), ClientError> {
    if condition {
        Ok(())
    } else {
        Err(ClientError::Protocol(format!(
            "unexpected answer: {response:?}"
        )))
    }
}

fn print_refusal(response: &Response) -> Result<(), ClientError> {
    let Response::Rejected { reason } = response else {
        return Err(ClientError::Protocol(format!(
            "expected a refusal, got: {response:?}"
        )));
    };
    println!("  Refused: {reason}");
    Ok(())
}

fn print_statement(name: &str, statement: &Statement) {
    println!(
        "  {name}: total reconciles: {} | {} entries in the ledger",
        statement.reconciles, statement.entries
    );
    for line in &statement.lines {
        match line.ownership {
            Ownership::Determined { shares, ratio } => println!(
                "    {}: {} shares, {}",
                line.instrument,
                shares.value(),
                percent(ratio)
            ),
            Ownership::PendingRound => {
                println!("    {}: set when a priced round happens", line.instrument);
            }
        }
    }
    for offer in &statement.mfn_offers {
        let cap = offer.terms.valuation_cap().map_or_else(
            || "none".to_owned(),
            |cap| format!("${}", cap.value() / MICROS_PER_DOLLAR),
        );
        println!("    MFN offer on {}: cap {cap}", offer.target);
    }
}

fn percent(ratio: Ratio) -> String {
    let basis_points = ratio.scaled() / RATIO_PER_BASIS_POINT;
    format!("{}.{:02}%", basis_points / 100, basis_points % 100)
}
