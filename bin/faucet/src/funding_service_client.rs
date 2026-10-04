//! HTTP client for the funding service.
//!
//! The funding service holds the chain's native asset and creates a public P2ID note for every
//! request.

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use anyhow::Context;
use axum::http::StatusCode;
use miden_protocol::account::AccountId;
use miden_protocol::note::Note;
use miden_protocol::utils::serde::Deserializable;
use serde::Deserialize;
use tracing::instrument;
use url::Url;

use crate::COMPONENT;

// CLIENT
// ================================================================================================

/// How long a `/status` response is reused before the service is asked again.
const STATUS_CACHE_LIFETIME: Duration = Duration::from_secs(20);

/// Client for the funding service's JSON HTTP API.
#[derive(Clone)]
pub struct FundingServiceClient {
    client: reqwest::Client,
    url: Url,
    /// The last successful status read and when it was read. Shared between clones so they answer
    /// from the same cache.
    status_cache: Arc<Mutex<Option<(Instant, FundingServiceStatus)>>>,
}

impl FundingServiceClient {
    /// Creates a client for the funding service at `url`, with `timeout` bounding every request.
    pub fn new(url: Url, timeout: Duration) -> anyhow::Result<Self> {
        let client = reqwest::Client::builder()
            .timeout(timeout)
            .build()
            .context("failed to build the funding service HTTP client")?;

        Ok(Self {
            client,
            url,
            status_cache: Arc::default(),
        })
    }

    /// Reads the funding service's status, reusing a response younger than
    /// [`STATUS_CACHE_LIFETIME`]. Failures are not cached.
    pub async fn status(&self) -> Result<FundingServiceStatus, FundingServiceError> {
        if let Some(status) = self.cached_status() {
            return Ok(status);
        }

        let response = self
            .client
            .get(self.endpoint("status"))
            .send()
            .await
            .map_err(FundingServiceError::from_transport)?;

        let status: FundingServiceStatus = Self::parse(response).await?;
        *self.lock_status_cache() = Some((Instant::now(), status.clone()));

        Ok(status)
    }

    /// The last status read, while it is younger than [`STATUS_CACHE_LIFETIME`].
    fn cached_status(&self) -> Option<FundingServiceStatus> {
        let cache = self.lock_status_cache();
        let (read_at, status) = cache.as_ref()?;

        (read_at.elapsed() < STATUS_CACHE_LIFETIME).then(|| status.clone())
    }

    /// Nothing panics while the cache is locked, so the lock cannot be poisoned.
    fn lock_status_cache(
        &self,
    ) -> std::sync::MutexGuard<'_, Option<(Instant, FundingServiceStatus)>> {
        self.status_cache.lock().expect("the status cache lock was poisoned")
    }

    /// Requests a public P2ID note holding `amount` base units of the native asset and targeting
    /// `account_id`.
    #[instrument(target = COMPONENT, name = "funding.request_funds", skip_all, err)]
    pub async fn request_funds(
        &self,
        account_id: AccountId,
        amount: u64,
    ) -> Result<RequestFundsResponse, FundingServiceError> {
        let response = self
            .client
            .post(self.endpoint("request-funds"))
            .json(&serde_json::json!({ "account_id": account_id.to_hex(), "amount": amount }))
            .send()
            .await
            .map_err(FundingServiceError::from_transport)?;

        Self::parse(response).await
    }

    /// Appends `path` to the service's base URL.
    fn endpoint(&self, path: &str) -> String {
        format!("{}/{path}", self.url.as_str().trim_end_matches('/'))
    }

    /// Deserializes a successful response, or turns a failed one into a [`FundingServiceError`].
    async fn parse<T: serde::de::DeserializeOwned>(
        response: reqwest::Response,
    ) -> Result<T, FundingServiceError> {
        let status = response.status();
        if status.is_success() {
            return response.json().await.map_err(|_| FundingServiceError::MalformedResponse);
        }

        let message = response
            .json::<ErrorResponse>()
            .await
            .map_or_else(|_| status.to_string(), |body| body.error);

        Err(match status {
            StatusCode::PRECONDITION_FAILED => FundingServiceError::InsufficientFunds(message),
            _ => FundingServiceError::Rejected { status, message },
        })
    }
}

// RESPONSES
// ================================================================================================

/// The funding service's `/status` response.
#[derive(Debug, Clone, Deserialize)]
pub struct FundingServiceStatus {
    pub version: String,
    /// The account which sends the notes, in hexadecimal.
    pub account_id: String,
    /// The balance of the native asset in the funding account, in base units.
    pub balance: u64,
    /// The largest amount which one funding request accepts, in base units.
    pub max_amount: u64,
}

/// The funding service's `/request-funds` response: the queued P2ID note, as hexadecimal of its
/// serialized form.
#[derive(Debug, Deserialize)]
pub struct RequestFundsResponse {
    #[serde(deserialize_with = "deserialize_hex")]
    pub note: Note,
}

/// Deserializes one of the funding service's hexadecimal fields into its domain type.
fn deserialize_hex<'de, D, T>(deserializer: D) -> Result<T, D::Error>
where
    D: serde::Deserializer<'de>,
    T: Deserializable,
{
    let hex = String::deserialize(deserializer)?;
    let bytes = hex::decode(&hex).map_err(serde::de::Error::custom)?;
    T::read_from_bytes(&bytes).map_err(serde::de::Error::custom)
}

#[derive(Debug, Deserialize)]
struct ErrorResponse {
    error: String,
}

// ERRORS
// ================================================================================================

#[derive(Debug, thiserror::Error)]
pub enum FundingServiceError {
    #[error("the funding service could not be reached: {0}")]
    TransportError(#[source] reqwest::Error),
    #[error("the funding service did not answer in time")]
    TimedOut,
    #[error("the funding service rejected the request: {message}")]
    Rejected { status: StatusCode, message: String },
    #[error("the funding account does not cover the request: {0}")]
    InsufficientFunds(String),
    #[error("the funding service returned a malformed response")]
    MalformedResponse,
}

impl FundingServiceError {
    /// Classifies a failure that happened before a response arrived.
    fn from_transport(error: reqwest::Error) -> Self {
        if error.is_timeout() {
            Self::TimedOut
        } else {
            Self::TransportError(error)
        }
    }

    /// The status code the faucet answers its own client with.
    pub fn status_code(&self) -> StatusCode {
        let Self::Rejected { status, .. } = self else {
            return match self {
                Self::TimedOut => StatusCode::GATEWAY_TIMEOUT,
                Self::InsufficientFunds(_) => StatusCode::SERVICE_UNAVAILABLE,
                _ => StatusCode::BAD_GATEWAY,
            };
        };

        match *status {
            // A bad account ID, a zero amount, or an amount above the service's cap. The faucet
            // checks its own cap first, so this is a request the user can correct.
            StatusCode::BAD_REQUEST => StatusCode::BAD_REQUEST,
            StatusCode::SERVICE_UNAVAILABLE
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
            Self::InsufficientFunds(_) => {
                "The faucet does not have enough funds for this amount. Try a smaller one, or \
                 try again later."
                    .to_owned()
            },
            _ => "The faucet is currently unavailable, please try again later.".to_owned(),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicU64, Ordering};

    use axum::routing::get;
    use axum::{Json, Router};
    use tokio::net::TcpListener;

    use super::*;

    /// Serves a status whose balance grows on every call, so a repeated read shows whether the
    /// answer came from the cache.
    async fn serve_counting_status() -> Url {
        static READS: AtomicU64 = AtomicU64::new(0);

        let status = || async {
            Json(serde_json::json!({
                "version": "0.0.0-stub",
                "account_id": "0x0",
                "balance": READS.fetch_add(1, Ordering::Relaxed),
                "max_amount": 0,
            }))
        };

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = Url::parse(&format!("http://{}", listener.local_addr().unwrap())).unwrap();
        let app = Router::new().route("/status", get(status));
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });

        url
    }

    #[tokio::test]
    async fn the_status_is_read_once_per_cache_lifetime() {
        let client =
            FundingServiceClient::new(serve_counting_status().await, Duration::from_secs(5))
                .unwrap();

        let first = client.status().await.unwrap().balance;
        assert_eq!(client.status().await.unwrap().balance, first, "the second read is cached");

        // Age the cached entry past its lifetime instead of waiting for it to expire.
        {
            let mut cache = client.lock_status_cache();
            let (read_at, _) = cache.as_mut().expect("the first read filled the cache");
            *read_at = read_at.checked_sub(STATUS_CACHE_LIFETIME).expect("the clock is old enough");
        }

        assert_ne!(client.status().await.unwrap().balance, first, "an expired entry is read again");
    }

    fn rejected(status: StatusCode) -> FundingServiceError {
        FundingServiceError::Rejected { status, message: "nope".to_owned() }
    }

    #[test]
    fn service_errors_map_to_faucet_status_codes() {
        assert_eq!(rejected(StatusCode::BAD_REQUEST).status_code(), StatusCode::BAD_REQUEST);
        assert_eq!(
            FundingServiceError::InsufficientFunds(String::new()).status_code(),
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
        assert_eq!(FundingServiceError::MalformedResponse.status_code(), StatusCode::BAD_GATEWAY);
        assert_eq!(FundingServiceError::TimedOut.status_code(), StatusCode::GATEWAY_TIMEOUT);
    }

    /// Only a rejection the user can act on carries the service's message through.
    #[test]
    fn only_bad_requests_expose_the_service_message() {
        assert_eq!(rejected(StatusCode::BAD_REQUEST).user_facing_error(), "nope");
        assert!(!rejected(StatusCode::CONFLICT).user_facing_error().contains("nope"));
    }
}
