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

#[cfg(test)]
mod tests {
    use super::*;

    /// Starts the stub and returns its base URL.
    async fn start_stub() -> String {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        tokio::spawn(async move { serve_stub(listener).await.unwrap() });
        url
    }

    /// Makes a gRPC-web call the way the web client does, and returns the response's data frame
    /// after checking the response and its trailers report success.
    async fn call(url: &str, method: &str) -> Vec<u8> {
        let response = reqwest::Client::new()
            .post(format!("{url}{SERVICE_PATH}/{method}"))
            .header(CONTENT_TYPE, "application/grpc-web+proto")
            .header("x-grpc-web", "1")
            .body(vec![0, 0, 0, 0, 0])
            .send()
            .await
            .unwrap();
        assert!(response.status().is_success());
        assert_eq!(response.headers()[CONTENT_TYPE], "application/grpc-web+proto");

        let body = response.bytes().await.unwrap();
        let (data, rest) = split_frame(&body, DATA_FRAME_FLAG);
        let (trailers, rest) = split_frame(rest, TRAILERS_FRAME_FLAG);
        assert_eq!(trailers, b"grpc-status:0\r\n");
        assert_eq!(rest, [] as [u8; 0]);
        data.to_vec()
    }

    const DATA_FRAME_FLAG: u8 = 0x00;
    const TRAILERS_FRAME_FLAG: u8 = 0x80;

    fn split_frame(bytes: &[u8], flag: u8) -> (&[u8], &[u8]) {
        assert_eq!(bytes[0], flag);
        let length = u32::from_be_bytes(bytes[1..5].try_into().unwrap()) as usize;
        bytes[5..].split_at(length)
    }

    #[tokio::test]
    async fn get_limits_lists_every_limit_the_client_requires() {
        let url = start_stub().await;
        let response = GetLimitsResponse::decode(call(&url, "GetLimits").await.as_slice()).unwrap();

        for (endpoint, parameter) in LIMITS {
            assert!(response.endpoints[endpoint].parameters.contains_key(parameter));
        }
    }

    #[tokio::test]
    async fn get_notes_by_id_returns_no_notes() {
        let url = start_stub().await;
        assert_eq!(call(&url, "GetNotesById").await, Vec::<u8>::new());
    }

    /// The frontend is served from another origin, so the browser preflights each call.
    #[tokio::test]
    async fn cross_origin_calls_are_allowed() {
        let url = start_stub().await;
        let response = reqwest::Client::new()
            .request(reqwest::Method::OPTIONS, format!("{url}{SERVICE_PATH}/GetNotesById"))
            .header("origin", "http://localhost:8080")
            .header("access-control-request-method", "POST")
            .header("access-control-request-headers", "content-type,x-grpc-web")
            .send()
            .await
            .unwrap();

        assert!(response.status().is_success());
        assert_eq!(response.headers()["access-control-allow-origin"], "*");
    }
}
