//! A stub of the Miden node RPC that answers only the calls the faucet frontend makes.
//!
//! The frontend talks to the node for one thing: it polls `GetNotesById` to learn when a minted
//! note is committed. Before its first call, the web client also fetches `GetLimits`. Both are
//! served here over gRPC-web with hand-written messages, so the tests don't need the node's
//! generated protobuf bindings.

use std::collections::HashMap;

use anyhow::Context;
use axum::Router;
use axum::http::header::CONTENT_TYPE;
use axum::response::IntoResponse;
use axum::routing::post;
use prost::Message;
use tokio::net::TcpListener;
use tower_http::cors::CorsLayer;

/// Path prefix of the node's gRPC service (`miden.node.v1.NodeService`).
const SERVICE_PATH: &str = "/miden.node.v1.NodeService";

/// The `(endpoint, parameter)` limits the client requires in a `GetLimits` response. The client
/// rejects the response if any of them is missing.
const LIMITS: [(&str, &str); 5] = [
    ("GetNotesById", "note_id"),
    ("SyncNullifiers", "nullifier_prefix"),
    ("SyncTransactions", "account_id"),
    ("SyncNotes", "note_tag"),
    ("GetAccount", "vault_asset"),
];

/// `miden.node.v1.GetLimitsResponse`.
#[derive(Clone, PartialEq, prost::Message)]
struct GetLimitsResponse {
    #[prost(map = "string, message", tag = "1")]
    endpoints: HashMap<String, EndpointLimits>,
}

/// `miden.node.v1.EndpointLimits`.
#[derive(Clone, PartialEq, prost::Message)]
struct EndpointLimits {
    #[prost(map = "string, uint32", tag = "1")]
    parameters: HashMap<String, u32>,
}

/// Serves the stub on an already-bound listener.
///
/// The listener is bound by the caller so the port is accepting connections before the caller
/// hands out its URL; binding it here instead would leave a window where clients are refused.
pub async fn serve_stub(listener: TcpListener) -> anyhow::Result<()> {
    let app = Router::new()
        .route(&format!("{SERVICE_PATH}/GetLimits"), post(get_limits))
        .route(&format!("{SERVICE_PATH}/GetNotesById"), post(get_notes_by_id))
        // The frontend is served from another origin.
        .layer(CorsLayer::permissive());

    axum::serve(listener, app).await.context("failed to serve stub node RPC")
}

async fn get_limits() -> impl IntoResponse {
    let endpoints = LIMITS
        .into_iter()
        .map(|(endpoint, parameter)| {
            let parameters = HashMap::from([(parameter.to_string(), 1000)]);
            (endpoint.to_string(), EndpointLimits { parameters })
        })
        .collect();

    grpc_web_response(&GetLimitsResponse { endpoints }.encode_to_vec())
}

/// Answers that none of the notes are committed yet. A `GetNotesByIdResponse` with no notes
/// encodes to an empty message.
async fn get_notes_by_id() -> impl IntoResponse {
    grpc_web_response(&[])
}

/// Wraps an encoded message in a successful gRPC-web response: a data frame holding the message,
/// followed by a trailers frame with an OK status.
fn grpc_web_response(message: &[u8]) -> impl IntoResponse + use<> {
    const DATA_FRAME: u8 = 0x00;
    const TRAILERS_FRAME: u8 = 0x80;

    let mut body = Vec::new();
    push_frame(&mut body, DATA_FRAME, message);
    push_frame(&mut body, TRAILERS_FRAME, b"grpc-status:0\r\n");

    ([(CONTENT_TYPE, "application/grpc-web+proto")], body)
}

fn push_frame(body: &mut Vec<u8>, flag: u8, payload: &[u8]) {
    let length = u32::try_from(payload.len()).expect("the payload fits in a gRPC-web frame");
    body.push(flag);
    body.extend_from_slice(&length.to_be_bytes());
    body.extend_from_slice(payload);
}
