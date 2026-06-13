use std::sync::Arc;

use axum::extract::{Query, State};
use axum::routing::get;
use axum::{Json, Router};
use clap::Parser;
use miden_client::account::AccountId;
use miden_client::note::NoteId;
use miden_client::testing::account_id::ACCOUNT_ID_REGULAR_PUBLIC_ACCOUNT_IMMUTABLE_CODE;
use miden_client::utils::ToHex;
use miden_faucet_client::mint::MintCmd;
use miden_faucet_lib::requests::{
    GetPowResponse,
    GetTokensQueryParams,
    GetTokensResponse,
    PowQueryParams,
};
use miden_pow_rate_limiter::Challenge;
use tokio::net::TcpListener;
use tokio::sync::Mutex;

#[derive(Clone, Default)]
struct RecordedRequest {
    pow_params: Option<PowQueryParams>,
    tokens_params: Option<GetTokensQueryParams>,
}

#[derive(Clone)]
struct AppState {
    pow_response: GetPowResponse,
    note_id_hex: String,
    tx_id: String,
    recorded: Arc<Mutex<RecordedRequest>>,
    challenge_hex: String,
}

#[tokio::test]
async fn mint_command_requests_public_note() {
    let account_id = AccountId::try_from(ACCOUNT_ID_REGULAR_PUBLIC_ACCOUNT_IMMUTABLE_CODE).unwrap();
    let expected_amount = 123_000;

    // Create a valid Challenge with target = u64::MAX so any nonce will solve it
    let challenge = Challenge::from_parts(
        u64::MAX,  // target - any nonce will pass
        0,         // timestamp
        1,         // request_complexity
        [0u8; 32], // requestor
        [0u8; 32], // domain
        [0u8; 32], // signature (doesn't matter for client-side validation)
    );
    let challenge_hex = challenge.to_bytes().to_hex();

    let pow_response = GetPowResponse {
        challenge: challenge_hex.clone(),
        target: u64::MAX,
        timestamp: 0,
    };
    let note_id_hex = format!("0x{}", "00".repeat(32));
    let _note_id = NoteId::try_from_hex(&note_id_hex).expect("hex string should produce a note id");
    // TransactionId requires a valid 32-byte Word (64 hex chars)
    let tx_id_hex = format!("0x{}", "ab".repeat(32));
    let app_state = AppState {
        pow_response,
        note_id_hex,
        tx_id: tx_id_hex,
        recorded: Arc::new(Mutex::new(RecordedRequest::default())),
        challenge_hex,
    };

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let app = Router::new()
        .route("/pow", get(pow_handler))
        .route("/get_tokens", get(tokens_handler))
        .with_state(app_state.clone());
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    let cli = MintCmd::parse_from([
        "mint",
        "--url",
        format!("http://{addr}").as_str(),
        "--target-account",
        account_id.to_hex().as_str(),
        "--amount",
        &expected_amount.to_string(),
        "--api-key",
        "test-key",
        "--no-consume",
    ]);

    Box::pin(cli.execute()).await.unwrap();

    let recorded = app_state.recorded.lock().await.clone();

    // Verify PoW request params
    let pow_params = recorded.pow_params.expect("pow_params should be recorded");
    assert_eq!(pow_params.account_id, account_id.to_hex());
    assert_eq!(pow_params.amount, expected_amount);
    assert_eq!(pow_params.api_key.as_deref(), Some("test-key"));

    // Verify get_tokens request params
    let tokens_params = recorded.tokens_params.expect("tokens_params should be recorded");
    assert_eq!(tokens_params.account_id, account_id.to_hex());
    assert_eq!(tokens_params.asset_amount, expected_amount);
    assert!(!tokens_params.is_private_note);
    assert_eq!(tokens_params.api_key.as_deref(), Some("test-key"));
    assert_eq!(tokens_params.challenge, app_state.challenge_hex);
}

async fn pow_handler(
    State(state): State<AppState>,
    Query(params): Query<PowQueryParams>,
) -> Json<GetPowResponse> {
    {
        let mut recorded = state.recorded.lock().await;
        recorded.pow_params = Some(params);
    }
    Json(state.pow_response.clone())
}

async fn tokens_handler(
    State(state): State<AppState>,
    Query(params): Query<GetTokensQueryParams>,
) -> Json<GetTokensResponse> {
    {
        let mut recorded = state.recorded.lock().await;
        recorded.tokens_params = Some(params);
    }
    Json(GetTokensResponse {
        note_id: state.note_id_hex.clone(),
        tx_id: state.tx_id.clone(),
    })
}
