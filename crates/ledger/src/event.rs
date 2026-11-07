use borsh::{BorshDeserialize, BorshSerialize};
use undiluted_domain::{
    BasisPoints, CapTable, CapTableError, HolderId, InstrumentId, Money, PricedRound, Safe, Shares,
};

use crate::crypto::PublicKey;
use crate::error::LedgerError;

/// Where the money for an instrument moved, e.g. a settlement transaction.
#[derive(Debug, Clone, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct SettlementReference(String);

impl SettlementReference {
    pub fn new(value: impl Into<String>) -> Result<Self, LedgerError> {
        let value = value.into();
        if value.trim().is_empty() {
            return Err(LedgerError::EmptySettlementReference);
        }
        Ok(Self(value))
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Where investments must arrive, and in which token.
#[derive(Debug, Clone, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct Treasury {
    account: String,
    token: String,
}

impl Treasury {
    pub fn new(account: impl Into<String>, token: impl Into<String>) -> Result<Self, LedgerError> {
        let (account, token) = (account.into(), token.into());
        if account.trim().is_empty() || token.trim().is_empty() {
            return Err(LedgerError::EmptyTreasury);
        }
        Ok(Self { account, token })
    }

    #[must_use]
    pub fn account(&self) -> &str {
        &self.account
    }

    #[must_use]
    pub fn token(&self) -> &str {
        &self.token
    }
}

/// Where a holder wants to receive exit proceeds.
#[derive(Debug, Clone, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct PayoutDestination(String);

impl PayoutDestination {
    pub fn new(value: impl Into<String>) -> Result<Self, LedgerError> {
        let value = value.into();
        if value.trim().is_empty() {
            return Err(LedgerError::EmptyPayoutDestination);
        }
        Ok(Self(value))
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, Clone, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub enum Event {
    HolderRegistered {
        holder: HolderId,
        key: PublicKey,
    },
    CommonIssued {
        instrument: InstrumentId,
        holder: HolderId,
        shares: Shares,
    },
    OptionPoolResized {
        shares: Shares,
    },
    SafeIssued {
        instrument: InstrumentId,
        holder: HolderId,
        safe: Safe,
        settlement: SettlementReference,
    },
    MfnElected {
        target: InstrumentId,
        source: InstrumentId,
    },
    TokenWarrantIssued {
        instrument: InstrumentId,
        holder: HolderId,
        allocation: BasisPoints,
    },
    RoundClosed {
        round: PricedRound,
    },
    PayoutDestinationSet {
        holder: HolderId,
        destination: PayoutDestination,
    },
    ExitApproved {
        proceeds: Money,
    },
    TreasuryDesignated {
        treasury: Treasury,
    },
}

impl Event {
    /// An MFN election exercises a right the company granted when it issued the
    /// SAFE, so the company cannot withhold it. Where a holder gets paid is the
    /// holder's own business. The rules still validate both.
    #[must_use]
    pub const fn requires_company_signature(&self) -> bool {
        !matches!(
            self,
            Self::MfnElected { .. } | Self::PayoutDestinationSet { .. }
        )
    }

    /// Holders whose rights this event creates or changes, who must therefore
    /// co-sign it. Registration is handled apart because it proves a new key.
    pub(crate) fn affected_holders(
        &self,
        table: &CapTable,
    ) -> Result<Vec<HolderId>, CapTableError> {
        Ok(match self {
            Self::HolderRegistered { .. }
            | Self::OptionPoolResized { .. }
            | Self::ExitApproved { .. }
            | Self::TreasuryDesignated { .. } => Vec::new(),
            Self::CommonIssued { holder, .. }
            | Self::SafeIssued { holder, .. }
            | Self::TokenWarrantIssued { holder, .. }
            | Self::PayoutDestinationSet { holder, .. } => vec![holder.clone()],
            Self::MfnElected { target, .. } => vec![table.holding(target)?.holder().clone()],
            Self::RoundClosed { round } => {
                let mut holders: Vec<HolderId> = round
                    .investments
                    .iter()
                    .map(|investment| investment.holder.clone())
                    .collect();
                holders.sort();
                holders.dedup();
                holders
            }
        })
    }

    pub(crate) fn apply_to(&self, table: &mut CapTable) -> Result<(), CapTableError> {
        match self {
            Self::HolderRegistered { .. }
            | Self::PayoutDestinationSet { .. }
            | Self::ExitApproved { .. }
            | Self::TreasuryDesignated { .. } => Ok(()),
            Self::CommonIssued {
                instrument,
                holder,
                shares,
            } => table.issue_common(instrument.clone(), holder.clone(), *shares),
            Self::OptionPoolResized { shares } => {
                table.set_option_pool(*shares);
                Ok(())
            }
            Self::SafeIssued {
                instrument,
                holder,
                safe,
                ..
            } => table.issue_safe(instrument.clone(), holder.clone(), *safe),
            Self::MfnElected { target, source } => table.elect_mfn(target, source),
            Self::TokenWarrantIssued {
                instrument,
                holder,
                allocation,
            } => table.issue_token_warrant(instrument.clone(), holder.clone(), *allocation),
            Self::RoundClosed { round } => table.close_round(round).map(|_| ()),
        }
    }
}
