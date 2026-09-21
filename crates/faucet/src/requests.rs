use miden_client::account::AccountId;
use miden_client::note::NoteId;
use miden_client::transaction::TransactionId;
use serde::{Deserialize, Serialize};
use tokio::sync::{mpsc, oneshot};

use crate::types::AssetAmount;

pub type MintResponseSender = oneshot::Sender<Result<MintResponse, MintError>>;
pub type MintRequestSender = mpsc::Sender<(MintRequest, MintResponseSender)>;

// QUERY PARAMETERS
// ================================================================================================

/// Query parameters for the `/pow` endpoint.
///
/// Used by both the client (to serialize) and the server (to deserialize).
#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct PowQueryParams {
    pub account_id: String,
    pub amount: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub api_key: Option<String>,
}

/// Query parameters for the `/get_tokens` endpoint.
///
/// Used by both the client (to serialize) and the server (to deserialize).
#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct GetTokensQueryParams {
    pub account_id: String,
    pub asset_amount: u64,
    /// Only public notes are supported. The faucet rejects the request when this is set.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub is_private_note: Option<bool>,
    pub challenge: String,
    pub nonce: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub api_key: Option<String>,
}

// RESPONSES
// ================================================================================================

/// Response from the `/pow` endpoint.
#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct GetPowResponse {
    pub challenge: String,
    pub target: u64,
    pub timestamp: u64,
}

/// Response from the `/get_tokens` endpoint.
#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct GetTokensResponse {
    pub tx_id: String,
    pub note_id: String,
}

/// A request for minting to the Faucet.
pub struct MintRequest {
    /// Destination account.
    pub account_id: AccountId,
    /// The amount to mint.
    pub asset_amount: AssetAmount,
}

#[derive(Debug, Clone)]
pub struct MintResponse {
    pub tx_id: TransactionId,
    pub note_id: NoteId,
}

#[derive(Debug, Clone, thiserror::Error)]
pub enum MintError {
    #[error("faucet supply exceeded")]
    AvailableSupplyExceeded,
}
