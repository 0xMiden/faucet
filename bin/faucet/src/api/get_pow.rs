use axum::Json;
use axum::extract::{Query, State};
use axum::response::IntoResponse;
use http::StatusCode;
use miden_faucet_lib::requests::{GetPowResponse, PowQueryParams};
use miden_faucet_lib::types::AssetAmount;
use miden_protocol::account::AccountId;
use miden_protocol::address::{Address, AddressId};
use miden_protocol::utils::ToHex;
use tracing::{info_span, instrument};

use crate::COMPONENT;
use crate::api::{AccountError, ApiServer};
use crate::api_key::ApiKey;
use crate::funding_service_client::FundingServiceError;

// ENDPOINT
// ================================================================================================

#[instrument(
    parent = None, target = COMPONENT, name = "server.get_pow", skip_all,
    fields(account_id = %params.account_id, api_key = ?params.api_key), err
)]
pub async fn get_pow(
    State(server): State<ApiServer>,
    Query(params): Query<PowQueryParams>,
) -> Result<Json<GetPowResponse>, PowRequestError> {
    let request = validate_pow_params(params)?;
    // Check the requested amount is below the maximum claimable
    if request.amount > server.max_claimable_amount.base_units() {
        return Err(PowRequestError::AssetAmountTooBig(
            request.amount,
            server.max_claimable_amount,
        ));
    }

    // Check the funding service has enough balance to fill the requested amount
    if let Ok(status) = server.funding_service.status().await
        && request.amount > status.balance
    {
        return Err(PowRequestError::FundingServiceError(FundingServiceError::InsufficientFunds(
            format!("balance {} is below the requested {}", status.balance, request.amount),
        )));
    }
    let account_id_bytes: [u8; AccountId::SERIALIZED_SIZE] = request.account_id.into();
    let mut requestor = [0u8; 32];
    requestor[..AccountId::SERIALIZED_SIZE].copy_from_slice(&account_id_bytes);

    let challenge = {
        let span =
            info_span!("server.get_pow.build_challenge", leading_zeros = tracing::field::Empty);
        let _enter = span.enter();
        let request_complexity = server.compute_request_complexity(request.amount);
        let challenge =
            server
                .rate_limiter
                .build_challenge(requestor, request.api_key, request_complexity);
        span.record("leading_zeros", challenge.target().leading_zeros());
        challenge
    };

    Ok(Json(GetPowResponse {
        challenge: challenge.to_bytes().to_hex(),
        target: challenge.target(),
        timestamp: challenge.timestamp(),
    }))
}

// REQUEST VALIDATION
// ================================================================================================

/// Validated and parsed request for the `PoW` challenge.
pub struct PowRequest {
    pub amount: u64,
    pub account_id: AccountId,
    pub api_key: ApiKey,
}

fn validate_pow_params(params: PowQueryParams) -> Result<PowRequest, PowRequestError> {
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
    .map_err(PowRequestError::AccountError)?;

    let api_key = params
        .api_key
        .as_deref()
        .map(ApiKey::decode)
        .transpose()
        .map_err(|_| PowRequestError::InvalidApiKey(params.api_key.unwrap_or_default()))?
        .unwrap_or_default();

    Ok(PowRequest {
        amount: params.amount,
        account_id,
        api_key,
    })
}

#[derive(Debug, thiserror::Error)]
pub enum PowRequestError {
    #[error(transparent)]
    AccountError(#[from] AccountError),
    #[error("API key {0} failed to parse")]
    InvalidApiKey(String),
    #[error("requested amount {0} exceeds the maximum claimable amount of {1}")]
    AssetAmountTooBig(u64, AssetAmount),
    #[error(transparent)]
    FundingServiceError(FundingServiceError),
}

impl PowRequestError {
    /// Take care to not expose internal errors here.
    fn user_facing_error(&self) -> String {
        match self {
            Self::AccountError(_) => "Please enter a valid recipient address".to_owned(),
            Self::InvalidApiKey(_) => "Invalid API key".to_owned(),
            Self::AssetAmountTooBig(..) => self.to_string(),
            Self::FundingServiceError(error) => error.user_facing_error(),
        }
    }

    fn status_code(&self) -> StatusCode {
        match self {
            Self::FundingServiceError(error) => error.status_code(),
            _ => StatusCode::BAD_REQUEST,
        }
    }
}

impl IntoResponse for PowRequestError {
    fn into_response(self) -> axum::response::Response {
        (self.status_code(), self.user_facing_error()).into_response()
    }
}
