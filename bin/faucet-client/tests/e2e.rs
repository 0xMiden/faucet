//! End-to-end test against a real node, funding service and faucet.
//!
//! Creates an account, requests tokens for it, waits for the note to be committed, consumes it and
//! checks the balance. Ignored by default because it needs the network: `make test-e2e` starts
//! everything and runs it.

use std::env::temp_dir;

use miden_client::account::{AccountId, AccountType};
use miden_client::builder::ClientBuilder;
use miden_client::keystore::FilesystemKeyStore;
use miden_client::note::{Note, NoteId};
use miden_client::rpc::Endpoint;
use miden_client::testing::common::{AccountSetup, TestClient};
use miden_client_sqlite_store::ClientBuilderSqliteExt;
use miden_faucet_client::mint::{FaucetHttpClient, solve_challenge};

const DEFAULT_FAUCET_URL: &str = "http://127.0.0.1:18000";

const REQUEST_TIMEOUT_MS: u64 = 30_000;

/// Consuming the note pays a fee out of the note itself, so a request too small to cover the fee
/// fails inside the transaction kernel.
const AMOUNT: u64 = 1_000_000;

/// The funding service answers before the note is committed, so the note takes a few blocks to
/// arrive and a loaded network takes longer than a couple of them.
const NOTE_MAX_BLOCKS: u32 = 60;

#[tokio::test]
#[ignore = "needs a node, funding service and faucet, run it with `make test-e2e`"]
async fn request_tokens_and_consume_the_note() {
    let faucet_url = env_or("FAUCET_URL", DEFAULT_FAUCET_URL);
    let node_url = env_or("NODE_URL", &Endpoint::localhost().to_string());

    let mut client = build_client(&node_url).await;

    // Unfunded: the note the faucet sends is what pays for the transaction that consumes it, and
    // funding would need a fee funder this client does not have.
    let (account, _) = client
        .insert_account(AccountSetup::wallet(AccountType::Public).unfunded())
        .await
        .expect("the account should be created");
    let account_id = account.id();
    println!("Recipient: {}", account_id.to_hex());

    let note_id = request_tokens(&faucet_url, account_id).await;
    println!("Note: {}", note_id.to_hex());

    let note = wait_for_note(&mut client, account_id, note_id).await;
    // The note carries the asset of the faucet the funding service pays from, which is the balance
    // to check once the note is consumed.
    let faucet_id = note
        .assets()
        .iter_fungible()
        .next()
        .expect("the note should carry a fungible asset")
        .faucet_id();

    consume_note(&mut client, account_id, note).await;

    let balance = client
        .account_reader(account_id)
        .get_balance(faucet_id)
        .await
        .expect("the balance should be readable")
        .as_u64();

    // The consume transaction pays its fee out of the note, so the balance is the requested amount
    // less that fee.
    assert!(balance > 0, "the recipient holds no asset after consuming the note");
    assert!(
        balance <= AMOUNT,
        "the recipient holds {balance}, more than the {AMOUNT} requested"
    );
    println!("Balance: {balance} of the {AMOUNT} requested, the rest paid the fee");
}

/// Builds a client with its own store and keystore, pointed at the test network.
async fn build_client(node_url: &str) -> TestClient {
    // Emptied first, so no account or note from an earlier run can take part in the test.
    let work_dir = temp_dir().join("miden-faucet-e2e");
    let _ = std::fs::remove_dir_all(&work_dir);
    std::fs::create_dir_all(&work_dir).expect("the work directory should be created");

    let endpoint = Endpoint::try_from(node_url).expect("the node url should be an endpoint");

    let client = ClientBuilder::<FilesystemKeyStore>::new()
        .grpc_client(&endpoint, Some(REQUEST_TIMEOUT_MS))
        .filesystem_keystore(work_dir.join("keys"))
        .expect("the keystore should be created")
        .sqlite_store(work_dir.join("store.sqlite3"))
        .build()
        .await
        .expect("the client should connect to the node");

    TestClient::new(client)
}

/// Requests tokens the way the faucet client does: a challenge, a nonce that solves it, and the
/// request itself.
async fn request_tokens(faucet_url: &str, account_id: AccountId) -> NoteId {
    let faucet = FaucetHttpClient::new(faucet_url, REQUEST_TIMEOUT_MS, None)
        .expect("the faucet url should be valid");

    let (challenge, target) = faucet
        .request_pow(&account_id, AMOUNT)
        .await
        .expect("the faucet should answer with a challenge");
    let nonce = solve_challenge(&challenge, target)
        .await
        .expect("the challenge should be solvable");

    faucet
        .request_tokens(&challenge, nonce, &account_id, AMOUNT)
        .await
        .expect("the faucet should answer with a note")
        .note_id
}

/// Syncs until the note is committed and consumable by the account.
async fn wait_for_note(client: &mut TestClient, account_id: AccountId, note_id: NoteId) -> Note {
    let consumable = client
        .wait_for_consumable_notes(account_id, NOTE_MAX_BLOCKS)
        .await
        .expect("the note should be committed");

    let (record, _) = consumable
        .into_iter()
        .find(|(record, _)| record.id() == Some(note_id))
        .unwrap_or_else(|| {
            panic!("the faucet's note {} is not the one that arrived", note_id.to_hex())
        });

    record.try_into().expect("a committed note record holds its note")
}

/// Consumes the note, which also creates the account on chain, and waits for the block holding it.
async fn consume_note(client: &mut TestClient, account_id: AccountId, note: Note) {
    let transaction_id = client
        .consume_notes(account_id, &[note])
        .await
        .expect("the node should accept the consume transaction");

    client
        .wait_for_tx(transaction_id)
        .await
        .expect("the consume transaction should be committed");
}

fn env_or(variable: &str, default: &str) -> String {
    std::env::var(variable).unwrap_or_else(|_| default.to_owned())
}
