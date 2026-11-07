use borsh::{BorshDeserialize, BorshSerialize, to_vec};
use undiluted_domain::{CompanyId, HolderId};

use crate::crypto::{Digest, PublicKey, SignatureBytes};
use crate::error::LedgerError;
use crate::event::Event;

const GENESIS_TAG: &str = "undiluted/ledger/genesis/v1";
const ENTRY_TAG: &str = "undiluted/ledger/entry/v1";

#[derive(Debug, Clone, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct HolderSignature {
    pub holder: HolderId,
    pub signature: SignatureBytes,
}

/// An event ready to be signed: the digest is what every party signs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Draft {
    pub sequence: u64,
    pub previous: Digest,
    pub event: Event,
    pub digest: Digest,
    pub company_must_sign: bool,
    /// Holders who must co-sign.
    pub required_holders: Vec<HolderId>,
}

impl Draft {
    #[must_use]
    pub fn seal(
        self,
        company_signature: Option<SignatureBytes>,
        holder_signatures: Vec<HolderSignature>,
    ) -> Entry {
        Entry {
            sequence: self.sequence,
            previous: self.previous,
            event: self.event,
            company_signature,
            holder_signatures,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct Entry {
    pub sequence: u64,
    pub previous: Digest,
    pub event: Event,
    pub company_signature: Option<SignatureBytes>,
    pub holder_signatures: Vec<HolderSignature>,
}

/// The head of a ledger with no entries yet. Binding it to the company and its
/// key means two companies can never share a history.
pub(crate) fn genesis_digest(
    company: &CompanyId,
    company_key: &PublicKey,
) -> Result<Digest, LedgerError> {
    let payload = to_vec(&(company, company_key)).map_err(|_| LedgerError::Encoding)?;
    Ok(Digest::tagged(GENESIS_TAG, &payload))
}

/// Signatures are left out so the digest can be signed by several parties.
pub(crate) fn entry_digest(
    company: &CompanyId,
    sequence: u64,
    previous: &Digest,
    event: &Event,
) -> Result<Digest, LedgerError> {
    let payload =
        to_vec(&(company, sequence, previous, event)).map_err(|_| LedgerError::Encoding)?;
    Ok(Digest::tagged(ENTRY_TAG, &payload))
}
