//! A stub of the funding service's HTTP API, serving a fixed status and a P2ID note per request.

use axum::routing::{get, post};
use axum::{Json, Router};
use miden_protocol::Word;
use miden_protocol::account::AccountId;
use miden_protocol::asset::FungibleAsset;
use miden_protocol::note::{Note, NoteType};
use miden_protocol::utils::serde::Serializable;
use miden_standards::note::P2idNote;
use serde::Deserialize;
use tokio::net::TcpListener;

/// The largest amount the stub accepts, mirroring the service's own cap.
pub const STUB_MAX_AMOUNT: u64 = 1_000_000_000_000;

/// Serves the stub on an already-bound listener. See [`crate::testing::stub_rpc_api::serve_stub`]
/// for why the caller binds it.
pub async fn serve_stub_funding_service(listener: TcpListener) -> anyhow::Result<()> {
    let app = Router::new()
        .route("/status", get(status))
        .route("/request-funds", post(request_funds));

    axum::serve(listener, app).await.map_err(Into::into)
}

async fn status() -> Json<serde_json::Value> {
    Json(serde_json::json!({
        "version": "0.0.0-stub",
        "account_id": FungibleAsset::mock_issuer().to_hex(),
        "balance": STUB_MAX_AMOUNT,
        "chain_tip": 0,
        "max_amount": STUB_MAX_AMOUNT,
        "verification_base_fee": 0,
    }))
}

#[derive(Deserialize)]
struct RequestFundsRequest {
    account_id: String,
    amount: u64,
}

async fn request_funds(Json(request): Json<RequestFundsRequest>) -> Json<serde_json::Value> {
    let faucet_id = FungibleAsset::mock_issuer();
    let target = AccountId::from_hex(&request.account_id).expect("the account ID is valid hex");
    let note: Note = P2idNote::builder()
        .sender(faucet_id)
        .target(target)
        .serial_number(Word::from([3u32; 4]))
        .note_type(NoteType::Public)
        .asset(FungibleAsset::new(faucet_id, request.amount).expect("the amount is a valid asset"))
        .build()
        .expect("the P2ID note is well formed")
        .into();
    Json(serde_json::json!({ "note": hex::encode(note.to_bytes()) }))
}
