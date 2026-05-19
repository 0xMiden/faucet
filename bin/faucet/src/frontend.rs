//! This file explicitly embeds each of the frontend files into the binary using `include_str!` and
//! `include_bytes!`.
use anyhow::Context;
use axum::extract::Request;
use axum::response::{Html, IntoResponse, Response};
use axum::routing::get;
use axum::{Json, Router};
use axum_extra::response::{Css, JavaScript};
use base64::Engine;
use http::header::{self};
use http::{HeaderMap, HeaderName, HeaderValue, StatusCode};
use sha2::{Digest, Sha256};
use tokio::net::TcpListener;
use tower_http::compression::CompressionLayer;
use tracing::info;
use url::Url;

use crate::COMPONENT;

/// Serves the frontend API endpoints.
pub async fn serve_frontend(url: Url, api_public_url: Url, node_url: String) -> anyhow::Result<()> {
    let config_json = Json(
        serde_json::json!({
            "api_url": api_public_url.to_string().trim_end_matches('/'),
            "node_url": node_url.trim_end_matches('/'),
        })
        .to_string(),
    );

    let app = Router::new()
        .route("/", get(get_index_html))
        .route("/bundle.js", get(get_bundle_js))
        .route("/index.css", get(get_index_css))
        .route("/wallet-icon.png", get(get_wallet_icon))
        .route("/header.png", get(get_header))
        .route("/favicon.ico", get(get_favicon))
        .route("/assets/miden_client_web.wasm", get(get_miden_client_web_wasm))
        .route("/config.json", get(config_json))
        .layer(CompressionLayer::new())
        .fallback(get(get_not_found_html));

    let listener = url
        .socket_addrs(|| None)
        .with_context(|| format!("failed to parse url {url}"))?;
    let listener = TcpListener::bind(&*listener)
        .await
        .with_context(|| format!("failed to bind TCP listener on {url}"))?;

    info!(target: COMPONENT, address = %url, "Frontend server started");

    axum::serve(listener, app).await.map_err(Into::into)
}

pub async fn get_index_html() -> Html<&'static str> {
    Html(include_str!(concat!(env!("OUT_DIR"), "/frontend/index.html")))
}

pub async fn get_miden_client_web_wasm(request: Request) -> Response {
    const WASM_BYTES: &[u8] = include_bytes!(concat!(
        env!("OUT_DIR"),
        "/frontend/node_modules/@miden-sdk/miden-sdk/dist/st/assets/miden_client_web.wasm"
    ));

    let etag = compute_etag(WASM_BYTES);
    if let Some(response) = check_if_none_match(&request, &etag) {
        return response;
    }

    let mut response = (
        [(header::CONTENT_TYPE, header::HeaderValue::from_static("application/wasm"))],
        WASM_BYTES,
    )
        .into_response();

    add_cache_headers(response.headers_mut(), &etag);
    response
}

pub async fn get_not_found_html() -> Html<&'static str> {
    Html(include_str!(concat!(env!("OUT_DIR"), "/frontend/not_found.html")))
}

pub async fn get_bundle_js(request: Request) -> Response {
    const BUNDLE_JS: &str = include_str!(concat!(env!("OUT_DIR"), "/frontend/bundle.js"));
    const BUNDLE_BYTES: &[u8] = BUNDLE_JS.as_bytes();

    let etag = compute_etag(BUNDLE_BYTES);
    if let Some(response) = check_if_none_match(&request, &etag) {
        return response;
    }

    let mut response = (
        [(header::CONTENT_TYPE, header::HeaderValue::from_static("application/javascript"))],
        JavaScript(BUNDLE_JS),
    )
        .into_response();

    add_cache_headers(response.headers_mut(), &etag);
    response
}

pub async fn get_index_css() -> Css<&'static str> {
    Css(include_str!(concat!(env!("OUT_DIR"), "/frontend/index.css")))
}

pub async fn get_wallet_icon() -> Response {
    (
        [(header::CONTENT_TYPE, header::HeaderValue::from_static("image/png"))],
        include_bytes!(concat!(env!("OUT_DIR"), "/frontend/wallet-icon.png"),),
    )
        .into_response()
}

pub async fn get_header() -> Response {
    (
        [(header::CONTENT_TYPE, header::HeaderValue::from_static("image/png"))],
        include_bytes!(concat!(env!("OUT_DIR"), "/frontend/header.png"),),
    )
        .into_response()
}

pub async fn get_favicon() -> Response {
    (
        [(header::CONTENT_TYPE, header::HeaderValue::from_static("image/x-icon"))],
        include_bytes!(concat!(env!("OUT_DIR"), "/frontend/favicon.ico"),),
    )
        .into_response()
}

// CACHE HEADERS HELPERS
// ================================================================================================

/// Add `ETag` and `Cache-Control` headers to the response.
fn add_cache_headers(headers: &mut HeaderMap, etag: &str) {
    headers.insert(header::ETAG, header::HeaderValue::from_str(etag).unwrap());
    let (name, value) = cache_control_header();
    headers.insert(name, value);
}

/// Return the `Cache-Control` header with 1 day expiration.
fn cache_control_header() -> (HeaderName, HeaderValue) {
    (
        header::CACHE_CONTROL,
        header::HeaderValue::from_static("public, max-age=86400, immutable"),
    )
}

/// Compute the `ETag` for the given byte content using SHA-256 hash and base64 encoding.
fn compute_etag(content: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(content);
    let hash = hasher.finalize();
    format!("\"{}\"", base64::engine::general_purpose::STANDARD.encode(&hash[..16]))
}

/// Check If-None-Match header and return 304 if `ETag` matches, otherwise return None
fn check_if_none_match(request: &Request, etag: &str) -> Option<Response> {
    if let Some(if_none_match) = request.headers().get(header::IF_NONE_MATCH)
        && if_none_match.to_str().unwrap_or("") == etag
    {
        return Some(
            (
                StatusCode::NOT_MODIFIED,
                [
                    (header::ETAG, header::HeaderValue::from_str(etag).unwrap()),
                    cache_control_header(),
                ],
            )
                .into_response(),
        );
    }
    None
}
