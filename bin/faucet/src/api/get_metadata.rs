use axum::Json;
use axum::extract::State;
use miden_faucet_lib::FaucetId;
use serde::Serialize;
use tracing::{instrument, warn};
use url::Url;

use crate::COMPONENT;
use crate::api::ApiServer;
use crate::api_key::ApiKey;

/// Describes the faucet metadata needed to show on the frontend.
#[derive(Clone)]
pub struct Metadata {
    /// The funding service account the notes are sent from.
    pub funder_account_id: FaucetId,
    pub decimals: u8,
    pub explorer_url: Option<Url>,
    pub base_amount: u64,
}

// ENDPOINT
// ================================================================================================

#[instrument(parent = None, target = COMPONENT, name = "server.get_metadata", skip_all)]
pub async fn get_metadata(State(server): State<ApiServer>) -> Json<GetMetadataResponse> {
    // The balance is read per request so the page shows what is left right now. A funding service
    // that cannot be reached leaves it out rather than failing the whole page.
    let balance = match server.funding_service.status().await {
        Ok(status) => Some(status.balance),
        Err(error) => {
            warn!(target: COMPONENT, %error, "failed to read the funding service's balance");
            None
        },
    };

    let metadata = server.metadata;
    Json(GetMetadataResponse {
        version: env!("CARGO_PKG_VERSION").to_string(),
        id: metadata.funder_account_id.to_bech32(),
        decimals: metadata.decimals,
        explorer_url: metadata.explorer_url,
        pow_load_difficulty: server.rate_limiter.get_load_difficulty(ApiKey::default()),
        base_amount: metadata.base_amount,
        balance,
    })
}

#[derive(Serialize)]
pub struct GetMetadataResponse {
    pub version: String,
    pub id: String,
    pub decimals: u8,
    pub explorer_url: Option<Url>,
    pub pow_load_difficulty: u64,
    pub base_amount: u64,
    /// The funding account's remaining balance in base units, if the funding service answered.
    pub balance: Option<u64>,
}
