//! End-to-end test against a real node and a real funding service.
//!
//! The network is not started here. `make test-e2e` brings up the node's compose stack, waits for
//! it, runs this test and tears it down again. The test is ignored by default so that `make test`
//! keeps working without Docker.
//!
//! Point it at an existing network with `MIDEN_FAUCET_E2E_NODE_URL` and
//! `MIDEN_FAUCET_E2E_FUNDING_SERVICE_URL`.

use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use miden_faucet_lib::requests::{GetPowResponse, GetTokensResponse};
use miden_pow_rate_limiter::Challenge;
use miden_protocol::account::AccountId;
use miden_protocol::testing::account_id::ACCOUNT_ID_REGULAR_PUBLIC_ACCOUNT_IMMUTABLE_CODE;

/// Where `make test-e2e` publishes the network. These are not the node's usual ports: the stack is
/// moved aside so it does not clash with a network the developer is already running.
const DEFAULT_NODE_URL: &str = "http://127.0.0.1:57391";
const DEFAULT_FUNDING_SERVICE_URL: &str = "http://127.0.0.1:50501";

/// A port the faucet is unlikely to share with a faucet the developer is already running.
const FAUCET_API_PORT: u16 = 18000;

/// How long the network gets to answer before the test gives up. The stack builds a genesis block
/// and starts six processes, so the first request can take a while on a cold runner.
const NETWORK_TIMEOUT: Duration = Duration::from_secs(180);

/// How long the faucet gets to bind its API once the network is up.
const FAUCET_TIMEOUT: Duration = Duration::from_secs(30);

/// Requesting the whole allowance would empty the funding account for later runs.
const REQUESTED_AMOUNT: u64 = 100;

/// Requests tokens the way the frontend does, and checks that the funding service answers with a
/// note for the recipient.
#[tokio::test]
#[ignore = "needs a node and a funding service, run it with `make test-e2e`"]
async fn a_token_request_reaches_the_funding_service() {
    let node_url = url_from_env("MIDEN_FAUCET_E2E_NODE_URL", DEFAULT_NODE_URL);
    let funding_service_url =
        url_from_env("MIDEN_FAUCET_E2E_FUNDING_SERVICE_URL", DEFAULT_FUNDING_SERVICE_URL);
    let client = reqwest::Client::new();

    wait_for(&client, &format!("{funding_service_url}/status"), NETWORK_TIMEOUT)
        .await
        .expect("the funding service should be reachable, is the network up?");

    let faucet_url = format!("http://127.0.0.1:{FAUCET_API_PORT}");
    let _faucet = Faucet::start(&node_url, &funding_service_url);
    wait_for(&client, &format!("{faucet_url}/get_metadata"), FAUCET_TIMEOUT)
        .await
        .expect("the faucet should serve its metadata");

    let recipient = AccountId::try_from(ACCOUNT_ID_REGULAR_PUBLIC_ACCOUNT_IMMUTABLE_CODE)
        .expect("the test account id is valid")
        .to_hex();

    // The faucet hands out a challenge per request and rejects a request without a solution to it.
    let challenge: GetPowResponse = client
        .get(format!("{faucet_url}/pow"))
        .query(&[("account_id", recipient.as_str()), ("amount", &REQUESTED_AMOUNT.to_string())])
        .send()
        .await
        .expect("the challenge request should reach the faucet")
        .json()
        .await
        .expect("the faucet should answer with a challenge");

    let nonce = solve(&challenge.challenge);

    let response = client
        .get(format!("{faucet_url}/get_tokens"))
        .query(&[
            ("account_id", recipient.as_str()),
            ("asset_amount", &REQUESTED_AMOUNT.to_string()),
            ("challenge", &challenge.challenge),
            ("nonce", &nonce.to_string()),
        ])
        .send()
        .await
        .expect("the token request should reach the faucet");

    let status = response.status();
    let body = response.text().await.expect("the faucet should answer with a body");
    assert!(status.is_success(), "the faucet answered {status}: {body}");

    let tokens: GetTokensResponse =
        serde_json::from_str(&body).unwrap_or_else(|_| panic!("unexpected response body: {body}"));
    assert_eq!(
        tokens.note_id.len(),
        66,
        "the note id should be a 32-byte hex string: {tokens:?}"
    );
}

/// The faucet under test, stopped when it goes out of scope.
struct Faucet(Child);

impl Faucet {
    fn start(node_url: &str, funding_service_url: &str) -> Self {
        // The faucet reads its API keys on startup, so the file has to exist even with no keys in
        // it. Writing one keeps the test independent of the directory it runs from.
        let api_keys_path = std::env::temp_dir().join("miden-faucet-e2e.keys");
        std::fs::write(&api_keys_path, "").expect("the API keys file should be written");

        let faucet = Command::new(env!("CARGO_BIN_EXE_miden-faucet"))
            .args([
                "start",
                "--funding-service-url",
                funding_service_url,
                "--node-url",
                node_url,
                "--network",
                "localhost",
                "--decimals",
                "6",
                "--api-bind-port",
                &FAUCET_API_PORT.to_string(),
                "--api-keys-file",
                &api_keys_path.to_string_lossy(),
                "--no-frontend",
            ])
            // Its logs go to stdout and would drown the test's output, but a startup failure is
            // reported on stderr and is the first thing to look at when the test cannot reach it.
            .stdout(Stdio::null())
            .stderr(Stdio::inherit())
            .spawn()
            .expect("the faucet binary should start");

        Self(faucet)
    }
}

impl Drop for Faucet {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

/// Reads a URL from the environment, falling back to where the compose stack publishes it.
fn url_from_env(variable: &str, default: &str) -> String {
    std::env::var(variable)
        .unwrap_or_else(|_| default.to_owned())
        .trim_end_matches('/')
        .to_owned()
}

/// Polls `url` until it answers, or gives up after `timeout`.
async fn wait_for(client: &reqwest::Client, url: &str, timeout: Duration) -> Result<(), String> {
    let deadline = Instant::now() + timeout;
    let mut last_error = String::new();

    while Instant::now() < deadline {
        match client.get(url).send().await {
            Ok(response) if response.status().is_success() => return Ok(()),
            Ok(response) => last_error = format!("{url} answered {}", response.status()),
            Err(error) => last_error = format!("{url}: {error}"),
        }

        tokio::time::sleep(Duration::from_millis(500)).await;
    }

    Err(format!("gave up after {}s, last error: {last_error}", timeout.as_secs()))
}

/// Finds a nonce which solves the faucet's challenge.
fn solve(challenge_hex: &str) -> u64 {
    let bytes = hex::decode(challenge_hex).expect("the challenge should be hex");
    let challenge = Challenge::try_from(bytes.as_slice()).expect("the challenge should decode");

    (0..u64::MAX)
        .find(|nonce| challenge.validate_pow(*nonce))
        .expect("a nonce should solve the challenge")
}
