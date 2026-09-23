mod api;
mod api_key;
mod frontend;
mod funding_service_client;
mod logging;
mod network;
#[cfg(test)]
mod testing;

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::Context;
use clap::{Parser, Subcommand};
use miden_faucet_lib::FaucetId;
use miden_faucet_lib::types::AssetAmount;
use miden_pow_rate_limiter::PoWRateLimiterConfig;
use miden_protocol::account::AccountId;
use rand::SeedableRng;
use rand::rngs::ChaCha20Rng;
use sha2::{Digest, Sha256};
use tokio::task::JoinSet;
use url::Url;

use crate::api::{ApiServer, Metadata};
use crate::api_key::ApiKey;
use crate::frontend::serve_frontend;
use crate::funding_service_client::FundingServiceClient;
use crate::logging::OpenTelemetry;
use crate::network::FaucetNetwork;

// CONSTANTS
// =================================================================================================

const COMPONENT: &str = "miden-faucet-server";
const DEFAULT_API_KEYS_PATH: &str = "api_keys.txt";

const ENV_API_BIND_PORT: &str = "MIDEN_FAUCET_API_BIND_PORT";
const ENV_API_PUBLIC_URL: &str = "MIDEN_FAUCET_API_PUBLIC_URL";
const ENV_FRONTEND_BIND_PORT: &str = "MIDEN_FAUCET_FRONTEND_BIND_PORT";
const ENV_NO_FRONTEND: &str = "MIDEN_FAUCET_NO_FRONTEND";
const ENV_NETWORK: &str = "MIDEN_FAUCET_NETWORK";
const ENV_NODE_URL: &str = "MIDEN_FAUCET_NODE_URL";
const ENV_TIMEOUT: &str = "MIDEN_FAUCET_TIMEOUT";
const ENV_MAX_CLAIMABLE_AMOUNT: &str = "MIDEN_FAUCET_MAX_CLAIMABLE_AMOUNT";
const ENV_POW_SECRET: &str = "MIDEN_FAUCET_POW_SECRET";
const ENV_POW_CHALLENGE_LIFETIME: &str = "MIDEN_FAUCET_POW_CHALLENGE_LIFETIME";
const ENV_POW_CLEANUP_INTERVAL: &str = "MIDEN_FAUCET_POW_CLEANUP_INTERVAL";
const ENV_POW_GROWTH_RATE: &str = "MIDEN_FAUCET_POW_GROWTH_RATE";
const ENV_POW_BASELINE: &str = "MIDEN_FAUCET_POW_BASELINE";
const ENV_BASE_AMOUNT: &str = "MIDEN_FAUCET_BASE_AMOUNT";
const ENV_ENABLE_OTEL: &str = "MIDEN_FAUCET_ENABLE_OTEL";
const ENV_API_KEYS: &str = "MIDEN_FAUCET_API_KEYS";
const ENV_DECIMALS: &str = "MIDEN_FAUCET_DECIMALS";
const ENV_EXPLORER_URL: &str = "MIDEN_FAUCET_EXPLORER_URL";
const ENV_FUNDING_SERVICE_URL: &str = "MIDEN_FAUCET_FUNDING_SERVICE_URL";

// COMMANDS
// ================================================================================================

#[derive(Parser)]
#[command(version, about, long_about = None)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
}

#[allow(clippy::large_enum_variant)]
#[derive(Subcommand)]
pub enum Command {
    /// Manage API keys.
    ApiKey {
        #[command(subcommand)]
        command: ApiKeyCommand,
    },

    /// Start the faucet server
    Start {
        #[clap(flatten)]
        config: FaucetConfig,

        /// Base URL of the funding service that emits the notes.
        #[arg(long = "funding-service-url", value_name = "URL", env = ENV_FUNDING_SERVICE_URL)]
        funding_service_url: Url,

        /// Decimals of the token the funding service hands out, used by the frontend to convert
        /// base units into token amounts.
        #[arg(long = "decimals", value_name = "U8", env = ENV_DECIMALS)]
        decimals: u8,

        /// Port to bind the API server. The server will be started on `0.0.0.0:<api-bind-port>`.
        #[arg(long = "api-bind-port", value_name = "PORT", env = ENV_API_BIND_PORT, default_value = "8000")]
        api_bind_port: u16,

        /// Public URL to access the API server.
        #[arg(long = "api-public-url", value_name = "URL", env = ENV_API_PUBLIC_URL, default_value = "http://localhost:8000")]
        api_public_url: Url,

        /// Port to bind the frontend server. The server will be started on
        /// `0.0.0.0:<frontend-bind-port>`.
        #[arg(long = "frontend-bind-port", value_name = "PORT", env = ENV_FRONTEND_BIND_PORT, default_value = "8080")]
        frontend_bind_port: u16,

        /// Optionally disable the frontend server.
        #[arg(long = "no-frontend", value_name = "BOOL", default_value_t = false, env = ENV_NO_FRONTEND)]
        no_frontend: bool,

        /// The maximum amount of assets' base units that can be dispersed on each request.
        #[arg(long = "max-claimable-amount", value_name = "U64", env = ENV_MAX_CLAIMABLE_AMOUNT, default_value = "1000000000")]
        max_claimable_amount: u64,

        /// The secret to be used by the server to sign the `PoW` challenges. This should NOT be
        /// shared.
        ///
        /// If not provided, a random secret is generated at startup.
        #[arg(long = "pow-secret", value_name = "STRING", env = ENV_POW_SECRET)]
        pow_secret: Option<String>,

        /// The duration during which the `PoW` challenges are valid. Changing this will affect the
        /// rate limiting, since it works by rejecting new submissions while the previous submitted
        /// challenge is still valid.
        #[arg(long = "pow-challenge-lifetime", value_name = "DURATION", env = ENV_POW_CHALLENGE_LIFETIME, default_value = "30s", value_parser = humantime::parse_duration)]
        pow_challenge_lifetime: Duration,

        /// Defines how quickly the `PoW` difficulty grows with the number of requests. The number
        /// of active challenges gets multiplied by the growth rate to compute the load
        /// difficulty.
        ///
        /// Meaning, the difficulty bits of the challenge will increase approximately by
        /// `log2(growth_rate * num_active_challenges)`.
        #[arg(long = "pow-growth-rate", value_name = "F64", env = ENV_POW_GROWTH_RATE, default_value = "0.1")]
        pow_growth_rate: f64,

        /// The interval at which the `PoW` challenge cache is cleaned up.
        #[arg(long = "pow-cleanup-interval", value_name = "DURATION", env = ENV_POW_CLEANUP_INTERVAL, default_value = "2s", value_parser = humantime::parse_duration)]
        pow_cleanup_interval: Duration,

        /// The baseline for the `PoW` challenges. This sets the `PoW` difficulty (in bits) that a
        /// a challenge will have when there are no requests against the faucet. It must be between
        /// 0 and 32.
        #[arg(value_parser = clap::value_parser!(u8).range(0..=32))]
        #[arg(long = "pow-baseline", value_name = "U8", env = ENV_POW_BASELINE, default_value = "16")]
        pow_baseline: u8,

        /// The baseline amount for token requests (in base units). Requests for greater amounts
        /// would require higher level of `PoW`.
        ///
        /// The request complexity for challenges is computed as: `request_complexity = (amount /
        /// base_amount) + 1`
        #[arg(long = "base-amount", value_name = "U64", env = ENV_BASE_AMOUNT, default_value = "100000000")]
        base_amount: u64,

        /// Enables the exporting of traces for OpenTelemetry.
        ///
        /// This can be further configured using environment variables as defined in the official
        /// OpenTelemetry documentation. See our operator manual for further details.
        #[arg(long = "enable-otel", value_name = "BOOL", default_value_t = false, env = ENV_ENABLE_OTEL)]
        open_telemetry: bool,

        /// Explorer URL.
        #[arg(long = "explorer-url", value_name = "URL", env = ENV_EXPLORER_URL)]
        explorer_url: Option<Url>,
    },
}

#[derive(Subcommand)]
pub enum ApiKeyCommand {
    /// Generate an API key and persist it to the store.
    ///
    /// Prints out the generated API key to stdout. The key is also stored in the faucet's
    /// database so that it is automatically loaded when the faucet starts.
    Create {
        /// Path to the file holding the API keys, one key per line.
        #[arg(long = "file", value_name = "FILE", default_value = DEFAULT_API_KEYS_PATH, env = ENV_API_KEYS)]
        api_keys_path: PathBuf,
    },

    /// Remove an API key from the store.
    Remove {
        /// Path to the file holding the API keys, one key per line.
        #[arg(long = "file", value_name = "FILE", default_value = DEFAULT_API_KEYS_PATH, env = ENV_API_KEYS)]
        api_keys_path: PathBuf,

        /// The API key to remove (encoded string).
        api_key: String,
    },

    /// List all API keys in the store.
    List {
        /// Path to the file holding the API keys, one key per line.
        #[arg(long = "file", value_name = "FILE", default_value = DEFAULT_API_KEYS_PATH, env = ENV_API_KEYS)]
        api_keys_path: PathBuf,
    },
}

/// Configuration for the faucet.
#[derive(Parser, Debug, Clone)]
pub struct FaucetConfig {
    /// Path to the file holding the API keys, one key per line.
    #[arg(long = "file", value_name = "FILE", default_value = DEFAULT_API_KEYS_PATH, env = ENV_API_KEYS)]
    api_keys_path: PathBuf,

    /// Timeout for attempting to connect to the node.
    #[arg(long = "timeout", value_name = "DURATION", default_value = "5s", env = ENV_TIMEOUT, value_parser = humantime::parse_duration)]
    timeout: Duration,

    /// Network configuration to use. Options are `devnet`, `testnet`, `localhost` or a custom
    /// network. It is used to display the correct bech32 addresses in the UI.
    #[arg(long = "network", value_name = "NETWORK", default_value = "localhost", env = ENV_NETWORK)]
    network: FaucetNetwork,

    /// Node RPC gRPC endpoint in the format `http://<host>[:<port>]`. If not set, the url is derived
    /// from the specified network.
    #[arg(long = "node-url", value_name = "URL", env = ENV_NODE_URL)]
    node_url: Option<Url>,
}

impl Command {
    fn open_telemetry(&self) -> OpenTelemetry {
        if matches!(*self, Command::Start { open_telemetry: true, .. }) {
            OpenTelemetry::Enabled
        } else {
            OpenTelemetry::Disabled
        }
    }
}

// MAIN
// =================================================================================================

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    // Configure tracing with optional OpenTelemetry exporting support.
    let _otel_guard = logging::setup_tracing(cli.command.open_telemetry())
        .context("failed to initialize logging")?;

    Box::pin(run_faucet_command(cli)).await
}

#[allow(clippy::too_many_lines)]
async fn run_faucet_command(cli: Cli) -> anyhow::Result<()> {
    // Note: open-telemetry is handled in main.
    match cli.command {
        Command::ApiKey { command } => match command {
            ApiKeyCommand::Create { api_keys_path } => {
                let mut rng = ChaCha20Rng::from_seed(rand::random());
                let key = ApiKey::generate(&mut rng);

                add_api_key_to_file(&api_keys_path, &key).await?;

                println!("{}", key.encode());
            },

            ApiKeyCommand::Remove { api_keys_path, api_key } => {
                let key = ApiKey::decode(&api_key).context("failed to decode API key")?;

                remove_api_key_from_file(&api_keys_path, &key).await?;

                println!("API key removed");
            },

            ApiKeyCommand::List { api_keys_path } => {
                let encoded_keys = list_api_keys_from_file(&api_keys_path).await?;
                if encoded_keys.is_empty() {
                    println!("No API keys found.");
                } else {
                    for key in encoded_keys {
                        println!("{key}");
                    }
                }
            },
        },

        Command::Start {
            funding_service_url,
            config:
                FaucetConfig {
                    node_url,
                    timeout,
                    network,
                    api_keys_path,
                },
            api_bind_port,
            api_public_url,
            no_frontend,
            frontend_bind_port,
            max_claimable_amount,
            pow_secret,
            pow_challenge_lifetime,
            pow_cleanup_interval,
            pow_growth_rate,
            pow_baseline,
            base_amount,
            open_telemetry: _,
            explorer_url,
            decimals,
        } => {
            let node_url = parse_node_url(node_url, &network)?;

            let api_keys = load_api_keys_from_file(&api_keys_path)
                .await
                .context("failed to load the API keys")?;

            // The funding service is the only source of notes, so the faucet refuses to serve
            // without it. Its status also bounds what the faucet may hand out.
            let funding_service = FundingServiceClient::new(funding_service_url.clone(), timeout)?;
            let funding_status = funding_service.status().await.with_context(|| {
                format!("failed to reach the funding service at {funding_service_url}")
            })?;

            let max_claimable_amount = AssetAmount::new(max_claimable_amount)?;
            anyhow::ensure!(
                max_claimable_amount.base_units() <= funding_status.max_amount,
                "the maximum claimable amount {} exceeds the funding service's maximum of {}",
                max_claimable_amount,
                funding_status.max_amount,
            );

            tracing::info!(
                target: COMPONENT,
                {
                    funding_service.url = %funding_service_url,
                    funding_service.version = funding_status.version,
                    funding.account.id = funding_status.account_id,
                    funding.balance = funding_status.balance,
                    funding.max_amount = funding_status.max_amount,
                },
                "Connected to the funding service",
            );

            let rate_limiter_config = PoWRateLimiterConfig {
                challenge_lifetime: pow_challenge_lifetime,
                cleanup_interval: pow_cleanup_interval,
                growth_rate: pow_growth_rate,
                baseline: pow_baseline,
            };
            // The funder account is what the notes are sent from, so its address is the one the
            // frontend shows.
            let (funder_account_id, _) = AccountId::parse(&funding_status.account_id)
                .context("the funding service reported an unparsable account ID")?;

            let metadata = Metadata {
                funder_account_id: FaucetId::new(funder_account_id, network.to_network_id()?),
                decimals,
                explorer_url,
                base_amount,
            };

            // Use a random secret if not explicitly provided.
            let pow_secret = match pow_secret {
                Some(secret) => Sha256::digest(secret.as_bytes()).into(),
                None => rand::random(),
            };

            let api_server = ApiServer::new(
                metadata,
                max_claimable_amount,
                funding_service,
                pow_secret,
                rate_limiter_config,
                &api_keys,
            );

            // Run the API server and, unless disabled, the frontend server, and fail as soon as
            // either of them stops.
            let mut tasks = JoinSet::new();
            let mut tasks_ids = HashMap::new();

            let api_url = Url::parse(&format!("http://0.0.0.0:{api_bind_port}"))?;
            let api_id = tasks.spawn(api_server.serve(api_url.clone())).id();
            tasks_ids.insert(api_id, "api");

            if !no_frontend {
                let frontend_url = Url::parse(&format!("http://0.0.0.0:{frontend_bind_port}"))?;
                let frontend_id = tasks
                    .spawn(serve_frontend(frontend_url, api_public_url, node_url.to_string()))
                    .id();
                tasks_ids.insert(frontend_id, "frontend");
            }

            let (id, result) = match tasks.join_next_with_id().await.expect("a task was spawned") {
                Ok((id, Ok(()))) => (id, Err(anyhow::anyhow!("completed unexpectedly"))),
                Ok((id, Err(err))) => (id, Err(err)),
                Err(join_err) => (join_err.id(), Err(join_err).context("failed to join task")),
            };
            let component = tasks_ids.get(&id).unwrap_or(&"unknown");
            result.context(format!("{component} server failed"))?;
        },
    }

    Ok(())
}

// UTILITIES
// =================================================================================================

/// Loads all API keys from the file at `path`.
async fn load_api_keys_from_file(path: &Path) -> anyhow::Result<Vec<ApiKey>> {
    list_api_keys_from_file(path)
        .await?
        .iter()
        .map(|encoded| ApiKey::decode(encoded).map_err(|e| anyhow::anyhow!(e)))
        .collect()
}

/// Lists all API keys in the file at `path` as encoded strings.
async fn list_api_keys_from_file(path: &Path) -> anyhow::Result<Vec<String>> {
    let contents = tokio::fs::read_to_string(path)
        .await
        .with_context(|| format!("failed to read {}", path.display()))?;

    Ok(contents
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(String::from)
        .collect())
}

/// Adds a single API key to the file at `path`, creating the file if it does not exist.
async fn add_api_key_to_file(path: &Path, key: &ApiKey) -> anyhow::Result<()> {
    // The first key is created before the file exists.
    let mut keys = if path.exists() {
        list_api_keys_from_file(path).await?
    } else {
        Vec::new()
    };
    let encoded = key.encode();
    if !keys.contains(&encoded) {
        keys.push(encoded);
    }

    write_api_keys_to_file(path, &keys).await
}

/// Removes a single API key from the file at `path`.
async fn remove_api_key_from_file(path: &Path, key: &ApiKey) -> anyhow::Result<()> {
    let mut keys = list_api_keys_from_file(path).await?;
    let encoded = key.encode();
    let before = keys.len();
    keys.retain(|existing| existing != &encoded);
    anyhow::ensure!(keys.len() < before, "API key not found in {}", path.display());

    write_api_keys_to_file(path, &keys).await
}

async fn write_api_keys_to_file(path: &Path, keys: &[String]) -> anyhow::Result<()> {
    let mut contents = keys.join("\n");
    contents.push('\n');
    tokio::fs::write(path, contents)
        .await
        .with_context(|| format!("failed to write {}", path.display()))
}

/// Parses the node url from the cli arguments. If an explicit url is provided, it is used.
/// Otherwise, it is derived from the specified network.
fn parse_node_url(node_url: Option<Url>, network: &FaucetNetwork) -> anyhow::Result<Url> {
    if let Some(node_url) = node_url {
        return Ok(node_url);
    }

    let url = network
        .to_rpc_endpoint()
        .context("no node url provided for the custom network")?;

    Url::parse(&url).with_context(|| format!("failed to parse node url: {url}"))
}

// TESTS
// =================================================================================================

#[cfg(test)]
mod tests {
    use std::env::temp_dir;
    use std::process::Stdio;
    use std::str::FromStr;
    use std::time::Duration;

    use clap::Parser;
    use clap::error::ErrorKind;
    use fantoccini::ClientBuilder;
    use miden_protocol::account::AccountId;
    use miden_protocol::address::{Address, NetworkId};
    use miden_protocol::testing::account_id::ACCOUNT_ID_REGULAR_PUBLIC_ACCOUNT_IMMUTABLE_CODE;
    use rand::SeedableRng;
    use serde_json::{Map, json};
    use tokio::io::AsyncBufReadExt;
    use tokio::net::TcpListener;
    use url::Url;
    use uuid::Uuid;

    use crate::funding_service_client::FundingServiceClient;
    use crate::network::FaucetNetwork;
    use crate::testing::stub_funding_service::{STUB_MAX_AMOUNT, serve_stub_funding_service};
    use crate::testing::stub_rpc_api::serve_stub;
    use crate::{Cli, FaucetConfig, run_faucet_command};

    // CLI TESTS
    // ---------------------------------------------------------------------------------------------

    /// The funding service is the only source of notes, so `start` cannot run without its URL.
    #[test]
    fn start_requires_a_funding_service_url() {
        let Err(error) = Cli::try_parse_from(["miden-faucet", "start"]) else {
            panic!("--funding-service-url should be required")
        };
        assert_eq!(error.kind(), ErrorKind::MissingRequiredArgument);
        assert!(error.to_string().contains("--funding-service-url"));
    }

    /// The token's decimals no longer come from a faucet account, so they must be configured.
    #[test]
    fn start_requires_the_token_decimals() {
        let Err(error) = Cli::try_parse_from([
            "miden-faucet",
            "start",
            "--funding-service-url",
            "http://localhost:50401",
        ]) else {
            panic!("--decimals should be required")
        };
        assert_eq!(error.kind(), ErrorKind::MissingRequiredArgument);
        assert!(error.to_string().contains("--decimals"));
    }

    // FUNDING SERVICE TESTS
    // ---------------------------------------------------------------------------------------------

    /// A token request is answered with the note the funding service created.
    #[tokio::test]
    async fn get_tokens_returns_the_funding_services_note() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = Url::from_str(&format!("http://{}", listener.local_addr().unwrap())).unwrap();
        tokio::spawn(async move { serve_stub_funding_service(listener).await.unwrap() });

        let funding_service = FundingServiceClient::new(url, Duration::from_secs(5)).unwrap();

        let status = funding_service.status().await.expect("the stub serves a status");
        assert_eq!(status.max_amount, STUB_MAX_AMOUNT);

        let target = AccountId::try_from(ACCOUNT_ID_REGULAR_PUBLIC_ACCOUNT_IMMUTABLE_CODE).unwrap();
        let funding_response =
            funding_service.request_funds(target, 1_000).await.expect("the stub funds it");

        assert_eq!(
            funding_response
                .note
                .assets()
                .iter()
                .next()
                .unwrap()
                .unwrap_fungible()
                .amount()
                .as_u64(),
            1_000
        );
    }

    // API KEY TESTS
    // ---------------------------------------------------------------------------------------------

    #[tokio::test]
    async fn create_api_key_persists_to_file() {
        let file_path = temp_dir().join(format!("{}.keys", Uuid::new_v4()));

        // Create an API key via the CLI command.
        let result = Box::pin(run_faucet_command(Cli::parse_from([
            "miden-faucet",
            "api-key",
            "create",
            "--file",
            file_path.to_str().unwrap(),
        ])))
        .await;
        assert!(result.is_ok());

        // Verify the key is present in the file.
        let keys = crate::load_api_keys_from_file(&file_path).await.unwrap();
        assert_eq!(keys.len(), 1);
    }

    #[tokio::test]
    async fn list_api_keys_shows_persisted_keys() {
        let file_path = temp_dir().join(format!("{}.keys", Uuid::new_v4()));

        // Create two API keys.
        for _ in 0..2 {
            Box::pin(run_faucet_command(Cli::parse_from([
                "miden-faucet",
                "api-key",
                "create",
                "--file",
                file_path.to_str().unwrap(),
            ])))
            .await
            .unwrap();
        }

        // Verify both keys can be loaded.
        let keys = crate::load_api_keys_from_file(&file_path).await.unwrap();
        assert_eq!(keys.len(), 2);

        // Also verify the list-api-keys command runs without error.
        let result = Box::pin(run_faucet_command(Cli::parse_from([
            "miden-faucet",
            "api-key",
            "list",
            "--file",
            file_path.to_str().unwrap(),
        ])))
        .await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn remove_api_key_deletes_from_file() {
        let file_path = temp_dir().join(format!("{}.keys", Uuid::new_v4()));

        // Create an API key.
        let mut rng = rand::rngs::ChaCha20Rng::from_seed(rand::random());
        let key = crate::api_key::ApiKey::generate(&mut rng);
        crate::add_api_key_to_file(&file_path, &key).await.unwrap();

        // Verify the key exists.
        let keys = crate::load_api_keys_from_file(&file_path).await.unwrap();
        assert_eq!(keys.len(), 1);

        // Remove the key via the CLI command.
        let result = Box::pin(run_faucet_command(Cli::parse_from([
            "miden-faucet",
            "api-key",
            "remove",
            "--file",
            file_path.to_str().unwrap(),
            &key.encode(),
        ])))
        .await;
        assert!(result.is_ok());

        // Verify the key was removed.
        let keys = crate::load_api_keys_from_file(&file_path).await.unwrap();
        assert!(keys.is_empty());
    }

    // INTEGRATION TEST
    // ---------------------------------------------------------------------------------------------

    /// Starts a stub node, a stub funding service, a faucet connected to both, and a chromedriver
    /// to drive the faucet website. It loads the page, requests tokens, and checks that every
    /// request returned a successful status.
    #[tokio::test]
    async fn frontend_request_tokens() {
        let stub_node_url = run_stub_node().await;
        let funding_service_url = run_stub_funding_service().await;
        let website_url = run_faucet_server(stub_node_url, funding_service_url);
        let client = start_fantoccini_client().await;

        // Open the website
        client.goto(website_url.as_str()).await.unwrap();

        let title = client.title().await.unwrap();
        assert_eq!(title, "Miden Faucet");

        let network_id = NetworkId::Testnet;
        let account_id =
            AccountId::try_from(ACCOUNT_ID_REGULAR_PUBLIC_ACCOUNT_IMMUTABLE_CODE).unwrap();
        let address = Address::new(account_id);
        let address_bech32 = address.encode(network_id);

        // Wait for the website to be fully loaded
        client
            .wait()
            .at_most(Duration::from_secs(10))
            .for_element(fantoccini::Locator::Css("#token-amount option"))
            .await
            .unwrap();

        // Fill in the account address
        client
            .find(fantoccini::Locator::Css("#recipient-address"))
            .await
            .unwrap()
            .send_keys(&address_bech32)
            .await
            .unwrap();

        // Select the first asset amount option
        client
            .find(fantoccini::Locator::Css("#token-amount"))
            .await
            .unwrap()
            .click()
            .await
            .unwrap();
        client
            .find(fantoccini::Locator::Css("#token-amount option"))
            .await
            .unwrap()
            .click()
            .await
            .unwrap();

        // Click the public note button
        client
            .find(fantoccini::Locator::Css("#send-button"))
            .await
            .unwrap()
            .click()
            .await
            .unwrap();

        // Execute a script to get all the failed requests
        let script = r"
            let errors = [];
            performance.getEntriesByType('resource').forEach(entry => {
                if (entry.responseStatus && entry.responseStatus >= 400) {
                    errors.push({url: entry.name, status: entry.responseStatus});
                }
            });
            return errors;
        ";
        let failed_requests = client.execute(script, vec![]).await.unwrap();

        // Verify all requests are successful
        assert!(failed_requests.as_array().unwrap().is_empty());

        client.close().await.unwrap();
    }

    // TESTING HELPERS
    // ---------------------------------------------------------------------------------------------

    pub async fn run_stub_node() -> Url {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let listener_addr = listener.local_addr().unwrap();
        let stub_node_url = Url::from_str(&format!("http://{listener_addr}")).unwrap();
        tokio::spawn(async move { serve_stub(listener).await.unwrap() });
        stub_node_url
    }

    /// Starts a faucet against the given stubs and returns its frontend URL.
    fn run_faucet_server(stub_node_url: Url, funding_service_url: Url) -> String {
        // The faucet reads the file on startup, so it has to exist even with no keys in it.
        let api_keys_path = temp_dir().join(format!("{}.keys", Uuid::new_v4()));
        std::fs::write(&api_keys_path, "").unwrap();

        let config = FaucetConfig {
            node_url: Some(stub_node_url),
            timeout: Duration::from_secs(5),
            network: FaucetNetwork::Localhost,
            api_keys_path,
        };
        let api_bind_port = 8000;
        let frontend_url = "http://localhost:8080";

        // Use std::thread to launch faucet - avoids Send requirements
        std::thread::spawn(move || {
            // Create a new runtime for this thread
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("Failed to build runtime");

            // Run the faucet on this thread's runtime
            rt.block_on(async {
                Box::pin(run_faucet_command(Cli {
                    command: crate::Command::Start {
                        config,
                        funding_service_url,
                        decimals: 6,
                        api_bind_port,
                        api_public_url: Url::parse(&format!("http://localhost:{api_bind_port}"))
                            .unwrap(),
                        frontend_bind_port: 8080,
                        no_frontend: false,
                        max_claimable_amount: 1_000_000_000,
                        pow_secret: Some("test".to_string()),
                        pow_challenge_lifetime: Duration::from_secs(30),
                        pow_cleanup_interval: Duration::from_secs(1),
                        pow_growth_rate: 1.0,
                        pow_baseline: 12,
                        base_amount: 100_000,
                        open_telemetry: false,
                        explorer_url: None,
                    },
                }))
                .await
                .expect("failed to start faucet");
            });
        });

        frontend_url.to_string()
    }

    async fn start_fantoccini_client() -> fantoccini::Client {
        // Start chromedriver. This requires having chromedriver and chrome installed
        let chromedriver_port = "57708";
        let mut chromedriver = tokio::process::Command::new("chromedriver")
            .arg(format!("--port={chromedriver_port}"))
            .stdout(Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .expect("failed to start chromedriver");
        let stdout = chromedriver.stdout.take().unwrap();
        tokio::spawn(
            async move { chromedriver.wait().await.expect("chromedriver process failed") },
        );
        // Wait for chromedriver to be running
        let mut reader = tokio::io::BufReader::new(stdout).lines();
        while let Some(line) = reader.next_line().await.unwrap() {
            if line.contains("ChromeDriver was started successfully") {
                break;
            }
        }

        // Start fantoccini client
        ClientBuilder::native()
            .capabilities(
                [(
                    "goog:chromeOptions".to_string(),
                    json!({"args": ["--headless", "--disable-gpu", "--no-sandbox"]}),
                )]
                .into_iter()
                .collect::<Map<_, _>>(),
            )
            .connect(&format!("http://localhost:{chromedriver_port}"))
            .await
            .expect("failed to connect to WebDriver")
    }

    async fn run_stub_funding_service() -> Url {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = Url::from_str(&format!("http://{}", listener.local_addr().unwrap())).unwrap();
        tokio::spawn(async move { serve_stub_funding_service(listener).await.unwrap() });
        url
    }
}
