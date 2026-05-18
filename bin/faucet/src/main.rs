mod api;
mod api_key;
mod frontend;
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
use miden_client::account::component::{AuthControlled, BasicFungibleFaucet};
use miden_client::account::{
    Account,
    AccountBuilder,
    AccountFile,
    AccountStorageMode,
    AccountType,
};
use miden_client::asset::TokenSymbol;
use miden_client::auth::{AuthSchemeId, AuthSecretKey, AuthSingleSig};
use miden_client::crypto::RandomCoin;
use miden_client::crypto::rpo_falcon512::SecretKey;
use miden_client::note_transport::grpc::GrpcNoteTransportClient;
use miden_client::rpc::Endpoint;
use miden_client::store::Store;
use miden_client::{Felt, Word};
use miden_client_sqlite_store::SqliteStore;
use miden_faucet_lib::types::AssetAmount;
use miden_faucet_lib::{Faucet, FaucetConfig};
use miden_pow_rate_limiter::PoWRateLimiterConfig;
use rand::{Rng, SeedableRng};
use rand_chacha::ChaCha20Rng;
use tokio::sync::mpsc;
use tokio::task::JoinSet;
use url::Url;

use crate::api::{ApiServer, Metadata};
use crate::api_key::ApiKey;
use crate::frontend::serve_frontend;
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
const ENV_BATCH_SIZE: &str = "MIDEN_FAUCET_BATCH_SIZE";
const ENV_IMPORT_ACCOUNT_PATH: &str = "MIDEN_FAUCET_IMPORT_ACCOUNT_PATH";
const ENV_DEPLOY: &str = "MIDEN_FAUCET_DEPLOY";
const ENV_TOKEN_SYMBOL: &str = "MIDEN_FAUCET_TOKEN_SYMBOL";
const ENV_DECIMALS: &str = "MIDEN_FAUCET_DECIMALS";
const ENV_MAX_SUPPLY: &str = "MIDEN_FAUCET_MAX_SUPPLY";
const ENV_NOTE_TRANSPORT_URL: &str = "MIDEN_FAUCET_NOTE_TRANSPORT_URL";

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
    /// Initialize the faucet with a new or existing account.
    Init {
        #[clap(flatten)]
        config: ClientConfig,

        /// Symbol of the new token.
        #[arg(
            short,
            long,
            value_name = "STRING",
            required_unless_present = "import_account_path", 
            env = ENV_TOKEN_SYMBOL
        )]
        token_symbol: Option<String>,

        /// Decimals of the new token.
        #[arg(short, long, value_name = "U8", required_unless_present = "import_account_path", env = ENV_DECIMALS)]
        decimals: Option<u8>,

        /// Max supply of the new token (in base units).
        #[arg(short, long, value_name = "U64", required_unless_present = "import_account_path", env = ENV_MAX_SUPPLY)]
        max_supply: Option<u64>,

        /// Set an existing faucet account file to use, instead of creating a new account.
        #[arg(long = "import", value_name = "FILE", conflicts_with_all = ["token_symbol", "decimals", "max_supply"], env = ENV_IMPORT_ACCOUNT_PATH)]
        import_account_path: Option<PathBuf>,

        /// Whether to deploy the faucet account to the node.
        #[arg(long, value_name = "BOOL", default_value_t = false, env = ENV_DEPLOY)]
        deploy: bool,
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
        #[arg(long = "pow-secret", value_name = "STRING", default_value = "", env = ENV_POW_SECRET)]
        pow_secret: String,

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

        /// Note transport endpoint. If not set, no note transport will be used.
        #[arg(long = "note-transport-url", value_name = "URL", env = ENV_NOTE_TRANSPORT_URL)]
        note_transport_url: Option<Url>,
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
            token_symbol,
            decimals,
            max_supply,
            import_account_path,
            deploy,
        } => {
            let (account, secret) = if let Some(account_path) = import_account_path {
                // Import existing faucet account
                let account_data = AccountFile::read(account_path)
                    .context("failed to read account data from file")?;
                let secret = account_data
                    .auth_secret_keys
                    .first()
                    .context("auth secret key is required")?
                    .clone();
                (account_data.account, secret)
            } else {
                println!("Generating new faucet account. This may take a few seconds...");
                let token_symbol =
                    token_symbol.expect("token_symbol should be present when not importing");
                let decimals = decimals.expect("decimals should be present when not importing");
                let max_supply =
                    max_supply.expect("max_supply should be present when not importing");
                create_faucet_account(token_symbol.as_str(), max_supply, decimals)?
            };
            let node_endpoint = parse_node_endpoint(node_url, &network)?;
            let faucet_config = FaucetConfig {
                store_path,
                node_endpoint,
                network_id: network.to_network_id()?,
                timeout,
                remote_tx_prover_url,
            };
            Box::pin(Faucet::init(&faucet_config, account, &secret, deploy))
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
            note_transport_url,
        } => {
            let node_endpoint = parse_node_endpoint(node_url, &network)?;
            let config = FaucetConfig {
                store_path: store_path.clone(),
                node_endpoint: node_endpoint.clone(),
                network_id: network.to_network_id()?,
                timeout,
                remote_tx_prover_url,
            };
            let faucet = Faucet::load(&config).await.context("failed to load faucet")?;
            let issuance_receiver = faucet.subscribe_issuance();

            let store =
                Arc::new(SqliteStore::new(store_path).await.context("failed to create store")?);

            // Maximum of 1000 requests in-queue at once. Overflow is rejected for faster feedback.
            let (tx_mint_requests, rx_mint_requests) = mpsc::channel(REQUESTS_QUEUE_SIZE);

            let api_keys = load_api_keys_from_store(&store)
                .await
                .context("failed to load API keys from store")?;

            let max_claimable_amount = AssetAmount::new(max_claimable_amount)?;
            let rate_limiter_config = PoWRateLimiterConfig {
                challenge_lifetime: pow_challenge_lifetime,
                cleanup_interval: pow_cleanup_interval,
                growth_rate: pow_growth_rate,
                baseline: pow_baseline,
            };
            let faucet_account = faucet.faucet_account().await?;
            let faucet_component = BasicFungibleFaucet::try_from(&faucet_account)?;
            let max_supply = AssetAmount::new(faucet_component.max_supply().as_canonical_u64())?;
            let decimals = faucet_component.decimals();

            let note_transport_client = note_transport_url.as_ref().map(|url| {
                Arc::new(GrpcNoteTransportClient::new(
                    url.to_string(),
                    timeout.as_millis().try_into().expect("timeout should fit into u64"),
                ))
            });

            let metadata = Metadata {
                id: faucet.faucet_id(),
                max_supply,
                decimals,
                explorer_url,
                base_amount,
                note_transport_url,
            };

            // We keep a channel sender open in the main thread to avoid the faucet closing before
            // servers can propagate any errors.
            let tx_mint_requests_clone = tx_mint_requests.clone();
            let api_server = ApiServer::new(
                metadata,
                max_claimable_amount,
                tx_mint_requests_clone,
                pow_secret.as_str(),
                rate_limiter_config,
                &api_keys,
                store,
                note_transport_client,
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
    let all_keys = store.list_setting_keys().await.context("failed to list settings")?;
    Ok(all_keys
        .into_iter()
        .filter_map(|key| key.strip_prefix(api_key::API_KEY_SETTING_PREFIX).map(String::from))
        .collect())
}

/// Stores a single API key in the settings table.
async fn add_api_key_to_store(store: &SqliteStore, key: &ApiKey) -> anyhow::Result<()> {
    let setting_key = format!("{}{}", api_key::API_KEY_SETTING_PREFIX, key.encode());
    store.set_setting(setting_key, vec![]).await.context("failed to store API key")
}

/// Removes a single API key from the settings table.
async fn remove_api_key_from_store(store: &SqliteStore, key: &ApiKey) -> anyhow::Result<()> {
    let setting_key = format!("{}{}", api_key::API_KEY_SETTING_PREFIX, key.encode());
    store.remove_setting(setting_key).await.context("failed to remove API key")
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

/// Creates a new faucet account from the given parameters.
fn create_faucet_account(
    token_symbol: &str,
    max_supply: u64,
    decimals: u8,
) -> anyhow::Result<(Account, AuthSecretKey)> {
    let mut rng = ChaCha20Rng::from_seed(rand::random());
    let secret = {
        let auth_seed: [u64; 4] = rng.random();
        let rng_seed = Word::from(auth_seed.map(Felt::new));
        SecretKey::with_rng(&mut RandomCoin::new(rng_seed))
    };

    let symbol = TokenSymbol::try_from(token_symbol).context("failed to parse token symbol")?;
    let max_supply = Felt::try_from(max_supply)
        .map_err(anyhow::Error::msg)
        .context("max supply value is greater than or equal to the field modulus")?;
    let auth_component = AuthSingleSig::new(
        secret.public_key().to_commitment().into(),
        AuthSchemeId::Falcon512Poseidon2,
    );

    let account = AccountBuilder::new(rng.random())
        .account_type(AccountType::FungibleFaucet)
        .storage_mode(AccountStorageMode::Public)
        .with_component(BasicFungibleFaucet::new(symbol, decimals, max_supply)?)
        .with_component(AuthControlled::allow_all())
        .with_auth_component(auth_component)
        .build()
        .context("failed to create basic fungible faucet account")?;

    Ok((account, AuthSecretKey::Falcon512Poseidon2(secret)))
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
    use fantoccini::ClientBuilder;
    use miden_client::account::{AccountFile, AccountId, Address, NetworkId};
    use miden_client_sqlite_store::SqliteStore;
    use rand::SeedableRng;
    use serde_json::{Map, json};
    use tokio::io::AsyncBufReadExt;
    use tokio::net::TcpListener;
    use url::Url;
    use uuid::Uuid;

    use crate::network::FaucetNetwork;
    use crate::testing::stub_rpc_api::serve_stub;
    use crate::{Cli, ClientConfig, create_faucet_account, run_faucet_command};

    // CLI TESTS
    // ---------------------------------------------------------------------------------------------

    #[tokio::test]
    async fn init_with_new_token() {
        let stub_node_url = run_stub_node().await;
        let store_path = temp_dir().join(format!("{}.sqlite3", Uuid::new_v4()));
        let result = Box::pin(run_faucet_command(Cli::parse_from([
            "miden-faucet",
            "init",
            "--token-symbol",
            "TEST",
            "--decimals",
            "6",
            "--max-supply",
            "100000000000000000",
            "--node-url",
            stub_node_url.to_string().as_str(),
            "--store",
            store_path.to_str().unwrap(),
        ])))
        .await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn init_importing_account_file() {
        let stub_node_url = run_stub_node().await;
        let store_path = temp_dir().join(format!("{}.sqlite3", Uuid::new_v4()));
        let account_path = temp_dir().join("test_account.mac");
        let (account, secret) = create_faucet_account("TEST", 100_000_000, 3).unwrap();
        let account_data = AccountFile::new(account, vec![secret]);
        account_data.write(&account_path).unwrap();

        let result = Box::pin(run_faucet_command(Cli::parse_from([
            "miden-faucet",
            "init",
            "--import",
            account_path.to_str().unwrap(),
            "--node-url",
            stub_node_url.to_string().as_str(),
            "--store",
            store_path.to_str().unwrap(),
        ])))
        .await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn init_with_deploy() {
        let stub_node_url = run_stub_node().await;
        let store_path = temp_dir().join(format!("{}.sqlite3", Uuid::new_v4()));
        let result = Box::pin(run_faucet_command(Cli::parse_from([
            "miden-faucet",
            "init",
            "--token-symbol",
            "TEST",
            "--decimals",
            "6",
            "--max-supply",
            "100000000000000000",
            "--node-url",
            stub_node_url.to_string().as_str(),
            "--store",
            store_path.to_str().unwrap(),
            "--deploy",
        ])))
        .await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn serve_fails_without_init() {
        let stub_node_url = run_stub_node().await;
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
            "--store",
            store_path.to_str().unwrap(),
        ])))
        .await;
        assert!(result.is_err());
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
        let mut rng = rand_chacha::ChaCha20Rng::from_seed(rand::random());
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

    // INTEGRATION TEST
    // ---------------------------------------------------------------------------------------------

    /// This test starts a stub node, a faucet connected to the stub node, and a chromedriver
    /// to test the faucet website. It then loads the website, mints tokens, and checks that all the
    /// requests returned status 200.
    #[tokio::test]
    async fn frontend_mint_tokens() {
        let stub_node_url = run_stub_node().await;
        let website_url = run_faucet_server(stub_node_url).await;
        let client = start_fantoccini_client().await;

        // Open the website
        client.goto(website_url.as_str()).await.unwrap();

        let title = client.title().await.unwrap();
        assert_eq!(title, "Miden Faucet");

        let network_id = NetworkId::Testnet;
        let account_id = AccountId::try_from(0).unwrap();
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
            .find(fantoccini::Locator::Css("#send-public-button"))
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
        tokio::spawn({
            let stub_node_url = stub_node_url.clone();
            async move { serve_stub(&stub_node_url).await.unwrap() }
        });
        stub_node_url
    }

    async fn run_faucet_server(stub_node_url: Url) -> String {
        let config = ClientConfig {
            node_url: Some(stub_node_url.clone()),
            timeout: Duration::from_millis(5000),
            network: FaucetNetwork::Localhost,
            store_path: temp_dir().join(format!("{}.sqlite3", Uuid::new_v4())),
            remote_tx_prover_url: None,
        };

        Box::pin(run_faucet_command(Cli {
            command: crate::Command::Init {
                config: config.clone(),
                token_symbol: Some("TEST".to_owned()),
                decimals: Some(6),
                max_supply: Some(1_000_000_000_000),
                import_account_path: None,
                deploy: false,
            },
        }))
        .await
        .expect("failed to create faucet account");

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
                        api_bind_port,
                        api_public_url: Url::parse(&format!("http://localhost:{api_bind_port}"))
                            .unwrap(),
                        frontend_bind_port: 8080,
                        no_frontend: false,
                        max_claimable_amount: 1_000_000_000,
                        pow_secret: "test".to_string(),
                        pow_challenge_lifetime: Duration::from_secs(30),
                        pow_cleanup_interval: Duration::from_secs(1),
                        pow_growth_rate: 1.0,
                        pow_baseline: 12,
                        base_amount: 100_000,
                        open_telemetry: false,
                        explorer_url: None,
                        batch_size: 8,
                        note_transport_url: None,
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
}
