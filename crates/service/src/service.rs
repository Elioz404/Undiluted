use borsh::{BorshDeserialize, BorshSerialize, from_slice, to_vec};
use undiluted_domain::{CompanyId, HolderId};
use undiluted_ledger::{
    Digest, Draft, Entry, Event, HolderSignature, Ledger, LedgerConfig, PublicKey, SignatureBytes,
};

use crate::error::ServiceError;
use crate::ports::{EnclaveSigner, Store};
use crate::protocol::{
    Actor, Command, CompanyView, HeadAttestation, ProposalView, Reply, Response, SignedRequest,
    Statement,
};

/// An event collecting signatures. Only one can be open per company because
/// its digest commits to the head it was drafted on.
#[derive(Debug, Clone, BorshSerialize, BorshDeserialize)]
struct Proposal {
    proposer: Actor,
    sequence: u64,
    previous: Digest,
    event: Event,
    digest: Digest,
    company_signature: Option<SignatureBytes>,
    holder_signatures: Vec<HolderSignature>,
}

impl Proposal {
    fn into_entry(self) -> Entry {
        Entry {
            sequence: self.sequence,
            previous: self.previous,
            event: self.event,
            company_signature: self.company_signature,
            holder_signatures: self.holder_signatures,
        }
    }
}

/// Handles authenticated requests. Every run rebuilds the ledger from storage,
/// which re-verifies every signature and link before anything is trusted.
#[derive(Debug)]
pub struct Service<S, K> {
    store: S,
    signer: K,
}

impl<S: Store, K: EnclaveSigner> Service<S, K> {
    pub const fn new(store: S, signer: K) -> Self {
        Self { store, signer }
    }

    pub(crate) fn signing_key(&self) -> Result<PublicKey, crate::ports::StoreError> {
        self.signer.public_key()
    }

    /// Answers a request. A refused request is still a signed reply, so a client
    /// can tell "you may not" from a failure on the way to the enclave.
    pub fn handle(&mut self, request: &SignedRequest) -> Result<Reply, ServiceError> {
        let digest = request.digest()?;
        let receipt_key = receipt_key(&digest);

        if request.command.is_mutation()
            && let Some(stored) = self.store.get(&receipt_key)?
        {
            return from_slice(&stored).map_err(|_| ServiceError::CorruptData);
        }

        let (response, committed) = match self.dispatch(request) {
            Ok(outcome) => outcome,
            Err(ServiceError::Store(error)) => return Err(ServiceError::Store(error)),
            Err(error) => (
                Response::Rejected {
                    reason: error.to_string(),
                },
                None,
            ),
        };

        let anchor = committed
            .map(|(sequence, head)| self.attest_head(&request.company, sequence, head))
            .transpose()?;
        let reply = self.sign_reply(digest, response, anchor)?;

        if request.command.is_mutation() && !matches!(reply.response, Response::Rejected { .. }) {
            let encoded = to_vec(&reply).map_err(|_| ServiceError::Encoding)?;
            self.store.put(&receipt_key, &encoded)?;
        }
        Ok(reply)
    }

    fn dispatch(
        &mut self,
        request: &SignedRequest,
    ) -> Result<(Response, Option<(u64, Digest)>), ServiceError> {
        if let Command::CreateCompany { key } = &request.command {
            return self.create_company(request, *key).map(|r| (r, None));
        }

        let ledger = self.load_ledger(&request.company)?;
        let proposal = self.load_proposal(&request.company)?;
        Self::authenticate(request, &ledger, proposal.as_ref())?;

        match &request.command {
            Command::CreateCompany { .. } => Err(ServiceError::CompanyExists),
            Command::Propose { event } => self
                .propose(request, &ledger, proposal.as_ref(), event)
                .map(|r| (r, None)),
            Command::Sign { digest, signature } => {
                self.sign_proposal(request, &ledger, proposal, *digest, *signature)
            }
            Command::CancelProposal => self.cancel(request, proposal).map(|r| (r, None)),
            Command::GetProposal => {
                Self::read_proposal(request, &ledger, proposal).map(|r| (r, None))
            }
            Command::GetCapTable => Self::read_table(request, &ledger).map(|r| (r, None)),
            Command::GetStatement => Self::read_statement(request, &ledger).map(|r| (r, None)),
        }
    }

    fn create_company(
        &mut self,
        request: &SignedRequest,
        key: PublicKey,
    ) -> Result<Response, ServiceError> {
        if !key.verifies(&request.digest()?, &request.signature) {
            return Err(ServiceError::InvalidSignature);
        }
        let config = LedgerConfig {
            company: request.company.clone(),
            company_key: key,
        };
        let ledger = Ledger::new(config)?;
        let encoded = to_vec(&key).map_err(|_| ServiceError::Encoding)?;
        if !self
            .store
            .put_new(&company_key(&request.company), &encoded)?
        {
            return Err(ServiceError::CompanyExists);
        }
        Ok(Response::CompanyCreated {
            head: ledger.head(),
        })
    }

    fn authenticate(
        request: &SignedRequest,
        ledger: &Ledger,
        proposal: Option<&Proposal>,
    ) -> Result<(), ServiceError> {
        let key = match &request.actor {
            Actor::Company => Some(ledger.config().company_key),
            Actor::Holder(holder) => ledger.holder_key(holder).or_else(|| {
                // Someone being invited has a key in the open proposal but is
                // not registered until that proposal commits.
                proposal.and_then(|open| match &open.event {
                    Event::HolderRegistered {
                        holder: invited,
                        key,
                    } if invited == holder => Some(*key),
                    _ => None,
                })
            }),
        };
        let digest = request.digest()?;
        if key.is_some_and(|key| key.verifies(&digest, &request.signature)) {
            Ok(())
        } else {
            Err(ServiceError::InvalidSignature)
        }
    }

    fn propose(
        &mut self,
        request: &SignedRequest,
        ledger: &Ledger,
        open: Option<&Proposal>,
        event: &Event,
    ) -> Result<Response, ServiceError> {
        if open.is_some() {
            return Err(ServiceError::ProposalPending);
        }
        let draft = ledger.draft(event.clone())?;
        Self::ensure_may_propose(&request.actor, &draft)?;

        let proposal = Proposal {
            proposer: request.actor.clone(),
            sequence: draft.sequence,
            previous: draft.previous,
            event: draft.event,
            digest: draft.digest,
            company_signature: None,
            holder_signatures: Vec::new(),
        };
        let view = Self::view(ledger, &proposal)?;
        self.save_proposal(&request.company, &proposal)?;
        Ok(Response::Proposal(view))
    }

    /// The company proposes anything; a holder only an MFN election of their own.
    fn ensure_may_propose(actor: &Actor, draft: &Draft) -> Result<(), ServiceError> {
        match actor {
            Actor::Company => Ok(()),
            Actor::Holder(holder)
                if matches!(draft.event, Event::MfnElected { .. })
                    && draft.required_holders.contains(holder) =>
            {
                Ok(())
            }
            Actor::Holder(_) => Err(ServiceError::Forbidden),
        }
    }

    fn sign_proposal(
        &mut self,
        request: &SignedRequest,
        ledger: &Ledger,
        proposal: Option<Proposal>,
        digest: Digest,
        signature: SignatureBytes,
    ) -> Result<(Response, Option<(u64, Digest)>), ServiceError> {
        let mut proposal = proposal.ok_or(ServiceError::NoProposal)?;
        if proposal.digest != digest {
            return Err(ServiceError::WrongProposal);
        }

        let signers = ledger.required_signers(&proposal.event)?;
        match &request.actor {
            Actor::Company => {
                if !ledger.config().company_key.verifies(&digest, &signature) {
                    return Err(ServiceError::InvalidSignature);
                }
                proposal.company_signature = Some(signature);
            }
            Actor::Holder(holder) => {
                let (_, key) = signers
                    .iter()
                    .find(|(signer, _)| signer == holder)
                    .ok_or(ServiceError::Forbidden)?;
                if !key.verifies(&digest, &signature) {
                    return Err(ServiceError::InvalidSignature);
                }
                proposal.holder_signatures.retain(|s| &s.holder != holder);
                proposal.holder_signatures.push(HolderSignature {
                    holder: holder.clone(),
                    signature,
                });
            }
        }

        let view = Self::view(ledger, &proposal)?;
        if view.company_signature_missing || !view.holder_signatures_missing.is_empty() {
            self.save_proposal(&request.company, &proposal)?;
            return Ok((Response::Proposal(view), None));
        }
        self.commit(&request.company, ledger.clone(), proposal)
    }

    fn commit(
        &mut self,
        company: &CompanyId,
        mut ledger: Ledger,
        proposal: Proposal,
    ) -> Result<(Response, Option<(u64, Digest)>), ServiceError> {
        let sequence = proposal.sequence;
        let entry = proposal.into_entry();
        let head = ledger.append(entry.clone())?;

        let encoded = to_vec(&entry).map_err(|_| ServiceError::Encoding)?;
        if !self
            .store
            .put_new(&entry_key(company, sequence), &encoded)?
        {
            return Err(ServiceError::Conflict);
        }
        self.store.delete(&proposal_key(company))?;
        Ok((
            Response::Committed { sequence, head },
            Some((sequence, head)),
        ))
    }

    fn cancel(
        &mut self,
        request: &SignedRequest,
        proposal: Option<Proposal>,
    ) -> Result<Response, ServiceError> {
        let proposal = proposal.ok_or(ServiceError::NoProposal)?;
        if request.actor != Actor::Company && request.actor != proposal.proposer {
            return Err(ServiceError::Forbidden);
        }
        self.store.delete(&proposal_key(&request.company))?;
        Ok(Response::Cancelled)
    }

    fn read_proposal(
        request: &SignedRequest,
        ledger: &Ledger,
        proposal: Option<Proposal>,
    ) -> Result<Response, ServiceError> {
        let proposal = proposal.ok_or(ServiceError::NoProposal)?;
        if let Actor::Holder(holder) = &request.actor {
            let is_signer = ledger
                .required_signers(&proposal.event)?
                .iter()
                .any(|(signer, _)| signer == holder);
            if !is_signer {
                return Err(ServiceError::Forbidden);
            }
        }
        Self::view(ledger, &proposal).map(Response::Proposal)
    }

    fn read_table(request: &SignedRequest, ledger: &Ledger) -> Result<Response, ServiceError> {
        if request.actor != Actor::Company {
            return Err(ServiceError::Forbidden);
        }
        Ok(Response::CapTable(CompanyView {
            head: ledger.head(),
            entries: ledger.entries().len() as u64,
            pro_forma: ledger.table().pro_forma()?,
        }))
    }

    fn read_statement(request: &SignedRequest, ledger: &Ledger) -> Result<Response, ServiceError> {
        let Actor::Holder(holder) = &request.actor else {
            return Err(ServiceError::StatementNeedsHolder);
        };
        let pro_forma = ledger.table().pro_forma()?;
        Ok(Response::Statement(Statement {
            head: ledger.head(),
            entries: ledger.entries().len() as u64,
            capitalization: pro_forma.capitalization,
            reconciles: pro_forma.reconciles()?,
            lines: pro_forma.lines_of(holder).cloned().collect(),
            mfn_offers: ledger.table().mfn_offers(holder),
        }))
    }

    fn view(ledger: &Ledger, proposal: &Proposal) -> Result<ProposalView, ServiceError> {
        let holder_signatures_missing: Vec<HolderId> = ledger
            .required_signers(&proposal.event)?
            .into_iter()
            .map(|(holder, _)| holder)
            .filter(|holder| {
                !proposal
                    .holder_signatures
                    .iter()
                    .any(|s| &s.holder == holder)
            })
            .collect();
        Ok(ProposalView {
            digest: proposal.digest,
            sequence: proposal.sequence,
            event: proposal.event.clone(),
            company_signature_missing: proposal.event.requires_company_signature()
                && proposal.company_signature.is_none(),
            holder_signatures_missing,
        })
    }

    fn load_ledger(&self, company: &CompanyId) -> Result<Ledger, ServiceError> {
        let encoded = self
            .store
            .get(&company_key(company))?
            .ok_or(ServiceError::UnknownCompany)?;
        let company_key: PublicKey = from_slice(&encoded).map_err(|_| ServiceError::CorruptData)?;

        let mut ledger = Ledger::new(LedgerConfig {
            company: company.clone(),
            company_key,
        })?;
        let mut sequence = 0;
        while let Some(encoded) = self.store.get(&entry_key(company, sequence))? {
            let entry: Entry = from_slice(&encoded).map_err(|_| ServiceError::CorruptData)?;
            ledger.append(entry)?;
            sequence += 1;
        }
        Ok(ledger)
    }

    fn load_proposal(&self, company: &CompanyId) -> Result<Option<Proposal>, ServiceError> {
        self.store
            .get(&proposal_key(company))?
            .map(|encoded| from_slice(&encoded).map_err(|_| ServiceError::CorruptData))
            .transpose()
    }

    fn save_proposal(
        &mut self,
        company: &CompanyId,
        proposal: &Proposal,
    ) -> Result<(), ServiceError> {
        let encoded = to_vec(proposal).map_err(|_| ServiceError::Encoding)?;
        self.store.put(&proposal_key(company), &encoded)?;
        Ok(())
    }

    fn attest_head(
        &self,
        company: &CompanyId,
        sequence: u64,
        head: Digest,
    ) -> Result<HeadAttestation, ServiceError> {
        let digest = HeadAttestation::signing_digest(company, sequence, &head)?;
        Ok(HeadAttestation {
            company: company.clone(),
            sequence,
            head,
            signature: self.signer.sign(digest.as_bytes())?,
        })
    }

    fn sign_reply(
        &self,
        request: Digest,
        response: Response,
        anchor: Option<HeadAttestation>,
    ) -> Result<Reply, ServiceError> {
        let digest = Reply::signing_digest(&request, &response, anchor.as_ref())?;
        Ok(Reply {
            request,
            response,
            anchor,
            signature: self.signer.sign(digest.as_bytes())?,
        })
    }
}

fn company_key(company: &CompanyId) -> String {
    format!("company/{company}/key")
}

fn entry_key(company: &CompanyId, sequence: u64) -> String {
    format!("company/{company}/entry/{sequence}")
}

fn proposal_key(company: &CompanyId) -> String {
    format!("company/{company}/proposal")
}

fn receipt_key(request: &Digest) -> String {
    format!("receipt/{request:?}")
}
