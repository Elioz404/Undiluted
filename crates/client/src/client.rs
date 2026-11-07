use borsh::{from_slice, to_vec};
use undiluted_domain::CompanyId;
use undiluted_envelope::seal_request;
use undiluted_ledger::{Digest, Event};
use undiluted_service::wire::AnchorOutput;
use undiluted_service::{Actor, Command, ProposalView, Reply, Response, SignedRequest, Statement};

use crate::error::ClientError;
use crate::party::{Party, random_bytes};
use crate::transport::{EnclaveIdentity, Transport};

/// Talks to one company's ledger inside one enclave.
#[derive(Debug)]
pub struct Client<T> {
    transport: T,
    identity: EnclaveIdentity,
    company: CompanyId,
    max_attempts: u32,
    latest_anchor: Option<AnchorOutput>,
}

impl<T: Transport> Client<T> {
    /// Fetches the enclave's keys. Pinning them here means every later reply
    /// must come from this same enclave.
    pub fn connect(
        mut transport: T,
        company: CompanyId,
        max_attempts: u32,
    ) -> Result<Self, ClientError> {
        let identity = retrying(max_attempts, || transport.identity())?;
        Ok(Self {
            transport,
            identity,
            company,
            max_attempts,
            latest_anchor: None,
        })
    }

    #[must_use]
    pub const fn identity(&self) -> &EnclaveIdentity {
        &self.identity
    }

    /// The anchor from the most recent commit, ready to publish on chain.
    #[must_use]
    pub const fn latest_anchor(&self) -> Option<&AnchorOutput> {
        self.latest_anchor.as_ref()
    }

    /// Signs a request and sends it, returning the verified response.
    ///
    /// A call that fails on the way may still have run, so retries resend the
    /// same signed request: the enclave answers with its stored reply rather
    /// than acting twice. Only the encryption is fresh each time.
    pub fn send(
        &mut self,
        party: &Party,
        actor: Actor,
        command: Command,
    ) -> Result<Response, ClientError> {
        let id = random_bytes()?;
        let digest = SignedRequest::signing_digest(&id, &self.company, &actor, &command)?;
        let request = SignedRequest {
            id,
            company: self.company.clone(),
            actor,
            command,
            signature: party.sign(&digest),
        };
        let encoded = to_vec(&request).map_err(|_| ClientError::Encoding)?;

        let max_attempts = self.max_attempts;
        retrying(max_attempts, || self.exchange(&encoded, &digest))
    }

    fn exchange(&mut self, encoded: &[u8], request: &Digest) -> Result<Response, ClientError> {
        let (envelope, reply_key) =
            seal_request(&self.identity.encryption_key, random_bytes()?, encoded)?;
        let exchange = self.transport.submit(&envelope)?;
        let reply: Reply = from_slice(&reply_key.open(&exchange.envelope)?)
            .map_err(|_| ClientError::Protocol("reply could not be decoded".to_owned()))?;

        if !reply.is_authentic(&self.identity.signing_key) {
            return Err(ClientError::UnauthenticReply);
        }
        if &reply.request != request {
            return Err(ClientError::MismatchedReply);
        }
        if exchange.anchor.is_some() {
            self.latest_anchor = exchange.anchor;
        }
        Ok(reply.response)
    }

    /// Proposes an event as `proposer` and has every party sign it.
    pub fn commit(
        &mut self,
        proposer: (&Party, Actor),
        event: Event,
        signers: &[(&Party, Actor)],
    ) -> Result<Response, ClientError> {
        let view = self.propose(proposer.0, proposer.1, event)?;
        let mut last = Response::Proposal(view.clone());
        for (party, actor) in signers {
            last = self.send(
                party,
                actor.clone(),
                Command::Sign {
                    digest: view.digest,
                    signature: party.sign(&view.digest),
                },
            )?;
            if let Response::Rejected { reason } = &last {
                return Err(ClientError::Enclave(reason.clone()));
            }
        }
        Ok(last)
    }

    pub fn propose(
        &mut self,
        party: &Party,
        actor: Actor,
        event: Event,
    ) -> Result<ProposalView, ClientError> {
        match self.send(party, actor, Command::Propose { event })? {
            Response::Proposal(view) => Ok(view),
            Response::Rejected { reason } => Err(ClientError::Enclave(reason)),
            other => Err(ClientError::Protocol(format!(
                "unexpected answer: {other:?}"
            ))),
        }
    }

    pub fn statement(&mut self, party: &Party, actor: Actor) -> Result<Statement, ClientError> {
        match self.send(party, actor, Command::GetStatement)? {
            Response::Statement(statement) => Ok(statement),
            Response::Rejected { reason } => Err(ClientError::Enclave(reason)),
            other => Err(ClientError::Protocol(format!(
                "unexpected answer: {other:?}"
            ))),
        }
    }
}

/// Repeats `call` only while the failure happened on the way to the enclave.
fn retrying<R>(
    max_attempts: u32,
    mut call: impl FnMut() -> Result<R, ClientError>,
) -> Result<R, ClientError> {
    let mut attempts = 0;
    loop {
        attempts += 1;
        match call() {
            Err(ClientError::Transport(_)) if attempts < max_attempts => {}
            result => return result,
        }
    }
}
