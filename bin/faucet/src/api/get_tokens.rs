use axum::Json;
use axum::extract::{Query, State};
use axum::http::{HeaderMap, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use miden_client::account::{AccountId, Address};
use miden_client::address::AddressId;
use miden_faucet_lib::requests::{GetTokensQueryParams, GetTokensResponse, MintRequest};
use miden_faucet_lib::types::{AssetAmount, AssetAmountError};
use miden_pow_rate_limiter::ChallengeError;
use tracing::instrument;

use crate::COMPONENT;
use crate::api::{AccountError, ApiServer};
use crate::api_key::ApiKey;
use crate::funding_service_client::FundingServiceError;

// ENDPOINT
// ================================================================================================

#[instrument(
    parent = None, target = COMPONENT, name = "server.get_tokens", skip_all, err,
    fields(
        account_id = %request.account_id,
        asset_amount = %request.asset_amount,
    )
)]
pub async fn get_tokens(
    State(server): State<ApiServer>,
    Query(request): Query<GetTokensQueryParams>,
) -> Result<Json<GetTokensResponse>, GetTokenError> {
    let validated_request =
        validate_get_tokens_params(&request, &server).map_err(GetTokenError::InvalidRequest)?;

    let funding_response = server
        .funding_service
        .request_funds(validated_request.account_id, validated_request.asset_amount.base_units())
        .await
        .map_err(GetTokenError::FundingServiceError)?;

    Ok(Json(GetTokensResponse {
        tx_id: funding_response.transaction_id.to_hex(),
        note_id: funding_response.note.id().to_hex(),
    }))
}

// REQUEST VALIDATION
// ================================================================================================

#[derive(Debug, thiserror::Error)]
pub enum MintRequestError {
    #[error(transparent)]
    AccountError(#[from] AccountError),
    #[error("requested amount {0} exceeds the maximum claimable amount of {1}")]
    AssetAmountTooBig(AssetAmount, AssetAmount),
    #[error("requested amount {0} is not a valid asset amount")]
    InvalidAssetAmount(AssetAmountError),
    #[error(transparent)]
    PowError(#[from] ChallengeError),
    #[error("API key {0} is invalid")]
    InvalidApiKey(String),
}

#[derive(Debug, thiserror::Error)]
pub enum GetTokenError {
    #[error("invalid request: {0}")]
    InvalidRequest(#[source] MintRequestError),
    #[error(transparent)]
    FundingServiceError(FundingServiceError),
}

impl GetTokenError {
    fn status_code(&self) -> StatusCode {
        match self {
            Self::InvalidRequest(MintRequestError::PowError(ChallengeError::RateLimited(_))) => {
                StatusCode::TOO_MANY_REQUESTS
            },
            Self::InvalidRequest(_) => StatusCode::BAD_REQUEST,
            Self::FundingServiceError(error) => error.status_code(),
        }
    }

    /// Take care to not expose internal errors here.
    fn user_facing_error(&self) -> String {
        match self {
            Self::InvalidRequest(MintRequestError::PowError(ChallengeError::RateLimited(
                remaining,
            ))) => format!("Account is rate limited for {remaining} more seconds."),
            Self::InvalidRequest(MintRequestError::AccountError(_)) => {
                "Please enter a valid recipient address.".to_owned()
            },
            Self::InvalidRequest(error) => error.to_string(),
            Self::FundingServiceError(error) => error.user_facing_error(),
        }
    }

    /// Write a trace log for the error, if applicable.
    fn trace(&self) {
        match self {
            Self::InvalidRequest(_) => {},
            Self::FundingServiceError(error) => {
                tracing::error!(target: COMPONENT, %error, "funding service request failed");
            },
        }
    }

    /// Returns headers for the error response. In case of a rate limited error, the Retry-After
    /// header is set. Otherwise, just returns an empty header map.
    fn headers(&self) -> HeaderMap {
        let mut headers = HeaderMap::new();
        if let Self::InvalidRequest(MintRequestError::PowError(ChallengeError::RateLimited(
            timestamp,
        ))) = self
        {
            headers.insert(axum::http::header::RETRY_AFTER, HeaderValue::from(*timestamp));
        }
        headers
    }
}

impl IntoResponse for GetTokenError {
    fn into_response(self) -> Response {
        self.trace();
        (self.headers(), (self.status_code(), self.user_facing_error())).into_response()
    }
}

/// Further validates a raw request, turning it into a valid [`MintRequest`] which can be
/// submitted to the faucet client.
///
/// # Errors
///
/// Returns an error if:
///   - the account ID is not a valid hex string
///   - the asset amount is not one of the provided options
///   - the API key is invalid
///   - the challenge is invalid
///   - the nonce doesn't solve the challenge
///   - the challenge timestamp is expired
///   - the challenge has already been used
fn validate_get_tokens_params(
    params: &GetTokensQueryParams,
    server: &ApiServer,
) -> Result<MintRequest, MintRequestError> {
    let account_id = if params.account_id.starts_with("0x") {
        AccountId::from_hex(&params.account_id).map_err(AccountError::ParseId)
    } else {
        Address::decode(&params.account_id)
            .map_err(AccountError::ParseAddress)
            .and_then(|(_, address)| match address.id() {
                AddressId::AccountId(account_id) => Ok(account_id),
                _ => Err(AccountError::AddressNotIdBased),
            })
    }
    .map_err(MintRequestError::AccountError)?;

    let asset_amount =
        AssetAmount::new(params.asset_amount).map_err(MintRequestError::InvalidAssetAmount)?;
    if asset_amount > server.max_claimable_amount {
        return Err(MintRequestError::AssetAmountTooBig(asset_amount, server.max_claimable_amount));
    }

    // Check the API key, if provided
    let api_key = params.api_key.as_deref().map(ApiKey::decode).transpose()?;
    if let Some(api_key) = &api_key
        && !server.api_keys.contains(api_key)
    {
        return Err(MintRequestError::InvalidApiKey(api_key.encode()));
    }

    // Validate Challenge and nonce
    let request_complexity = server.compute_request_complexity(asset_amount.base_units());

    server.submit_challenge(
        &params.challenge,
        params.nonce,
        account_id,
        api_key.unwrap_or_default(),
        request_complexity,
    )?;

    Ok(MintRequest { account_id, asset_amount })
}
