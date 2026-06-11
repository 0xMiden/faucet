//! A collection of new types and safety wrappers used throughout the faucet.

use miden_client::asset::FungibleAsset;

/// Represents a valid asset amount for a [`FungibleAsset`].
///
/// A [`FungibleAsset`] has a maximum representable amount
/// and this type guarantees that its value is within this range.
#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Default)]
pub struct AssetAmount(u64);

impl std::fmt::Display for AssetAmount {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl AssetAmount {
    /// The absolute maximum asset amount allowed by the network.
    pub const MAX: u64 = FungibleAsset::MAX_AMOUNT.as_u64();

    /// Creates an [`AssetAmount`] from a base unit amount.
    ///
    /// Returns an error if the amount is greater than the maximum allowed amount.
    pub fn new(base_units: u64) -> Result<Self, AssetAmountError> {
        if base_units > Self::MAX {
            return Err(AssetAmountError::AssetAmountTooBig(base_units));
        }

        Ok(Self(base_units))
    }

    /// Creates an [`AssetAmount`] with the maximum allowed amount ([`FungibleAsset::MAX_AMOUNT`]).
    pub fn max() -> Self {
        Self(Self::MAX)
    }

    /// Returns the asset amount in base units.
    pub fn base_units(&self) -> u64 {
        self.0
    }

    /// Adds another [`AssetAmount`] to the current one and returns the result if it is valid.
    pub fn checked_add(self, other: Self) -> Option<Self> {
        Self::new(self.0.checked_add(other.0)?).ok()
    }

    /// Subtracts another [`AssetAmount`] from the current one and returns the result if it is
    /// valid.
    pub fn checked_sub(self, other: Self) -> Option<Self> {
        Self::new(self.0.checked_sub(other.0)?).ok()
    }
}

#[derive(Debug, thiserror::Error)]
pub enum AssetAmountError {
    #[error(
        "fungible asset amount {0} exceeds the max allowed amount of {max_amount}",
        max_amount = FungibleAsset::MAX_AMOUNT
      )]
    AssetAmountTooBig(u64),
}

/// Type of note to generate for a mint request.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum NoteType {
    Private,
    Public,
}

impl From<NoteType> for miden_client::note::NoteType {
    fn from(value: NoteType) -> Self {
        match value {
            NoteType::Private => Self::Private,
            NoteType::Public => Self::Public,
        }
    }
}

impl std::fmt::Display for NoteType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Private => f.write_str("private"),
            Self::Public => f.write_str("public"),
        }
    }
}
