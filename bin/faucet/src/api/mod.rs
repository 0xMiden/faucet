use std::collections::HashSet;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::Context;
use axum::Router;
use axum::extract::FromRef;
use axum::routing::{get, post};
use http::HeaderValue;
use miden_client::account::{AccountId, AccountIdError, AddressError};
use miden_client::note_transport::grpc::GrpcNoteTransportClient;
use miden_client::store::Store;
use miden_client::utils::hex_to_bytes;
use miden_faucet_lib::requests::MintRequestSender;
use miden_faucet_lib::types::AssetAmount;
use miden_pow_rate_limiter::{Challenge, ChallengeError, PoWRateLimiter, PoWRateLimiterConfig};
use tokio::net::TcpListener;
use tokio::sync::watch;
use tower::ServiceBuilder;
use tower_http::cors::CorsLayer;
use tower_http::set_header::SetResponseHeaderLayer;
use tracing::instrument;
use url::Url;

use crate::COMPONENT;
use crate::api::events::issuance_stream;
use crate::api::get_metadata::get_metadata;
use crate::api::get_note::get_note;
use crate::api::get_pow::get_pow;
use crate::api::get_tokens::{GetTokensState, MintRequestError, get_tokens};
use crate::api::send_note::send_note;
use crate::api_key::ApiKey;

mod events;
mod get_metadata;
mod get_note;
mod get_pow;
mod get_tokens;
mod send_note;

pub use get_metadata::Metadata;

// FAUCET STATE
// ================================================================================================

/// Serves the faucet's API server that handles token requests.
#[derive(Clone)]
pub struct ApiServer {
    mint_state: GetTokensState,
    metadata: Metadata,
    issuance_receiver: watch::Receiver<AssetAmount>,
    rate_limiter: PoWRateLimiter,
    api_keys: HashSet<ApiKey>,
    store: Arc<dyn Store>,
    note_transport_client: Option<Arc<GrpcNoteTransportClient>>,
}

impl ApiServer {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        metadata: Metadata,
        max_claimable_amount: AssetAmount,
        mint_request_sender: MintRequestSender,
        pow_secret: [u8; 32],
        rate_limiter_config: PoWRateLimiterConfig,
        api_keys: &[ApiKey],
        store: Arc<dyn Store>,
        note_transport_client: Option<Arc<GrpcNoteTransportClient>>,
        issuance_receiver: watch::Receiver<AssetAmount>,
    ) -> Self {
        let mint_state = GetTokensState::new(mint_request_sender, max_claimable_amount);

        let rate_limiter = PoWRateLimiter::new_with_cleanup(pow_secret, rate_limiter_config);

        ApiServer {
            mint_state,
            metadata,
            issuance_receiver,
            rate_limiter,
            api_keys: api_keys.iter().cloned().collect::<HashSet<_>>(),
            store,
            note_transport_client,
        }
    }

    /// Serves the faucet API endpoints.
    pub async fn serve(self, url: Url) -> anyhow::Result<()> {
        let app = Router::new()
            .route("/get_metadata", get(get_metadata))
            .route("/issuance", get(issuance_stream))
            .route("/pow", get(get_pow))
            .route("/get_tokens", get(get_tokens))
            .route("/get_note", get(get_note))
            .route("/send_note", post(send_note))
            .layer(
                ServiceBuilder::new()
                    .layer(SetResponseHeaderLayer::if_not_present(
                        http::header::CACHE_CONTROL,
                        HeaderValue::from_static("no-cache"),
                    ))
                    .layer(
                        CorsLayer::new()
                            .allow_origin(tower_http::cors::Any)
                            .allow_methods(tower_http::cors::Any)
                            .allow_headers([http::header::CONTENT_TYPE]),
                    ),
            )
            .with_state(self);

        let listener = url
            .socket_addrs(|| None)
            .with_context(|| format!("failed to parse url {url}"))?;
        let listener = TcpListener::bind(&*listener)
            .await
            .with_context(|| format!("failed to bind TCP listener on {url}"))?;

        tracing::info!(target: COMPONENT, address = %url, "API server started");

        axum::serve(listener, app).await.map_err(Into::into)
    }

    /// Submits a challenge to the `PoW` instance.
    ///
    /// The challenge is validated and added to the cache.
    ///
    /// # Errors
    /// Returns an error if:
    /// * The challenge is expired.
    /// * The challenge is invalid.
    /// * The challenge was already used.
    ///
    /// # Panics
    /// Panics if the current timestamp is before the UNIX epoch.
    #[instrument(target = COMPONENT, name = "server.submit_challenge", skip_all)]
    pub(crate) fn submit_challenge(
        &self,
        challenge: &str,
        nonce: u64,
        account_id: AccountId,
        api_key: ApiKey,
        request_complexity: u64,
    ) -> Result<(), MintRequestError> {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("current timestamp should be greater than unix epoch")
            .as_secs();
        let account_id_bytes: [u8; AccountId::SERIALIZED_SIZE] = account_id.into();
        let mut requestor = [0u8; 32];
        requestor[..AccountId::SERIALIZED_SIZE].copy_from_slice(&account_id_bytes);

        let challenge = hex_to_bytes::<{ Challenge::SERIALIZED_SIZE }>(&format!("0x{challenge}"))
            .map_err(|_| MintRequestError::PowError(ChallengeError::InvalidSerialization))?
            .into();
        self.rate_limiter
            .submit_challenge(requestor, api_key, &challenge, nonce, timestamp, request_complexity)
            .map_err(MintRequestError::PowError)
    }

    /// Computes the request complexity for a given asset amount.
    pub(crate) fn compute_request_complexity(&self, base_units: u64) -> u64 {
        (base_units / self.metadata.base_amount) + 1
    }
}

impl FromRef<ApiServer> for Metadata {
    fn from_ref(input: &ApiServer) -> Self {
        input.metadata.clone()
    }
}

impl FromRef<ApiServer> for GetTokensState {
    fn from_ref(input: &ApiServer) -> Self {
        input.mint_state.clone()
    }
}

impl FromRef<ApiServer> for PoWRateLimiter {
    fn from_ref(input: &ApiServer) -> Self {
        // Clone is cheap: only copies a 32-byte array and increments Arc reference counters.
        input.rate_limiter.clone()
    }
}

// ERRORS
// ================================================================================================

/// Errors that can occur when parsing an account ID or address.
#[derive(Debug, thiserror::Error)]
pub enum AccountError {
    #[error("account ID failed to parse: {0}")]
    ParseId(#[source] AccountIdError),
    #[error("account address failed to parse: {0}")]
    ParseAddress(#[source] AddressError),
    #[error("account address is not an ID based")]
    AddressNotIdBased,
}
