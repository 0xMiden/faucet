//! End-to-end test against a real node, funding service and faucet.
//!
//! Creates an account, requests tokens for it, waits for the note to be committed, consumes it and
//! checks the balance. Ignored by default because it needs the network: `make test-e2e` starts
//! everything and runs it.

use std::sync::Arc;
use std::time::Duration;

use miden_client::Client;
use miden_client::account::component::BasicWallet;
use miden_client::account::{
    Account, AccountBuilder, AccountBuilderSchemaCommitmentExt, AccountId, AccountType,
};
use miden_client::auth::{Approver, AuthSchemeId, AuthSecretKey, AuthSingleSig};
use miden_client::builder::ClientBuilder;
use miden_client::keystore::{FilesystemKeyStore, Keystore};
use miden_client::note::{Note, NoteId};
use miden_client::rpc::{Endpoint, GrpcClient, VerifyingRpcClient};
use miden_client::transaction::TransactionRequestBuilder;
use miden_client_sqlite_store::SqliteStore;
use miden_faucet_client::mint::{FaucetHttpClient, solve_challenge};

const DEFAULT_FAUCET_URL: &str = "http://127.0.0.1:18000";
const DEFAULT_NODE_URL: &str = "http://127.0.0.1:57291";

/// The store and keys of this test, kept out of the developer's own `~/.miden`.
const WORK_DIR: &str = "target/e2e/client";

const REQUEST_TIMEOUT_MS: u64 = 30_000;

/// Consuming the note pays a fee out of the note itself, so a request too small to cover the fee
/// fails inside the transaction kernel.
const AMOUNT: u64 = 1_000_000;

/// The funding service answers before the note is committed, so the note takes a few blocks to
/// arrive and a loaded network takes longer than a couple of them.
const NOTE_ATTEMPTS: u32 = 30;
const NOTE_DELAY: Duration = Duration::from_secs(2);

#[tokio::test]
#[ignore = "needs a node, funding service and faucet, run it with `make test-e2e`"]
async fn request_tokens_and_consume_the_note() {
    let faucet_url = env_or("FAUCET_URL", DEFAULT_FAUCET_URL);
    let node_url = env_or("NODE_URL", DEFAULT_NODE_URL);

    let mut client = build_client(&node_url).await;
    let account_id = create_account(&mut client).await;
    println!("Recipient: {}", account_id.to_hex());

    let note_id = request_tokens(&faucet_url, account_id).await;
    println!("Note: {}", note_id.to_hex());

    let note = wait_for_note(&mut client, account_id, note_id).await;
    consume_note(&mut client, account_id, note).await;

    let account = client
        .get_account(account_id)
        .await
        .expect("the account should be readable")
        .expect("the account should be tracked");
    let balance = fungible_balance(&account);

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
async fn build_client(node_url: &str) -> Client<FilesystemKeyStore> {
    let work_dir = std::path::Path::new(WORK_DIR);
    // A fresh store every run, so a note or account from an earlier run cannot satisfy the test.
    let _ = std::fs::remove_dir_all(work_dir);
    std::fs::create_dir_all(work_dir.join("keys")).expect("the work directory should be created");

    let store = SqliteStore::new(work_dir.join("store.sqlite3"))
        .await
        .expect("the store should be created");
    let keystore =
        FilesystemKeyStore::new(work_dir.join("keys")).expect("the keystore should be created");
    let endpoint = Endpoint::try_from(node_url).expect("the node url should be an endpoint");

    ClientBuilder::new()
        .rpc(Arc::new(VerifyingRpcClient::new(GrpcClient::new(
            &endpoint,
            REQUEST_TIMEOUT_MS,
        ))))
        .store(Arc::new(store))
        .authenticator(Arc::new(keystore))
        .build()
        .await
        .expect("the client should connect to the node")
}

/// Creates a public wallet the faucet can send a note to, with its key in the client's keystore.
async fn create_account(client: &mut Client<FilesystemKeyStore>) -> AccountId {
    let key = AuthSecretKey::new_falcon512_poseidon2_with_rng(client.rng());
    let auth = AuthSingleSig::new(Approver::new(
        key.public_key().to_commitment(),
        AuthSchemeId::Falcon512Poseidon2,
    ));

    let account = AccountBuilder::new(rand::random())
        .account_type(AccountType::Public)
        .with_component(auth)
        .with_component(BasicWallet)
        .build_with_schema_commitment()
        .expect("the account should build");

    client
        .authenticator()
        .expect("the client has a keystore")
        .add_key(&key, account.id())
        .await
        .expect("the key should be stored");
    client
        .add_account(&account, false)
        .await
        .expect("the account should be tracked");

    account.id()
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
async fn wait_for_note(
    client: &mut Client<FilesystemKeyStore>,
    account_id: AccountId,
    note_id: NoteId,
) -> Note {
    for attempt in 1..=NOTE_ATTEMPTS {
        client.sync_state().await.expect("the client should sync");

        let consumable = client
            .get_consumable_notes(Some(account_id))
            .await
            .expect("the consumable notes should be readable");

        let found = consumable.into_iter().find(|(record, _)| record.id() == Some(note_id));
        if let Some((record, _)) = found {
            return record.try_into().expect("a committed note record holds its note");
        }

        println!("Note not committed yet, attempt {attempt}/{NOTE_ATTEMPTS}");
        tokio::time::sleep(NOTE_DELAY).await;
    }

    panic!("note {} was not committed in time", note_id.to_hex());
}

/// Consumes the note, which also creates the account on chain.
async fn consume_note(client: &mut Client<FilesystemKeyStore>, account_id: AccountId, note: Note) {
    let request = TransactionRequestBuilder::new()
        .input_notes(vec![(note, None)])
        .build()
        .expect("the consume request should build");

    client
        .submit_new_transaction(account_id, request)
        .await
        .expect("the node should accept the consume transaction");
}

/// Sums the fungible assets the account holds.
fn fungible_balance(account: &Account) -> u64 {
    account
        .vault()
        .assets()
        .filter_map(|asset| asset.as_fungible().map(|asset| asset.amount().as_u64()))
        .sum()
}

fn env_or(variable: &str, default: &str) -> String {
    std::env::var(variable).unwrap_or_else(|_| default.to_owned())
}
