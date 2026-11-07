//! Messages exchanged with the enclave, independent of how they travel.

use borsh::{BorshDeserialize, BorshSerialize, to_vec};
use undiluted_domain::{CompanyId, HolderId, MfnOffer, Money, Payout, ProForma};
use undiluted_ledger::{Digest, Event, PublicKey, SignatureBytes};

use crate::error::ServiceError;

const REQUEST_TAG: &str = "undiluted/service/request/v1";
const REPLY_TAG: &str = "undiluted/service/reply/v1";
const ANCHOR_TAG: &str = "undiluted/service/anchor/v1";
const PAYOUT_TAG: &str = "undiluted/service/payout/v1";

pub type RequestId = [u8; 16];

#[derive(Debug, Clone, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub enum Actor {
    Company,
    Holder(HolderId),
}

#[derive(Debug, Clone, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub enum Command {
    CreateCompany {
        key: PublicKey,
    },
    Propose {
        event: Event,
    },
    Sign {
        digest: Digest,
        signature: SignatureBytes,
    },
    CancelProposal,
    GetProposal,
    GetCapTable,
    GetStatement,
    GetPayoutPlan,
}

impl Command {
    /// Whether running it twice would change state twice. Only these commands
    /// have their replies stored so a retry gets the first answer back.
    #[must_use]
    pub const fn is_mutation(&self) -> bool {
        matches!(
            self,
            Self::CreateCompany { .. }
                | Self::Propose { .. }
                | Self::Sign { .. }
                | Self::CancelProposal
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct SignedRequest {
    pub id: RequestId,
    pub company: CompanyId,
    pub actor: Actor,
    pub command: Command,
    pub signature: SignatureBytes,
}

impl SignedRequest {
    /// What the actor signs. The random id makes each request unique.
    pub fn signing_digest(
        id: &RequestId,
        company: &CompanyId,
        actor: &Actor,
        command: &Command,
    ) -> Result<Digest, ServiceError> {
        let payload = to_vec(&(id, company, actor, command)).map_err(|_| ServiceError::Encoding)?;
        Ok(Digest::tagged(REQUEST_TAG, &payload))
    }

    pub fn digest(&self) -> Result<Digest, ServiceError> {
        Self::signing_digest(&self.id, &self.company, &self.actor, &self.command)
    }
}

/// A proposal as its signers see it.
#[derive(Debug, Clone, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct ProposalView {
    pub digest: Digest,
    pub sequence: u64,
    pub event: Event,
    pub company_signature_missing: bool,
    pub holder_signatures_missing: Vec<HolderId>,
}

/// The whole table, for the company only.
#[derive(Debug, Clone, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct CompanyView {
    pub head: Digest,
    pub entries: u64,
    pub holders: Vec<undiluted_domain::HolderId>,
    pub pro_forma: ProForma,
}

/// One holder's view: their own lines and proof the total is consistent,
/// without any other holder's identity, amount or terms.
#[derive(Debug, Clone, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct Statement {
    pub head: Digest,
    pub entries: u64,
    pub capitalization: undiluted_domain::Shares,
    pub reconciles: bool,
    pub lines: Vec<undiluted_domain::OwnershipLine>,
    pub mfn_offers: Vec<MfnOffer>,
    pub exit: Option<HolderExit>,
}

/// A holder's share of an approved exit, without anyone else's.
#[derive(Debug, Clone, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct HolderExit {
    pub proceeds: Money,
    /// Whether all payouts together add up to exactly the proceeds.
    pub total_matches_proceeds: bool,
    pub payouts: Vec<Payout>,
}

/// One payment the distribution contract may make, signed by the enclave.
#[derive(Debug, Clone, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct PayoutInstruction {
    pub index: u32,
    pub recipient: String,
    pub amount: Money,
    pub signature: SignatureBytes,
}

impl PayoutInstruction {
    pub fn signing_digest(
        company: &CompanyId,
        index: u32,
        recipient: &str,
        amount: Money,
    ) -> Result<Digest, ServiceError> {
        let payload =
            to_vec(&(company, index, recipient, amount)).map_err(|_| ServiceError::Encoding)?;
        Ok(Digest::tagged(PAYOUT_TAG, &payload))
    }
}

/// Every payment of an exit, for the company to relay.
#[derive(Debug, Clone, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct PayoutPlan {
    pub proceeds: Money,
    pub instructions: Vec<PayoutInstruction>,
}

#[derive(Debug, Clone, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub enum Response {
    CompanyCreated {
        head: Digest,
    },
    Proposal(ProposalView),
    /// The last signature arrived and the event is now part of the ledger.
    Committed {
        sequence: u64,
        head: Digest,
    },
    Cancelled,
    CapTable(CompanyView),
    Statement(Statement),
    PayoutPlan(PayoutPlan),
    Rejected {
        reason: String,
    },
}

/// A commitment the enclave signs so a head can be anchored publicly. It
/// carries no private data: the anchor contract stores the same fields.
#[derive(Debug, Clone, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct HeadAttestation {
    pub company: CompanyId,
    pub sequence: u64,
    pub head: Digest,
    pub signature: SignatureBytes,
}

impl HeadAttestation {
    pub fn signing_digest(
        company: &CompanyId,
        sequence: u64,
        head: &Digest,
    ) -> Result<Digest, ServiceError> {
        let payload = to_vec(&(company, sequence, head)).map_err(|_| ServiceError::Encoding)?;
        Ok(Digest::tagged(ANCHOR_TAG, &payload))
    }
}

/// A response signed by the enclave and bound to the request it answers, so an
/// old reply cannot be passed off as the answer to a new question.
#[derive(Debug, Clone, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct Reply {
    pub request: Digest,
    pub response: Response,
    pub anchor: Option<HeadAttestation>,
    pub signature: SignatureBytes,
}

impl Reply {
    pub fn signing_digest(
        request: &Digest,
        response: &Response,
        anchor: Option<&HeadAttestation>,
    ) -> Result<Digest, ServiceError> {
        let payload = to_vec(&(request, response, anchor)).map_err(|_| ServiceError::Encoding)?;
        Ok(Digest::tagged(REPLY_TAG, &payload))
    }

    /// Whether this reply really came from the enclave holding `enclave_key`.
    #[must_use]
    pub fn is_authentic(&self, enclave_key: &PublicKey) -> bool {
        Self::signing_digest(&self.request, &self.response, self.anchor.as_ref())
            .is_ok_and(|digest| enclave_key.verifies(&digest, &self.signature))
    }
}
