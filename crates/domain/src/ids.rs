use core::fmt;

use borsh::{BorshDeserialize, BorshSerialize};

use crate::error::CapTableError;

macro_rules! identifier {
    ($(#[$meta:meta])* $name:ident) => {
        $(#[$meta])*
        #[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, BorshSerialize, BorshDeserialize)]
        pub struct $name(String);

        impl $name {
            pub fn new(value: impl Into<String>) -> Result<Self, CapTableError> {
                let value = value.into();
                if value.trim().is_empty() {
                    return Err(CapTableError::EmptyIdentifier);
                }
                Ok(Self(value))
            }

            #[must_use]
            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(&self.0)
            }
        }
    };
}

identifier!(
    /// The company a cap table belongs to.
    CompanyId
);
identifier!(
    /// A party that can hold instruments, independent of how it pays or signs.
    HolderId
);
identifier!(
    /// A single issued instrument, unique within one company.
    InstrumentId
);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_blank_identifiers() {
        assert_eq!(HolderId::new("  "), Err(CapTableError::EmptyIdentifier));
        assert_eq!(InstrumentId::new(""), Err(CapTableError::EmptyIdentifier));
    }
}
