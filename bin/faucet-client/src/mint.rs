//! CLI command to request a public P2ID note from a remote faucet by solving its `PoW` challenge.

use std::time::Duration;

use clap::Parser;
use miden_client::Word;
use miden_client::account::{AccountId, Address};
use miden_client::address::AddressId;
use miden_client::note::{NoteId, get_input_note_with_id_prefix};
use miden_client::store::NoteRecordError;
use miden_client::transaction::{TransactionId, TransactionRequestBuilder};
use miden_client_cli::CliClient;
use miden_faucet_lib::requests::{
    GetPowResponse,
    GetTokensQueryParams,
    GetTokensResponse,
    MintResponse,
    PowQueryParams,
};
use miden_pow_rate_limiter::{Challenge, ChallengeError};
use rand::RngExt;
use reqwest::{Client as HttpClient, Url};
use tokio::task;

// CONSTANTS
// ================================================================================================

const DEFAULT_FAUCET_URL: &str = "https://faucet-api.testnet.miden.io";
const REQUEST_TIMEOUT_MS: u64 = 30_000;
const MAX_SYNC_RETRIES: u32 = 3;
const SYNC_RETRY_DELAY_SECS: u64 = 5;

// CLI
// ================================================================================================

/// Mint tokens from a remote faucet by solving its `PoW` challenge and requesting a **public**
/// P2ID note.
#[derive(Debug, Parser, Clone)]
pub struct MintCmd {
    /// Faucet API base URL. Defaults to the public testnet faucet.
    #[arg(long = "url", default_value = DEFAULT_FAUCET_URL, value_name = "URL")]
    api_url: String,

    /// Account ID or address to receive the minted tokens.
    #[arg(short = 't', long = "target-account", value_name = "ACCOUNT")]
    target_account: String,

    /// Amount to mint (in base units).
    #[arg(short = 'a', long = "amount", value_name = "U64")]
    amount: u64,

    /// Optional faucet API key.
    #[arg(long = "api-key", value_name = "STRING")]
    api_key: Option<String>,

    /// Skip auto-consumption of the minted note.
    #[arg(long = "no-consume", default_value_t = false)]
    no_consume: bool,
}

impl MintCmd {
    /// Executes the mint command.
    pub async fn execute(&self) -> Result<(), MintClientError> {
        if self.amount == 0 {
            return Err(MintClientError::AmountZero);
        }

        let account_id = parse_account_id(&self.target_account)?;
        let faucet_client =
            FaucetHttpClient::new(&self.api_url, REQUEST_TIMEOUT_MS, self.api_key.clone())?;

        println!(
            "Requesting PoW challenge for account {} from faucet at {}...",
            account_id.to_hex(),
            faucet_client.base_url
        );

        let (challenge, target) = faucet_client.request_pow(&account_id, self.amount).await?;

        println!("Solving faucet PoW challenge, this can take some time...");
        let nonce = solve_challenge(&challenge, target).await?;

        println!("Submitting mint request for a public P2ID note...");
        let mint_response = faucet_client
            .request_tokens(&challenge, nonce, &account_id, self.amount)
            .await?;

        println!("Mint request accepted. Transaction: {}", mint_response.tx_id.to_hex());
        println!("Public P2ID note commitment: {}", mint_response.note_id.to_hex());

        if self.no_consume {
            return Ok(());
        }

        Box::pin(self.consume_note(account_id, mint_response.note_id)).await
    }

    /// Consumes the minted note instantiating a new client instance with the global user
    /// configuration.
    async fn consume_note(
        &self,
        account_id: AccountId,
        note_id: NoteId,
    ) -> Result<(), MintClientError> {
        println!("Initializing client...");
        let mut client = CliClient::new()
            .await
            .map_err(|e| MintClientError::ClientConfig(e.to_string()))?;

        let note_id_hex = note_id.to_hex();
        let mut note_record = None;

        for attempt in 0..MAX_SYNC_RETRIES {
            println!("Syncing client state...");
            client
                .sync_state()
                .await
                .map_err(|e| MintClientError::SyncFailed(e.to_string()))?;

            match get_input_note_with_id_prefix(&client, &note_id_hex).await {
                Ok(record) => {
                    note_record = Some(record);
                    break;
                },
                Err(_) if attempt < MAX_SYNC_RETRIES - 1 => {
                    println!(
                        "Note not found, retrying in {} seconds... ({}/{})",
                        SYNC_RETRY_DELAY_SECS,
                        attempt + 1,
                        MAX_SYNC_RETRIES
                    );
                    tokio::time::sleep(Duration::from_secs(SYNC_RETRY_DELAY_SECS)).await;
                },
                Err(_) => {},
            }
        }

        let note_record = note_record.ok_or_else(|| {
            MintClientError::NoteNotFound(format!(
                "Note {note_id_hex} not found after {MAX_SYNC_RETRIES} sync attempts. \
                 You can manually consume it later using: miden-client consume-notes {note_id_hex}"
            ))
        })?;

        println!("Consuming note...");
        let input_note = note_record
            .try_into()
            .map_err(|e: NoteRecordError| MintClientError::ConsumeTransaction(e.to_string()))?;

        let tx_request = TransactionRequestBuilder::new()
            .input_notes(vec![(input_note, None)])
            .build()
            .map_err(|e| MintClientError::ConsumeTransaction(e.to_string()))?;

        Box::pin(client.submit_new_transaction(account_id, tx_request))
            .await
            .map_err(|e| MintClientError::ConsumeTransaction(e.to_string()))?;

        println!("Note {note_id_hex} successfully consumed.");

        Ok(())
    }
}

// HTTP CLIENT
// ================================================================================================

/// HTTP client for interacting with the faucet API.
#[derive(Clone)]
struct FaucetHttpClient {
    http_client: HttpClient,
    base_url: Url,
    api_key: Option<String>,
}

impl FaucetHttpClient {
    /// Creates a new `FaucetHttpClient` instance.
    fn new(
        endpoint: &str,
        timeout_ms: u64,
        api_key: Option<String>,
    ) -> Result<Self, MintClientError> {
        let base_url = Url::parse(endpoint)
            .map_err(|err| MintClientError::InvalidUrl(endpoint.to_owned(), err))?;

        let http_client = HttpClient::builder()
            .timeout(Duration::from_millis(timeout_ms))
            .build()
            .map_err(MintClientError::HttpClient)?;

        Ok(Self { http_client, base_url, api_key })
    }

    /// Requests a `PoW` challenge from the faucet API.
    async fn request_pow(
        &self,
        account_id: &AccountId,
        amount: u64,
    ) -> Result<(String, u64), MintClientError> {
        let pow_url = self
            .base_url
            .join("pow")
            .map_err(|err| MintClientError::InvalidUrl(self.base_url.to_string(), err))?;

        let params = PowQueryParams {
            account_id: account_id.to_hex(),
            amount,
            api_key: self.api_key.clone(),
        };

        let response = self
            .http_client
            .get(pow_url)
            .query(&params)
            .send()
            .await
            .map_err(|err| MintClientError::Request("PoW", err))?;

        if !response.status().is_success() {
            let status = response.status();
            let body =
                response.text().await.map_err(|err| MintClientError::ResponseBody("pow", err))?;
            return Err(MintClientError::UnexpectedStatus("pow", status, body));
        }

        let body =
            response.text().await.map_err(|err| MintClientError::ResponseBody("pow", err))?;
        let parsed = serde_json::from_str::<GetPowResponse>(&body)
            .map_err(|err| MintClientError::ParseResponse("PoW", err, body.clone()))?;

        Ok((parsed.challenge, parsed.target))
    }

    /// Requests tokens from the faucet API.
    async fn request_tokens(
        &self,
        challenge: &str,
        nonce: u64,
        account_id: &AccountId,
        amount: u64,
    ) -> Result<MintResponse, MintClientError> {
        let url = self
            .base_url
            .join("get_tokens")
            .map_err(|err| MintClientError::InvalidUrl(self.base_url.to_string(), err))?;

        let params = GetTokensQueryParams {
            account_id: account_id.to_hex(),
            asset_amount: amount,
            is_private_note: None,
            challenge: challenge.to_owned(),
            nonce,
            api_key: self.api_key.clone(),
        };

        let response = self
            .http_client
            .get(url)
            .query(&params)
            .send()
            .await
            .map_err(|err| MintClientError::Request("get_tokens", err))?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response
                .text()
                .await
                .map_err(|err| MintClientError::ResponseBody("get_tokens", err))?;
            return Err(MintClientError::UnexpectedStatus("get_tokens", status, body));
        }

        let body = response
            .text()
            .await
            .map_err(|err| MintClientError::ResponseBody("get_tokens", err))?;
        let parsed = serde_json::from_str::<GetTokensResponse>(&body)
            .map_err(|err| MintClientError::ParseResponse("get_tokens", err, body.clone()))?;

        let note_id = NoteId::try_from_hex(&parsed.note_id).map_err(|err| {
            MintClientError::InvalidNoteId(parsed.note_id.clone(), err.to_string())
        })?;

        let tx_id_word = Word::try_from(parsed.tx_id.as_str()).map_err(|err| {
            MintClientError::InvalidTransactionId(parsed.tx_id.clone(), err.to_string())
        })?;
        // SAFETY: TransactionId is a newtype wrapper around Word with the same memory layout
        let tx_id = unsafe { std::mem::transmute::<Word, TransactionId>(tx_id_word) };

        Ok(MintResponse { note_id, tx_id })
    }
}

// ERRORS
// ================================================================================================

/// Errors that can occur while interacting with the faucet API.
#[derive(Debug, thiserror::Error)]
pub enum MintClientError {
    #[error("amount must be greater than zero")]
    AmountZero,
    #[error("invalid account `{0}`: {1}")]
    InvalidAccount(String, String),
    #[error("invalid faucet URL `{0}`: {1}")]
    InvalidUrl(String, url::ParseError),
    #[error("failed to build HTTP client: {0}")]
    HttpClient(#[source] reqwest::Error),
    #[error("{0} request failed: {1}")]
    Request(&'static str, #[source] reqwest::Error),
    #[error("{0} request failed with status {1}: {2}")]
    UnexpectedStatus(&'static str, reqwest::StatusCode, String),
    #[error("failed to parse {0} response: {1}. Body: {2}")]
    ParseResponse(&'static str, #[source] serde_json::Error, String),
    #[error("failed to read {0} response body: {1}")]
    ResponseBody(&'static str, #[source] reqwest::Error),
    #[error("faucet returned a PoW target of 0")]
    ZeroTarget,
    #[error("invalid challenge hex returned by faucet: {0}")]
    InvalidChallengeHex(#[source] hex::FromHexError),
    #[error("invalid challenge returned by faucet: {0}")]
    InvalidChallenge(#[source] ChallengeError),
    #[error("PoW solving task failed: {0}")]
    PowTask(String),
    #[error("invalid note id `{0}`: {1}")]
    InvalidNoteId(String, String),
    #[error("invalid transaction id `{0}`: {1}")]
    InvalidTransactionId(String, String),
    #[error("failed to initialize client: {0}")]
    ClientConfig(String),
    #[error("failed to sync client state: {0}")]
    SyncFailed(String),
    #[error("note not found: {0}")]
    NoteNotFound(String),
    #[error("failed to consume note: {0}")]
    ConsumeTransaction(String),
}

// HELPERS
// ================================================================================================

/// Parses a user provided account ID string and returns the corresponding `AccountId`
fn parse_account_id(input: &str) -> Result<AccountId, MintClientError> {
    if input.starts_with("0x") {
        AccountId::from_hex(input)
            .map_err(|err| MintClientError::InvalidAccount(input.to_owned(), err.to_string()))
    } else {
        Address::decode(input)
            .map_err(|err| MintClientError::InvalidAccount(input.to_owned(), err.to_string()))
            .and_then(|(_, address)| match address.id() {
                AddressId::AccountId(account_id) => Ok(account_id),
                _ => Err(MintClientError::InvalidAccount(
                    input.to_owned(),
                    "address is not account-based".to_owned(),
                )),
            })
    }
}

/// Solves the `PoW` challenge and returns the nonce that satisfies the target.
///
/// Heavy work runs on a blocking thread so we don't stall the async runtime.
async fn solve_challenge(challenge_hex: &str, target: u64) -> Result<u64, MintClientError> {
    if target == 0 {
        return Err(MintClientError::ZeroTarget);
    }

    let challenge_bytes =
        hex::decode(challenge_hex).map_err(MintClientError::InvalidChallengeHex)?;
    let challenge = Challenge::try_from(challenge_bytes.as_slice())
        .map_err(MintClientError::InvalidChallenge)?;

    task::spawn_blocking(move || {
        let mut rng = rand::rng();

        loop {
            let nonce: u64 = rng.random();

            if challenge.validate_pow(nonce) {
                return nonce;
            }
        }
    })
    .await
    .map_err(|err| MintClientError::PowTask(err.to_string()))
}
