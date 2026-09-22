mod api;
mod api_key;
mod frontend;
mod funding;
mod logging;
mod network;
#[cfg(test)]
mod testing;

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use anyhow::Context;
use clap::{Parser, Subcommand};
use miden_client::account::component::FungibleFaucet;
use miden_client::account::{AccountFile, AccountId};
use miden_client::rpc::Endpoint;
use miden_client::store::{SettingScope, Store};
use miden_client_sqlite_store::SqliteStore;
use miden_faucet_lib::types::AssetAmount;
use miden_faucet_lib::{Faucet, FaucetAccount, FaucetConfig};
use miden_pow_rate_limiter::PoWRateLimiterConfig;
use rand::SeedableRng;
use rand::rngs::ChaCha20Rng;
use sha2::{Digest, Sha256};
use tokio::sync::mpsc;
use tokio::task::JoinSet;
use url::Url;

use crate::api::{ApiServer, Metadata};
use crate::api_key::ApiKey;
use crate::frontend::serve_frontend;
use crate::funding::FundingClient;
use crate::logging::OpenTelemetry;
use crate::network::FaucetNetwork;

// CONSTANTS
// =================================================================================================

pub const REQUESTS_QUEUE_SIZE: usize = 1000;
const COMPONENT: &str = "miden-faucet-server";
const DEFAULT_STORE_PATH: &str = "faucet_client_store.sqlite3";

const ENV_API_BIND_PORT: &str = "MIDEN_FAUCET_API_BIND_PORT";
const ENV_API_PUBLIC_URL: &str = "MIDEN_FAUCET_API_PUBLIC_URL";
const ENV_FRONTEND_BIND_PORT: &str = "MIDEN_FAUCET_FRONTEND_BIND_PORT";
const ENV_NO_FRONTEND: &str = "MIDEN_FAUCET_NO_FRONTEND";
const ENV_NETWORK: &str = "MIDEN_FAUCET_NETWORK";
const ENV_NODE_URL: &str = "MIDEN_FAUCET_NODE_URL";
const ENV_TIMEOUT: &str = "MIDEN_FAUCET_TIMEOUT";
const ENV_MAX_CLAIMABLE_AMOUNT: &str = "MIDEN_FAUCET_MAX_CLAIMABLE_AMOUNT";
const ENV_REMOTE_TX_PROVER_URL: &str = "MIDEN_FAUCET_REMOTE_TX_PROVER_URL";
const ENV_POW_SECRET: &str = "MIDEN_FAUCET_POW_SECRET";
const ENV_POW_CHALLENGE_LIFETIME: &str = "MIDEN_FAUCET_POW_CHALLENGE_LIFETIME";
const ENV_POW_CLEANUP_INTERVAL: &str = "MIDEN_FAUCET_POW_CLEANUP_INTERVAL";
const ENV_POW_GROWTH_RATE: &str = "MIDEN_FAUCET_POW_GROWTH_RATE";
const ENV_POW_BASELINE: &str = "MIDEN_FAUCET_POW_BASELINE";
const ENV_BASE_AMOUNT: &str = "MIDEN_FAUCET_BASE_AMOUNT";
const ENV_ENABLE_OTEL: &str = "MIDEN_FAUCET_ENABLE_OTEL";
const ENV_STORE: &str = "MIDEN_FAUCET_STORE";
const ENV_EXPLORER_URL: &str = "MIDEN_FAUCET_EXPLORER_URL";
const ENV_FUNDING_SERVICE_URL: &str = "MIDEN_FAUCET_FUNDING_SERVICE_URL";
const ENV_BATCH_SIZE: &str = "MIDEN_FAUCET_BATCH_SIZE";
const ENV_IMPORT_OPERATOR_ACCOUNT_PATH: &str = "MIDEN_FAUCET_IMPORT_OPERATOR_ACCOUNT_PATH";
const ENV_FAUCET_ACCOUNT_ID: &str = "MIDEN_FAUCET_FAUCET_ACCOUNT_ID";

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
    /// Initialize the faucet with an existing account.
    Init {
        #[clap(flatten)]
        config: ClientConfig,

        /// Operator account file to use.
        ///
        /// Must be paired with `--faucet-account-id`, which identifies the faucet account this
        /// operator owns.
        #[arg(
            long = "import",
            value_name = "FILE",
            requires = "faucet_account_id",
            env = ENV_IMPORT_OPERATOR_ACCOUNT_PATH
        )]
        import_operator_account_path: PathBuf,

        /// Account ID of the existing faucet account to use.
        /// It must be a network account and it must be already deployed.
        /// Must be paired with `--import`, which supplies the operator account that owns it.
        #[arg(
            long = "faucet-account-id",
            value_name = "ACCOUNT_ID",
            requires = "import_operator_account_path",
            env = ENV_FAUCET_ACCOUNT_ID
        )]
        faucet_account_id: String,
    },

    /// Manage API keys.
    ApiKey {
        #[command(subcommand)]
        command: ApiKeyCommand,
    },

    /// Start the faucet server
    Start {
        #[clap(flatten)]
        config: ClientConfig,

        /// Base URL of the funding service that emits the notes.
        #[arg(long = "funding-service-url", value_name = "URL", env = ENV_FUNDING_SERVICE_URL)]
        funding_service_url: Url,

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

        /// The maximum number of requests to process in each batch. Each batch is processed in a
        /// single transaction.
        #[arg(long = "batch-size", value_name = "USIZE", default_value = "32", env = ENV_BATCH_SIZE)]
        batch_size: usize,
    },
}

#[derive(Subcommand)]
pub enum ApiKeyCommand {
    /// Generate an API key and persist it to the store.
    ///
    /// Prints out the generated API key to stdout. The key is also stored in the faucet's
    /// database so that it is automatically loaded when the faucet starts.
    Create {
        /// Path to the `SQLite` store.
        #[arg(long = "store", value_name = "FILE", default_value = DEFAULT_STORE_PATH, env = ENV_STORE)]
        store_path: PathBuf,
    },

    /// Remove an API key from the store.
    Remove {
        /// Path to the `SQLite` store.
        #[arg(long = "store", value_name = "FILE", default_value = DEFAULT_STORE_PATH, env = ENV_STORE)]
        store_path: PathBuf,

        /// The API key to remove (encoded string).
        api_key: String,
    },

    /// List all API keys in the store.
    List {
        /// Path to the `SQLite` store.
        #[arg(long = "store", value_name = "FILE", default_value = DEFAULT_STORE_PATH, env = ENV_STORE)]
        store_path: PathBuf,
    },
}

/// Configuration for the faucet client.
#[derive(Parser, Debug, Clone)]
pub struct ClientConfig {
    /// Path to the `SQLite` store.
    #[arg(long = "store", value_name = "FILE", default_value = DEFAULT_STORE_PATH, env = ENV_STORE)]
    store_path: PathBuf,

    /// Timeout for attempting to connect to the node.
    #[arg(long = "timeout", value_name = "DURATION", default_value = "5s", env = ENV_TIMEOUT, value_parser = humantime::parse_duration)]
    timeout: Duration,

    /// Network configuration to use. Options are `devnet`, `testnet`, `localhost` or a custom
    /// network. It is used to display the correct bech32 addresses in the UI.
    #[arg(long = "network", value_name = "NETWORK", default_value = "localhost", env = ENV_NETWORK)]
    network: FaucetNetwork,

    /// Endpoint of the remote transaction prover in the format `<protocol>://<host>[:<port>]`.
    #[arg(long = "remote-tx-prover-url", value_name = "URL", env = ENV_REMOTE_TX_PROVER_URL)]
    remote_tx_prover_url: Option<Url>,

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
        Command::Init {
            config:
                ClientConfig {
                    node_url,
                    timeout,
                    remote_tx_prover_url,
                    network,
                    store_path,
                },
            import_operator_account_path,
            faucet_account_id,
        } => {
            let node_endpoint = parse_node_endpoint(node_url, &network)?;

            let operator_account_data = AccountFile::read(import_operator_account_path)
                .context("failed to read operator account data from file")?;
            let operator_secret = operator_account_data
                .auth_secret_keys
                .first()
                .context("auth secret key is required")?
                .clone();
            let (faucet_account_id, _) = AccountId::parse(&faucet_account_id)
                .context("failed to parse faucet account id")?;
            println!(
                "Using existing faucet account {} owned by operator account {}",
                faucet_account_id.to_hex(),
                operator_account_data.account.id(),
            );
            let faucet_account = FaucetAccount::Existing(faucet_account_id);
            let operator_account = operator_account_data.account;

            let faucet_config = FaucetConfig {
                store_path,
                node_endpoint,
                network_id: network.to_network_id()?,
                timeout,
                remote_tx_prover_url,
            };
            Box::pin(Faucet::init(
                &faucet_config,
                faucet_account,
                &operator_secret,
                operator_account,
            ))
            .await
            .context("failed to initialize faucet")?;

            println!("Faucet account successfully initialized");
        },

        Command::ApiKey { command } => match command {
            ApiKeyCommand::Create { store_path } => {
                let store = SqliteStore::new(store_path).await.context("failed to open store")?;
                let mut rng = ChaCha20Rng::from_seed(rand::random());
                let key = ApiKey::generate(&mut rng);

                add_api_key_to_store(&store, &key).await?;

                println!("{}", key.encode());
            },

            ApiKeyCommand::Remove { store_path, api_key } => {
                let store = SqliteStore::new(store_path).await.context("failed to open store")?;
                let key = ApiKey::decode(&api_key).context("failed to decode API key")?;

                remove_api_key_from_store(&store, &key).await?;

                println!("API key removed");
            },

            ApiKeyCommand::List { store_path } => {
                let store = SqliteStore::new(store_path).await.context("failed to open store")?;
                let encoded_keys = list_api_keys_from_store(&store).await?;
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
                ClientConfig {
                    node_url,
                    timeout,
                    remote_tx_prover_url,
                    network,
                    store_path,
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
            batch_size,
        } => {
            let node_endpoint = parse_node_endpoint(node_url, &network)?;
            let config = FaucetConfig {
                store_path: store_path.clone(),
                node_endpoint: node_endpoint.clone(),
                network_id: network.to_network_id()?,
                timeout,
                remote_tx_prover_url,
            };
            let mut faucet = Faucet::load(&config).await.context("failed to load faucet")?;
            let issuance_receiver = faucet.subscribe_issuance();
            let fee_parameters = faucet
                .fee_parameters()
                .await
                .context("failed to read the chain's fee parameters")?;

            tracing::info!(
                target: COMPONENT,
                {
                    faucet.account.id = %faucet.faucet_id().account_id,
                    operator.account.id = %faucet.operator_id(),
                    node.endpoint = %node_endpoint,
                    fee.verification_base_fee = fee_parameters.verification_base_fee(),
                    batch_size
                },
                "Faucet loaded",
            );

            let store =
                Arc::new(SqliteStore::new(store_path).await.context("failed to create store")?);

            // Maximum of 1000 requests in-queue at once. Overflow is rejected for faster feedback.
            let (tx_mint_requests, rx_mint_requests) = mpsc::channel(REQUESTS_QUEUE_SIZE);

            let api_keys = load_api_keys_from_store(&store)
                .await
                .context("failed to load API keys from store")?;

            // The funding service is the only source of notes, so the faucet refuses to serve
            // without it. Its status also bounds what the faucet may hand out.
            let funding = FundingClient::new(funding_service_url.clone(), timeout)?;
            let funding_status = funding.status().await.with_context(|| {
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
            let faucet_account = faucet.faucet_account().await.map_err(|error| *error)?;
            let token_metadata = FungibleFaucet::try_from(faucet_account.storage())?;
            let max_supply = AssetAmount::new(token_metadata.max_supply().as_u64())?;
            let decimals = token_metadata.decimals();

            let metadata = Metadata {
                id: faucet.faucet_id(),
                max_supply,
                decimals,
                explorer_url,
                base_amount,
            };

            // Use a random secret if not explicitly provided.
            let pow_secret = match pow_secret {
                Some(secret) => Sha256::digest(secret.as_bytes()).into(),
                None => rand::random(),
            };

            // Requests now go to the funding service, but the sender is kept alive so the idle
            // mint worker does not see a closed channel and shut the faucet down.
            let _tx_mint_requests = tx_mint_requests;
            let api_server = ApiServer::new(
                metadata,
                max_claimable_amount,
                funding,
                pow_secret,
                rate_limiter_config,
                &api_keys,
                issuance_receiver,
            );

            // Use select to concurrently:
            // - Run and wait for the faucet (on current thread)
            // - Run and wait for API server (in a spawned task)
            // - Run and wait for frontend server (in a spawned task, only if set)
            let faucet_future = faucet.run(rx_mint_requests, batch_size);

            let mut tasks = JoinSet::new();
            let mut tasks_ids = HashMap::new();

            let api_url = Url::parse(&format!("http://0.0.0.0:{api_bind_port}"))?;
            let api_id = tasks.spawn(api_server.serve(api_url.clone())).id();
            tasks_ids.insert(api_id, "api");

            if !no_frontend {
                let frontend_url = Url::parse(&format!("http://0.0.0.0:{frontend_bind_port}"))?;
                let frontend_id = tasks
                    .spawn(serve_frontend(frontend_url, api_public_url, node_endpoint.to_string()))
                    .id();
                tasks_ids.insert(frontend_id, "frontend");
            }

            tokio::select! {
                serve_result = tasks.join_next_with_id() => {
                    let (id, err) = match serve_result.unwrap() {
                        Ok((id, Ok(_))) => (id, Err(anyhow::anyhow!("completed unexpectedly"))),
                        Ok((id, Err(err))) => (id, Err(err)),
                        Err(join_err) => (join_err.id(), Err(join_err).context("failed to join task")),
                    };
                    let component = tasks_ids.get(&id).unwrap_or(&"unknown");
                    err.context(format!("{component} server failed"))
                },
                faucet_result = faucet_future => {
                    // Faucet completed, return its result
                    faucet_result.context("faucet failed")
                },
            }?;
        },
    }

    Ok(())
}

// UTILITIES
// =================================================================================================

/// Loads all API keys from the store's settings table.
async fn load_api_keys_from_store(store: &SqliteStore) -> anyhow::Result<Vec<ApiKey>> {
    list_api_keys_from_store(store)
        .await?
        .iter()
        .map(|encoded| ApiKey::decode(encoded).map_err(|e| anyhow::anyhow!(e)))
        .collect()
}

/// Lists all API keys from the store as encoded strings.
async fn list_api_keys_from_store(store: &SqliteStore) -> anyhow::Result<Vec<String>> {
    let all_keys = store
        .list_setting_keys(SettingScope::User)
        .await
        .context("failed to list settings")?;
    Ok(all_keys
        .into_iter()
        .filter_map(|key| key.strip_prefix(api_key::API_KEY_SETTING_PREFIX).map(String::from))
        .collect())
}

/// Stores a single API key in the settings table.
async fn add_api_key_to_store(store: &SqliteStore, key: &ApiKey) -> anyhow::Result<()> {
    let setting_key = format!("{}{}", api_key::API_KEY_SETTING_PREFIX, key.encode());
    store
        .set_setting(SettingScope::User, setting_key, vec![])
        .await
        .context("failed to store API key")
}

/// Removes a single API key from the settings table.
///
/// Fails if the key is not present in the store, so a typo does not report a successful removal.
async fn remove_api_key_from_store(store: &SqliteStore, key: &ApiKey) -> anyhow::Result<()> {
    let setting_key = format!("{}{}", api_key::API_KEY_SETTING_PREFIX, key.encode());
    let removed = store
        .remove_setting(SettingScope::User, setting_key)
        .await
        .context("failed to remove API key")?;
    anyhow::ensure!(removed, "API key not found in the store");
    Ok(())
}

/// Parses the node endpoint from the cli arguments. If an explicit url is provided, it is used.
/// Otherwise, it is derived from the specified network.
fn parse_node_endpoint(node_url: Option<Url>, network: &FaucetNetwork) -> anyhow::Result<Endpoint> {
    let url = if let Some(node_url) = node_url {
        node_url.to_string()
    } else {
        network
            .to_rpc_endpoint()
            .context("no node url provided for the custom network")?
    };

    Endpoint::try_from(url.as_str())
        .map_err(anyhow::Error::msg)
        .with_context(|| format!("failed to parse node url: {url}"))
}

// TESTS
// =================================================================================================

#[cfg(test)]
mod tests {
    use std::env::temp_dir;
    use std::str::FromStr;
    use std::time::Duration;

    use clap::Parser;
    use clap::error::ErrorKind;
    use miden_client::account::{AccountFile, AccountId};
    use miden_client::testing::account_id::ACCOUNT_ID_REGULAR_PUBLIC_ACCOUNT_IMMUTABLE_CODE;
    use miden_client_sqlite_store::SqliteStore;
    use rand::SeedableRng;
    use tokio::net::TcpListener;
    use url::Url;
    use uuid::Uuid;

    use crate::funding::FundingClient;
    use crate::testing::stub_funding_service::{STUB_MAX_AMOUNT, serve_stub_funding_service};
    use crate::testing::stub_rpc_api::serve_stub;
    use crate::{Cli, run_faucet_command};

    // CLI TESTS
    // ---------------------------------------------------------------------------------------------

    const TEST_FAUCET_ACCOUNT_ID: &str = "0xf640ba4c3fe40e710eb82764ff48e9";

    /// The funding service is the only source of notes, so `start` cannot run without its URL.
    #[test]
    fn start_requires_a_funding_service_url() {
        let Err(error) = Cli::try_parse_from(["miden-faucet", "start"]) else {
            panic!("--funding-service-url should be required")
        };
        assert_eq!(error.kind(), ErrorKind::MissingRequiredArgument);
        assert!(error.to_string().contains("--funding-service-url"));
    }

    // FUNDING SERVICE TESTS
    // ---------------------------------------------------------------------------------------------

    /// A token request is answered with the note the funding service created.
    #[tokio::test]
    async fn get_tokens_returns_the_funding_services_note() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = Url::from_str(&format!("http://{}", listener.local_addr().unwrap())).unwrap();
        tokio::spawn(async move { serve_stub_funding_service(listener).await.unwrap() });

        let funding = FundingClient::new(url, Duration::from_secs(5)).unwrap();

        let status = funding.status().await.expect("the stub serves a status");
        assert_eq!(status.max_amount, STUB_MAX_AMOUNT);

        let target = AccountId::try_from(ACCOUNT_ID_REGULAR_PUBLIC_ACCOUNT_IMMUTABLE_CODE).unwrap();
        let funded = funding.request_funds(target, 1_000).await.expect("the stub funds it");

        assert_eq!(
            funded.note.assets().iter().next().unwrap().unwrap_fungible().amount().as_u64(),
            1_000
        );
    }

    /// Parses an `init` invocation, with `args` appended to the fixed prefix.
    fn parse_init(args: &[&str]) -> Result<Cli, clap::Error> {
        let mut command_args = vec!["miden-faucet", "init"];
        command_args.extend_from_slice(args);
        Cli::try_parse_from(command_args)
    }

    /// `--import` and `--faucet-account-id` are all-or-nothing: each requires the other.
    #[test]
    fn init_import_requires_faucet_account_id() {
        let Err(error) = parse_init(&["--import", "operator.mac"]) else {
            panic!("--faucet-account-id should be required")
        };
        assert_eq!(error.kind(), ErrorKind::MissingRequiredArgument);
        assert!(error.to_string().contains("--faucet-account-id"));
    }

    #[test]
    fn init_faucet_account_id_requires_import() {
        let Err(error) = parse_init(&["--faucet-account-id", TEST_FAUCET_ACCOUNT_ID]) else {
            panic!("--import should be required")
        };
        assert_eq!(error.kind(), ErrorKind::MissingRequiredArgument);

        // Only `--import` is missing. The token metadata must NOT be demanded here: it conflicts
        // with `--faucet-account-id`, so asking for it would make the request unsatisfiable.
        let message = error.to_string();
        assert!(message.contains("--import"), "expected --import in: {message}");
        for arg in ["--token-symbol", "--decimals", "--max-supply"] {
            assert!(!message.contains(arg), "did not expect {arg} in: {message}");
        }
    }

    /// `--import` and `--faucet-account-id` together take the `FaucetAccount::Existing` path: the
    /// operator account is read from the file and the faucet account is fetched from the node
    /// instead of being created.
    ///
    /// The stub node serves no accounts, so the run ends at that fetch. Failing there rather than
    /// earlier is what shows both flags were honoured: the operator file was read, the faucet id
    /// parsed, and `Existing` chosen over `New`.
    #[tokio::test]
    async fn init_with_imported_operator_account() {
        let stub_node_url = run_stub_node().await;
        let store_path = temp_dir().join(format!("{}.sqlite3", Uuid::new_v4()));

        // Write out an operator account file for `--import` to read.
        let operator_account_path = temp_dir().join(format!("{}.mac", Uuid::new_v4()));
        let (operator_account, operator_secret) =
            miden_faucet_lib::create_faucet_operator_account()
                .expect("failed to create operator account");
        AccountFile::new(operator_account, vec![operator_secret])
            .write(&operator_account_path)
            .expect("failed to write operator account file");

        let result = Box::pin(run_faucet_command(Cli::parse_from([
            "miden-faucet",
            "init",
            "--import",
            operator_account_path.to_str().unwrap(),
            "--faucet-account-id",
            TEST_FAUCET_ACCOUNT_ID,
            "--node-url",
            stub_node_url.to_string().as_str(),
            "--store",
            store_path.to_str().unwrap(),
        ])))
        .await;

        let error = format!("{:#}", result.expect_err("stub node serves no faucet account"));
        assert!(
            error.contains("failed to fetch faucet account"),
            "expected the faucet account fetch to fail, got: {error}"
        );
    }

    /// `start` reaches the funding service first, then fails on the uninitialised store.
    #[tokio::test]
    async fn serve_fails_without_init() {
        let stub_node_url = run_stub_node().await;
        let funding_service_url = run_stub_funding_service().await;
        let store_path = temp_dir().join(format!("{}.sqlite3", Uuid::new_v4()));

        let result = Box::pin(run_faucet_command(Cli::parse_from([
            "miden-faucet",
            "start",
            "--api-bind-port",
            "8000",
            "--frontend-bind-port",
            "8081",
            "--node-url",
            stub_node_url.to_string().as_str(),
            "--funding-service-url",
            funding_service_url.to_string().as_str(),
            "--max-claimable-amount",
            "1000",
            "--store",
            store_path.to_str().unwrap(),
        ])))
        .await;
        let error = format!("{:#}", result.expect_err("the store holds no faucet account"));
        assert!(error.contains("failed to load faucet"), "unexpected failure: {error}");
    }

    // API KEY TESTS
    // ---------------------------------------------------------------------------------------------

    #[tokio::test]
    async fn create_api_key_persists_to_store() {
        let store_path = temp_dir().join(format!("{}.sqlite3", Uuid::new_v4()));

        // Create an API key via the CLI command.
        let result = Box::pin(run_faucet_command(Cli::parse_from([
            "miden-faucet",
            "api-key",
            "create",
            "--store",
            store_path.to_str().unwrap(),
        ])))
        .await;
        assert!(result.is_ok());

        // Verify the key is present in the store.
        let store = SqliteStore::new(store_path).await.unwrap();
        let keys = crate::load_api_keys_from_store(&store).await.unwrap();
        assert_eq!(keys.len(), 1);
    }

    #[tokio::test]
    async fn list_api_keys_shows_persisted_keys() {
        let store_path = temp_dir().join(format!("{}.sqlite3", Uuid::new_v4()));

        // Create two API keys.
        for _ in 0..2 {
            Box::pin(run_faucet_command(Cli::parse_from([
                "miden-faucet",
                "api-key",
                "create",
                "--store",
                store_path.to_str().unwrap(),
            ])))
            .await
            .unwrap();
        }

        // Verify both keys can be loaded.
        let store = SqliteStore::new(store_path.clone()).await.unwrap();
        let keys = crate::load_api_keys_from_store(&store).await.unwrap();
        assert_eq!(keys.len(), 2);

        // Also verify the list-api-keys command runs without error.
        let result = Box::pin(run_faucet_command(Cli::parse_from([
            "miden-faucet",
            "api-key",
            "list",
            "--store",
            store_path.to_str().unwrap(),
        ])))
        .await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn remove_api_key_deletes_from_store() {
        let store_path = temp_dir().join(format!("{}.sqlite3", Uuid::new_v4()));

        // Create an API key.
        let store = SqliteStore::new(store_path.clone()).await.unwrap();
        let mut rng = rand::rngs::ChaCha20Rng::from_seed(rand::random());
        let key = crate::api_key::ApiKey::generate(&mut rng);
        crate::add_api_key_to_store(&store, &key).await.unwrap();

        // Verify the key exists.
        let keys = crate::load_api_keys_from_store(&store).await.unwrap();
        assert_eq!(keys.len(), 1);

        // Remove the key via the CLI command.
        let result = Box::pin(run_faucet_command(Cli::parse_from([
            "miden-faucet",
            "api-key",
            "remove",
            "--store",
            store_path.to_str().unwrap(),
            &key.encode(),
        ])))
        .await;
        assert!(result.is_ok());

        // Verify the key was removed.
        let store = SqliteStore::new(store_path).await.unwrap();
        let keys = crate::load_api_keys_from_store(&store).await.unwrap();
        assert!(keys.is_empty());
    }

    // TESTING HELPERS
    // ---------------------------------------------------------------------------------------------

    async fn run_stub_funding_service() -> Url {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = Url::from_str(&format!("http://{}", listener.local_addr().unwrap())).unwrap();
        tokio::spawn(async move { serve_stub_funding_service(listener).await.unwrap() });
        url
    }

    pub async fn run_stub_node() -> Url {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let listener_addr = listener.local_addr().unwrap();
        let stub_node_url = Url::from_str(&format!("http://{listener_addr}")).unwrap();
        tokio::spawn(async move { serve_stub(listener).await.unwrap() });
        stub_node_url
    }
}
