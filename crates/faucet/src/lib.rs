use miden_client::account::AccountId;
use miden_client::address::{Address, NetworkId};

pub mod requests;
pub mod types;

// FAUCET ID
// ================================================================================================

/// The faucet's account ID and network ID.
///
/// Used as a type safety mechanism to avoid confusion with user account IDs, and allows us to
/// implement traits.
#[derive(Clone)]
pub struct FaucetId {
    pub account_id: AccountId,
    pub network_id: NetworkId,
}

impl FaucetId {
    pub fn new(account_id: AccountId, network_id: NetworkId) -> Self {
        Self { account_id, network_id }
    }

    pub fn to_bech32(&self) -> String {
        Address::new(self.account_id).encode(self.network_id.clone())
    }
}
