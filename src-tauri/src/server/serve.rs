//! `--serve` mode: an axum HTTP server that exposes the locally-loaded speech
//! model to other Handy instances on the network (e.g. a low-power laptop
//! offloading inference to a GPU mini-PC over LAN/Tailscale).
//!
//! Wire protocol lives in [`crate::server::protocol`]. The server is a thin
//! adapter over [`TranscriptionManager::transcribe`](crate::managers::transcription::TranscriptionManager::transcribe)
//! — it does not re-implement inference, it just feeds the received 16 kHz mono
//! f32 buffer through the same path the GUI uses.

use crate::managers::transcription::TranscriptionManager;
use crate::server::protocol::{ServerHealth, TranscribeError, TranscribeResponse};
use axum::{
    body::Bytes,
    extract::State,
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Json, Response},
    routing::{get, post},
    Router,
};
use std::net::SocketAddr;
use std::sync::Arc;
use tauri::{AppHandle, Manager};
use tower_http::trace::TraceLayer;

#[derive(Clone)]
struct ServerState {
    tm: Arc<TranscriptionManager>,
    token: Option<String>,
}

/// True if the request carries the configured Bearer token (or if no token is
/// configured, in which case the server is open — bind to loopback then).
fn token_valid(headers: &HeaderMap, expected: &Option<String>) -> bool {
    match expected {
        None => true,
        Some(expected) => headers
            .get("Authorization")
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.strip_prefix("Bearer "))
            .is_some_and(|provided| provided == expected),
    }
}

/// Build the transcription server and spawn it on Tauri's async runtime. The
/// server lives for the lifetime of the process; binding failures are logged
/// rather than crashing the app.
pub fn start_server(app_handle: AppHandle, addr: SocketAddr, token: Option<String>) {
    let tm = app_handle
        .state::<Arc<TranscriptionManager>>()
        .inner()
        .clone();
    let state = ServerState { tm, token };

    let app = Router::new()
        .route("/health", get(health))
        .route("/status", get(status))
        .route("/transcribe", post(transcribe))
        .layer(TraceLayer::new_for_http())
        .with_state(state);

    tauri::async_runtime::spawn(async move {
        match tokio::net::TcpListener::bind(&addr).await {
            Ok(listener) => {
                log::info!("Handy transcription server listening on http://{addr}");
                if let Err(e) = axum::serve(listener, app).await {
                    log::error!("Handy transcription server stopped: {e}");
                }
            }
            Err(e) => log::error!("Failed to bind Handy transcription server to {addr}: {e}"),
        }
    });
}

async fn health(State(state): State<ServerState>, headers: HeaderMap) -> Response {
    if !token_valid(&headers, &state.token) {
        return unauthorized();
    }
    let loaded = state.tm.is_model_loaded();
    let model = state.tm.get_current_model();
    Json(ServerHealth {
        status: "ok".to_string(),
        model,
        loaded,
        engine: Some("handy".to_string()),
    })
    .into_response()
}

async fn status(State(state): State<ServerState>, headers: HeaderMap) -> Response {
    health(State(state), headers).await
}

async fn transcribe(State(state): State<ServerState>, headers: HeaderMap, body: Bytes) -> Response {
    if !token_valid(&headers, &state.token) {
        return unauthorized();
    }
    if body.is_empty() || !body.len().is_multiple_of(4) {
        return (
            StatusCode::BAD_REQUEST,
            Json(TranscribeError {
                error: "audio body must be non-empty raw little-endian f32 bytes".to_string(),
            }),
        )
            .into_response();
    }
    // bytemuck::cast_slice produces a &[f32] view over the request bytes; copy
    // it into an owned Vec<f32> for the transcription manager.
    let audio: Vec<f32> = bytemuck::cast_slice::<u8, f32>(&body).to_vec();
    // The server transcribes with its own configured model + language (single
    // source of truth). The client's X-Language header is surfaced for
    // diagnostics only in v1.
    let language = headers
        .get("X-Language")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("auto")
        .to_string();
    log::debug!(
        "Server /transcribe: {} samples (~{:.1}s), language={}",
        audio.len(),
        audio.len() as f32 / super::protocol::PROTOCOL_SAMPLE_RATE as f32,
        language
    );
    match state.tm.transcribe(audio) {
        Ok(text) => Json(TranscribeResponse {
            text,
            language: Some(language),
        })
        .into_response(),
        Err(e) => {
            log::error!("Server transcription failed: {e}");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(TranscribeError {
                    error: e.to_string(),
                }),
            )
                .into_response()
        }
    }
}

fn unauthorized() -> Response {
    (
        StatusCode::UNAUTHORIZED,
        Json(TranscribeError {
            error: "invalid or missing bearer token".to_string(),
        }),
    )
        .into_response()
}
