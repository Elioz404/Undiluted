//! JSON shapes for the browser. They keep the protocol's own types out of the
//! interface, so either side can change without breaking the other. Amounts are
//! strings because JavaScript numbers cannot hold them exactly.

use serde::{Deserialize, Serialize};
use undiluted_domain::{
    BasisPoints, DiscountRate, HolderId, InstrumentId, MfnOffer, Money, Ownership, OwnershipLine,
    Payout, PayoutBasis, Safe, SafeTerms, Shares,
};
use undiluted_ledger::{
    Digest, Event, PayoutDestination, PublicKey, SettlementReference, Treasury,
};
use undiluted_service::{Actor, ProposalView, Response, Statement};

use crate::error::WebError;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "role", rename_all = "snake_case")]
pub enum ActorDto {
    Company,
    Holder { holder: String },
}

impl ActorDto {
    pub(crate) fn into_actor(self) -> Result<Actor, WebError> {
        Ok(match self {
            Self::Company => Actor::Company,
            Self::Holder { holder } => Actor::Holder(HolderId::new(holder)?),
        })
    }
}

/// A command. `Sign` is signed with the same key that signs the request.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum CommandDto {
    CreateCompany,
    Propose { event: EventDto },
    Sign { digest: String },
    CancelProposal,
    GetProposal,
    GetCapTable,
    GetStatement,
    GetPayoutPlan,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum EventDto {
    TreasuryDesignated {
        account: String,
        token: String,
    },
    HolderRegistered {
        holder: String,
        key: String,
    },
    CommonIssued {
        instrument: String,
        holder: String,
        shares: String,
    },
    SafeIssued {
        instrument: String,
        holder: String,
        purchase_micros: String,
        valuation_cap_micros: Option<String>,
        discount_basis_points: Option<u16>,
        mfn: bool,
        settlement: String,
    },
    MfnElected {
        target: String,
        source: String,
    },
    PayoutDestinationSet {
        holder: String,
        destination: String,
    },
    ExitApproved {
        proceeds_micros: String,
    },
    /// Anything the browser does not need to build, shown for review only.
    Other {
        description: String,
    },
}

fn amount(value: &str) -> Result<u128, WebError> {
    value
        .parse()
        .map_err(|_| WebError::Input(format!("not an amount: {value}")))
}

pub(crate) fn decode_key(value: &str) -> Result<[u8; 32], WebError> {
    hex::decode(value)
        .ok()
        .and_then(|bytes| bytes.try_into().ok())
        .ok_or_else(|| WebError::Input("a key must be 64 hex characters".to_owned()))
}

pub(crate) fn decode_digest(value: &str) -> Result<Digest, WebError> {
    decode_key(value).map(Digest::from_bytes)
}

impl EventDto {
    pub(crate) fn into_event(self) -> Result<Event, WebError> {
        Ok(match self {
            Self::TreasuryDesignated { account, token } => Event::TreasuryDesignated {
                treasury: Treasury::new(account, token)?,
            },
            Self::HolderRegistered { holder, key } => Event::HolderRegistered {
                holder: HolderId::new(holder)?,
                key: PublicKey::from_bytes(decode_key(&key)?),
            },
            Self::CommonIssued {
                instrument,
                holder,
                shares,
            } => Event::CommonIssued {
                instrument: InstrumentId::new(instrument)?,
                holder: HolderId::new(holder)?,
                shares: Shares::new(amount(&shares)?),
            },
            Self::SafeIssued {
                instrument,
                holder,
                purchase_micros,
                valuation_cap_micros,
                discount_basis_points,
                mfn,
                settlement,
            } => {
                let cap = valuation_cap_micros
                    .map(|cap| amount(&cap).map(Money::new))
                    .transpose()?;
                let discount = discount_basis_points
                    .map(|basis_points| DiscountRate::new(BasisPoints::new(basis_points)?))
                    .transpose()?;
                Event::SafeIssued {
                    instrument: InstrumentId::new(instrument)?,
                    holder: HolderId::new(holder)?,
                    safe: Safe::new(
                        Money::new(amount(&purchase_micros)?),
                        SafeTerms::new(cap, discount, mfn)?,
                    )?,
                    settlement: SettlementReference::new(settlement)?,
                }
            }
            Self::MfnElected { target, source } => Event::MfnElected {
                target: InstrumentId::new(target)?,
                source: InstrumentId::new(source)?,
            },
            Self::PayoutDestinationSet {
                holder,
                destination,
            } => Event::PayoutDestinationSet {
                holder: HolderId::new(holder)?,
                destination: PayoutDestination::new(destination)?,
            },
            Self::ExitApproved { proceeds_micros } => Event::ExitApproved {
                proceeds: Money::new(amount(&proceeds_micros)?),
            },
            Self::Other { .. } => {
                return Err(WebError::Input(
                    "this event cannot be proposed here".to_owned(),
                ));
            }
        })
    }

    pub(crate) fn from_event(event: &Event) -> Self {
        match event {
            Event::TreasuryDesignated { treasury } => Self::TreasuryDesignated {
                account: treasury.account().to_owned(),
                token: treasury.token().to_owned(),
            },
            Event::HolderRegistered { holder, key } => Self::HolderRegistered {
                holder: holder.to_string(),
                key: hex::encode(key.as_bytes()),
            },
            Event::CommonIssued {
                instrument,
                holder,
                shares,
            } => Self::CommonIssued {
                instrument: instrument.to_string(),
                holder: holder.to_string(),
                shares: shares.value().to_string(),
            },
            Event::SafeIssued {
                instrument,
                holder,
                safe,
                settlement,
            } => Self::SafeIssued {
                instrument: instrument.to_string(),
                holder: holder.to_string(),
                purchase_micros: safe.purchase_amount().value().to_string(),
                valuation_cap_micros: safe
                    .terms()
                    .valuation_cap()
                    .map(|cap| cap.value().to_string()),
                discount_basis_points: safe.terms().discount().map(|rate| rate.discount().value()),
                mfn: safe.terms().has_mfn_clause(),
                settlement: settlement.as_str().to_owned(),
            },
            Event::MfnElected { target, source } => Self::MfnElected {
                target: target.to_string(),
                source: source.to_string(),
            },
            Event::PayoutDestinationSet {
                holder,
                destination,
            } => Self::PayoutDestinationSet {
                holder: holder.to_string(),
                destination: destination.as_str().to_owned(),
            },
            Event::ExitApproved { proceeds } => Self::ExitApproved {
                proceeds_micros: proceeds.value().to_string(),
            },
            other => Self::Other {
                description: format!("{other:?}"),
            },
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LineDto {
    pub instrument: String,
    pub holder: String,
    /// Absent until a priced round fixes an uncapped SAFE's ownership.
    pub shares: Option<String>,
    /// Ownership in basis points of a percent, e.g. 996 for 9.96%.
    pub ownership_basis_points: Option<String>,
}

impl LineDto {
    fn from_line(line: &OwnershipLine) -> Self {
        let (shares, ownership) = match line.ownership {
            Ownership::Determined { shares, ratio } => (
                Some(shares.value().to_string()),
                Some((ratio.scaled() / RATIO_PER_BASIS_POINT).to_string()),
            ),
            Ownership::PendingRound => (None, None),
        };
        Self {
            instrument: line.instrument.to_string(),
            holder: line.holder.to_string(),
            shares,
            ownership_basis_points: ownership,
        }
    }
}

const RATIO_PER_BASIS_POINT: u128 = 100_000_000_000_000;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OfferDto {
    pub target: String,
    pub source: String,
    pub valuation_cap_micros: Option<String>,
    pub discount_basis_points: Option<u16>,
}

impl OfferDto {
    fn from_offer(offer: &MfnOffer) -> Self {
        Self {
            target: offer.target.to_string(),
            source: offer.source.to_string(),
            valuation_cap_micros: offer
                .terms
                .valuation_cap()
                .map(|cap| cap.value().to_string()),
            discount_basis_points: offer.terms.discount().map(|rate| rate.discount().value()),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PayoutDto {
    pub instrument: String,
    pub holder: String,
    pub amount_micros: String,
    pub took_preference: bool,
}

impl PayoutDto {
    fn from_payout(payout: &Payout) -> Self {
        Self {
            instrument: payout.instrument.to_string(),
            holder: payout.holder.to_string(),
            amount_micros: payout.amount.value().to_string(),
            took_preference: payout.basis == PayoutBasis::Preference,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExitDto {
    pub proceeds_micros: String,
    pub total_matches_proceeds: bool,
    pub payouts: Vec<PayoutDto>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StatementDto {
    pub head: String,
    pub entries: u64,
    pub capitalization: String,
    pub reconciles: bool,
    pub lines: Vec<LineDto>,
    pub mfn_offers: Vec<OfferDto>,
    pub exit: Option<ExitDto>,
}

impl StatementDto {
    fn from_statement(statement: &Statement) -> Self {
        Self {
            head: hex::encode(statement.head.as_bytes()),
            entries: statement.entries,
            capitalization: statement.capitalization.value().to_string(),
            reconciles: statement.reconciles,
            lines: statement.lines.iter().map(LineDto::from_line).collect(),
            mfn_offers: statement
                .mfn_offers
                .iter()
                .map(OfferDto::from_offer)
                .collect(),
            exit: statement.exit.as_ref().map(|exit| ExitDto {
                proceeds_micros: exit.proceeds.value().to_string(),
                total_matches_proceeds: exit.total_matches_proceeds,
                payouts: exit.payouts.iter().map(PayoutDto::from_payout).collect(),
            }),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProposalDto {
    pub digest: String,
    pub sequence: u64,
    pub event: EventDto,
    pub company_signature_missing: bool,
    pub holder_signatures_missing: Vec<String>,
}

impl ProposalDto {
    fn from_view(view: &ProposalView) -> Self {
        Self {
            digest: hex::encode(view.digest.as_bytes()),
            sequence: view.sequence,
            event: EventDto::from_event(&view.event),
            company_signature_missing: view.company_signature_missing,
            holder_signatures_missing: view
                .holder_signatures_missing
                .iter()
                .map(ToString::to_string)
                .collect(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InstructionDto {
    pub index: u32,
    pub recipient: String,
    pub amount_micros: String,
    /// Base64, as the distribution contract takes it.
    pub signature: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ResponseDto {
    CompanyCreated {
        head: String,
    },
    Proposal(ProposalDto),
    Committed {
        sequence: u64,
        head: String,
    },
    Cancelled,
    CapTable {
        head: String,
        entries: u64,
        holders: Vec<String>,
        capitalization: String,
        option_pool: String,
        lines: Vec<LineDto>,
    },
    Statement(StatementDto),
    PayoutPlan {
        proceeds_micros: String,
        instructions: Vec<InstructionDto>,
    },
    Rejected {
        reason: String,
    },
}

impl ResponseDto {
    pub(crate) fn from_response(response: &Response) -> Self {
        use base64::Engine;
        use base64::engine::general_purpose::STANDARD;

        match response {
            Response::CompanyCreated { head } => Self::CompanyCreated {
                head: hex::encode(head.as_bytes()),
            },
            Response::Proposal(view) => Self::Proposal(ProposalDto::from_view(view)),
            Response::Committed { sequence, head } => Self::Committed {
                sequence: *sequence,
                head: hex::encode(head.as_bytes()),
            },
            Response::Cancelled => Self::Cancelled,
            Response::CapTable(view) => Self::CapTable {
                head: hex::encode(view.head.as_bytes()),
                entries: view.entries,
                holders: view.holders.iter().map(ToString::to_string).collect(),
                capitalization: view.pro_forma.capitalization.value().to_string(),
                option_pool: view.pro_forma.option_pool.value().to_string(),
                lines: view
                    .pro_forma
                    .lines
                    .iter()
                    .map(LineDto::from_line)
                    .collect(),
            },
            Response::Statement(statement) => {
                Self::Statement(StatementDto::from_statement(statement))
            }
            Response::PayoutPlan(plan) => Self::PayoutPlan {
                proceeds_micros: plan.proceeds.value().to_string(),
                instructions: plan
                    .instructions
                    .iter()
                    .map(|instruction| InstructionDto {
                        index: instruction.index,
                        recipient: instruction.recipient.clone(),
                        amount_micros: instruction.amount.value().to_string(),
                        signature: STANDARD.encode(instruction.signature.as_bytes()),
                    })
                    .collect(),
            },
            Response::Rejected { reason } => Self::Rejected {
                reason: reason.clone(),
            },
        }
    }
}
