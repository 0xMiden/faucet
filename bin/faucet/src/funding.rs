//! HTTP client for the funding service.
//!
//! The funding service holds the chain's native asset and creates a public P2ID note for every
//! request. It waits until the note is committed before answering, so a successful response
//! describes a note that already exists on chain.

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use anyhow::Context;
use axum::http::StatusCode;
use miden_client::account::AccountId;
use miden_client::note::Note;
use miden_client::transaction::TransactionId;
use miden_client::utils::Deserializable;
use serde::Deserialize;
use tracing::instrument;
use url::Url;

use crate::COMPONENT;

// CLIENT
// ================================================================================================

/// How long a status response is reused before the funding service is asked again.
const STATUS_CACHE_TTL: Duration = Duration::from_secs(5);

/// How long the faucet waits for a funding request.
///
/// The funding service answers only once the note is committed, which takes at least one block
/// interval, so the node timeout does not apply here. The service's own HTTP timeout is 5 minutes,
/// past which it answers 408; the faucet waits a little longer so that the service reports the
/// timeout itself.
const REQUEST_FUNDS_TIMEOUT: Duration = Duration::from_secs(310);

/// Client for the funding service's JSON HTTP API.
#[derive(Clone)]
pub struct FundingClient {
    http: reqwest::Client,
    url: Url,
    /// Timeout of a status request, which the service answers from memory.
    status_timeout: Duration,
    /// The last status and when it was read.
    cached_status: Arc<Mutex<Option<(Instant, FundingStatus)>>>,
}

impl FundingClient {
    pub fn new(url: Url, status_timeout: Duration) -> anyhow::Result<Self> {
        // Each request sets its own timeout, because they wait for very different things.
        let http = reqwest::Client::builder()
            .build()
            .context("failed to build the funding service HTTP client")?;

        Ok(Self {
            http,
            url,
            status_timeout,
            cached_status: Arc::new(Mutex::new(None)),
        })
    }

    /// Reads the funding service's status, reusing one no older than [`STATUS_CACHE_TTL`].
    ///
    /// `/get_metadata` is public and unauthenticated, so without this every request to it would
    /// reach the funding service.
    pub async fn cached_status(&self) -> Result<FundingStatus, FundingError> {
        if let Some((read_at, status)) = self.cache().as_ref()
            && read_at.elapsed() < STATUS_CACHE_TTL
        {
            return Ok(status.clone());
        }

        let status = self.status().await?;
        *self.cache() = Some((Instant::now(), status.clone()));

        Ok(status)
    }

    fn cache(&self) -> std::sync::MutexGuard<'_, Option<(Instant, FundingStatus)>> {
        self.cached_status.lock().expect("the funding status cache is poisoned")
    }

    /// Reads the funding service's status.
    pub async fn status(&self) -> Result<FundingStatus, FundingError> {
        let url = self.endpoint("status")?;
        let response = self
            .http
            .get(url)
            .timeout(self.status_timeout)
            .send()
            .await
            .map_err(FundingError::from_transport)?;

        Self::parse(response).await
    }

    /// Requests a public P2ID note holding `amount` base units of the native asset and targeting
    /// `account_id`.
    ///
    /// The funding service answers once the note is committed, which takes at least one block
    /// interval.
    #[instrument(target = COMPONENT, name = "funding.request_funds", skip_all, err)]
    pub async fn request_funds(
        &self,
        account_id: AccountId,
        amount: u64,
    ) -> Result<FundedNote, FundingError> {
        let url = self.endpoint("request-funds")?;
        let response = self
            .http
            .post(url)
            .timeout(REQUEST_FUNDS_TIMEOUT)
            .json(&serde_json::json!({ "account_id": account_id.to_hex(), "amount": amount }))
            .send()
            .await
            .map_err(FundingError::from_transport)?;

        let funded: RequestFundsResponse = Self::parse(response).await?;

        Ok(FundedNote {
            note: decode(&funded.note)?,
            transaction_id: decode(&funded.transaction_id)?,
        })
    }

    /// Appends `path` to the service's base URL.
    fn endpoint(&self, path: &str) -> Result<Url, FundingError> {
        let base = self.url.as_str().trim_end_matches('/');
        Url::parse(&format!("{base}/{path}")).map_err(|_| FundingError::MalformedResponse)
    }

    /// Deserializes a successful response, or turns a failed one into a [`FundingError`].
    async fn parse<T: serde::de::DeserializeOwned>(
        response: reqwest::Response,
    ) -> Result<T, FundingError> {
        let status = response.status();
        if status.is_success() {
            return response.json().await.map_err(|_| FundingError::MalformedResponse);
        }

        // The service reports the reason in an `{"error": "..."}` body. A body in any other shape
        // means we are not talking to a funding service, so the status code carries the meaning.
        let message = response
            .json::<ErrorResponse>()
            .await
            .map_or_else(|_| status.to_string(), |body| body.error);

        Err(FundingError::Rejected { status, message })
    }
}

/// A committed P2ID note created by the funding service.
pub struct FundedNote {
    pub note: Note,
    /// The transaction which created the note.
    pub transaction_id: TransactionId,
}

/// Deserializes one of the funding service's hexadecimal fields.
fn decode<T: Deserializable>(hex: &str) -> Result<T, FundingError> {
    let bytes = hex::decode(hex).map_err(|_| FundingError::MalformedResponse)?;
    T::read_from_bytes(&bytes).map_err(|_| FundingError::MalformedResponse)
}

// RESPONSES
// ================================================================================================

/// The funding service's `/status` response.
#[derive(Debug, Clone, Deserialize)]
pub struct FundingStatus {
    pub version: String,
    /// The account which sends the notes, in hexadecimal.
    pub account_id: String,
    /// The balance of the native asset in the funding account, in base units.
    pub balance: u64,
    /// The block number which the service is synchronized to.
    pub chain_tip: u32,
    /// The largest amount which one funding request accepts, in base units.
    pub max_amount: u64,
    /// The base fee for the verification of a transaction, in base units.
    pub verification_base_fee: u32,
}

#[derive(Debug, Deserialize)]
struct RequestFundsResponse {
    note: String,
    transaction_id: String,
}

#[derive(Debug, Deserialize)]
struct ErrorResponse {
    error: String,
}

// ERRORS
// ================================================================================================

#[derive(Debug, thiserror::Error)]
pub enum FundingError {
    #[error("the funding service could not be reached")]
    Unreachable(#[source] reqwest::Error),
    #[error("the funding service did not answer in time")]
    TimedOut,
    #[error("the funding service rejected the request: {message}")]
    Rejected { status: StatusCode, message: String },
    #[error("the funding service returned a malformed response")]
    MalformedResponse,
}

impl FundingError {
    /// Classifies a failure that happened before a response arrived.
    fn from_transport(error: reqwest::Error) -> Self {
        if error.is_timeout() {
            Self::TimedOut
        } else {
            Self::Unreachable(error)
        }
    }

    /// The status code the faucet answers its own client with.
    ///
    /// The funding service's codes are remapped because the faucet's client cannot act on most of
    /// them, and because 429 is reserved for the `PoW` rate limiter, which pairs it with a
    /// `Retry-After` header.
    pub fn status_code(&self) -> StatusCode {
        let Self::Rejected { status, .. } = self else {
            return match self {
                Self::TimedOut => StatusCode::GATEWAY_TIMEOUT,
                _ => StatusCode::BAD_GATEWAY,
            };
        };

        match *status {
            // A bad account ID, a zero amount, or an amount above the service's cap. The faucet
            // checks its own cap first, so this is a request the user can correct.
            StatusCode::BAD_REQUEST => StatusCode::BAD_REQUEST,
            // Out of funds, not synchronized yet, too many requests queued, or a transaction that
            // did not commit. None of these are the user's doing and all of them may clear.
            StatusCode::PRECONDITION_FAILED
            | StatusCode::SERVICE_UNAVAILABLE
            | StatusCode::CONFLICT
            | StatusCode::TOO_MANY_REQUESTS => StatusCode::SERVICE_UNAVAILABLE,
            StatusCode::REQUEST_TIMEOUT => StatusCode::GATEWAY_TIMEOUT,
            _ => StatusCode::BAD_GATEWAY,
        }
    }

    /// Take care to not expose internal errors here.
    pub fn user_facing_error(&self) -> String {
        match self {
            Self::Rejected { status, message } if *status == StatusCode::BAD_REQUEST => {
                message.clone()
            },
            _ => "The faucet is currently unavailable, please try again later.".to_owned(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rejected(status: StatusCode) -> FundingError {
        FundingError::Rejected { status, message: "nope".to_owned() }
    }

    #[test]
    fn service_errors_map_to_faucet_status_codes() {
        assert_eq!(rejected(StatusCode::BAD_REQUEST).status_code(), StatusCode::BAD_REQUEST);
        assert_eq!(
            rejected(StatusCode::PRECONDITION_FAILED).status_code(),
            StatusCode::SERVICE_UNAVAILABLE
        );
        assert_eq!(
            rejected(StatusCode::TOO_MANY_REQUESTS).status_code(),
            StatusCode::SERVICE_UNAVAILABLE,
            "429 is reserved for the PoW rate limiter"
        );
        assert_eq!(
            rejected(StatusCode::REQUEST_TIMEOUT).status_code(),
            StatusCode::GATEWAY_TIMEOUT
        );
        assert_eq!(rejected(StatusCode::IM_A_TEAPOT).status_code(), StatusCode::BAD_GATEWAY);
        assert_eq!(FundingError::MalformedResponse.status_code(), StatusCode::BAD_GATEWAY);
        assert_eq!(FundingError::TimedOut.status_code(), StatusCode::GATEWAY_TIMEOUT);
    }

    /// Only a rejection the user can act on carries the service's message through.
    #[test]
    fn only_bad_requests_expose_the_service_message() {
        assert_eq!(rejected(StatusCode::BAD_REQUEST).user_facing_error(), "nope");
        assert!(!rejected(StatusCode::CONFLICT).user_facing_error().contains("nope"));
    }
}
