//! A fictional raise: a founder, two investors and an MFN clause that enforces
//! itself. Every party signs with its own key and sees only what it may see.

use undiluted_client::{Client, ClientError, Party, Transport};
use undiluted_domain::{
    CompanyId, HolderId, InstrumentId, MfnOffer, Money, Ownership, Ratio, Safe, SafeTerms, Shares,
};
use undiluted_ledger::{Event, SettlementReference};
use undiluted_service::{Actor, Command, Response, Statement};

const MICROS_PER_DOLLAR: u128 = 1_000_000;
const RATIO_PER_BASIS_POINT: u128 = 100_000_000_000_000;
const MAX_ATTEMPTS: u32 = 4;

const FOUNDER_SHARES: u128 = 8_000_000;
const ANA_INVESTMENT: u128 = 250_000;
const BEN_INVESTMENT: u128 = 500_000;
const BEN_POST_MONEY_CAP: u128 = 5_000_000;
const DEMO_SETTLEMENT: &str = "demo-settlement";

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

struct Demo<T> {
    client: Client<T>,
    company: Party,
    founder: Member,
    ana: Member,
    ben: Member,
}

pub fn run<T: Transport>(transport: T) -> Result<(), ClientError> {
    let mut demo = Demo::connect(transport)?;
    demo.incorporate()?;
    demo.raise()?;
    let offer = demo.discover_better_terms()?;
    demo.exercise_mfn(offer)?;
    demo.check_privacy()?;
    demo.show_commitment();
    println!("\nDone. Every reply was signed by the enclave and checked against its key.");
    Ok(())
}

impl<T: Transport> Demo<T> {
    fn connect(transport: T) -> Result<Self, ClientError> {
        let company = Party::generate()?;
        let suffix = hex::encode(&company.public_key().as_bytes()[..4]);
        let client = Client::connect(
            transport,
            CompanyId::new(format!("demo-{suffix}"))?,
            MAX_ATTEMPTS,
        )?;
        println!(
            "Connected to the enclave. Signing key: {}",
            hex::encode(client.identity().signing_key.as_bytes())
        );
        Ok(Self {
            client,
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

    fn show_commitment(&self) {
        if let Some(anchor) = self.client.latest_anchor() {
            step("Anyone may publish this commitment on chain. It reveals nothing about the table");
            println!("  company  {}", anchor.company);
            println!("  sequence {}", anchor.sequence);
            println!("  head     {}", anchor.head);
        }
    }

    fn commit(&mut self, event: Event, holder: &Party, actor: Actor) -> Result<(), ClientError> {
        let company = self.company.clone();
        let response = self.client.commit(
            (&company, Actor::Company),
            event,
            &[(&company, Actor::Company), (holder, actor)],
        )?;
        expect(matches!(response, Response::Committed { .. }), &response)
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
