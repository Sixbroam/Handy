use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use axum::body::Bytes;
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Json, Response};
use axum::routing::{get, post};
use axum::Router;
use tower_http::trace::TraceLayer;

use crate::catalog::Catalog;
use crate::download::download_gguf_from_hf;
use crate::engine::{EngineConfig, EngineManager};
use crate::store::Store;
use crate::wire::*;

/// Default body limit: 64 MB (~17 min of f32 16kHz mono audio).
#[allow(dead_code)]
const DEFAULT_BODY_LIMIT: usize = 64 * 1024 * 1024;

/// Server state shared across handlers.
pub struct ServerState {
    pub engine: Arc<EngineManager>,
    pub catalog: Catalog,
    pub store: Store,
    pub token: Option<String>,
    pub config: EngineConfig,
    pub body_limit: usize,
    /// Semaphore to serialize transcription requests
    transcribe_sem: tokio::sync::Semaphore,
}

impl ServerState {
    pub fn new(
        engine: Arc<EngineManager>,
        token: Option<String>,
        config: EngineConfig,
        body_limit: usize,
    ) -> Self {
        Self {
            engine,
            catalog: Catalog::new(),
            store: Store::new(),
            token,
            config,
            body_limit,
            transcribe_sem: tokio::sync::Semaphore::new(1),
        }
    }
}

impl Clone for ServerState {
    fn clone(&self) -> Self {
        Self {
            engine: self.engine.clone(),
            catalog: self.catalog.clone(),
            store: Store::new(),
            token: self.token.clone(),
            config: self.config.clone(),
            body_limit: self.body_limit,
            transcribe_sem: tokio::sync::Semaphore::new(1),
        }
    }
}

/// Constant-time token comparison.
fn constant_time_eq(a: &str, b: &str) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut result: u8 = 0;
    for (x, y) in a.as_bytes().iter().zip(b.as_bytes().iter()) {
        result |= x ^ y;
    }
    result == 0
}

/// Check if the request has a valid Bearer token.
fn token_valid(headers: &HeaderMap, expected: &Option<String>) -> bool {
    match expected {
        None => true,
        Some(expected) => headers
            .get("Authorization")
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.strip_prefix("Bearer "))
            .is_some_and(|provided| constant_time_eq(provided, expected)),
    }
}

/// Build the server router.
pub fn create_router(state: ServerState) -> Router {
    Router::new()
        .route("/health", get(health))
        .route("/status", get(status))
        .route("/transcribe", post(transcribe))
        .route("/models", get(list_models))
        .route("/models/load", post(load_model))
        .route("/models/unload", post(unload_model))
        .layer(
            axum::extract::DefaultBodyLimit::max(state.body_limit),
        )
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}

/// Start the server.
pub async fn start_server(state: ServerState, addr: SocketAddr) -> Result<()> {
    let app = create_router(state.clone());

    // Validate security: non-loopback bind requires a token
    if !addr.ip().is_loopback() && state.token.is_none() {
        return Err(anyhow::anyhow!(
            "Binding to non-loopback address {} requires --token. Use --insecure to bypass.",
            addr
        ));
    }

    let listener = tokio::net::TcpListener::bind(addr).await?;
    tracing::info!("Server listening on http://{}", addr);
    axum::serve(listener, app).await?;
    Ok(())
}

// --- Handlers ---

async fn health(State(state): State<ServerState>, headers: HeaderMap) -> Response {
    if !token_valid(&headers, &state.token) {
        return unauthorized();
    }

    let loaded = state.engine.is_loaded();
    let model = state.engine.get_current_model();
    let backend = state.engine.get_backend();

    Json(ServerHealth {
        status: "ok".to_string(),
        model,
        loaded,
        engine: Some("handy".to_string()),
        server_version: Some(env!("CARGO_PKG_VERSION").to_string()),
        backend,
        protocol_version: Some(1),
    })
    .into_response()
}

async fn status(State(state): State<ServerState>, headers: HeaderMap) -> Response {
    health(State(state), headers).await
}

async fn transcribe(
    State(state): State<ServerState>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    if !token_valid(&headers, &state.token) {
        return unauthorized();
    }

    // Validate body format
    if body.is_empty() || !body.len().is_multiple_of(4) {
        return (
            StatusCode::BAD_REQUEST,
            Json(TranscribeError {
                error: "audio body must be non-empty raw little-endian f32 bytes (multiple of 4)"
                    .to_string(),
            }),
        )
            .into_response();
    }

    // Check for WAV header (common mistake)
    if body.len() >= 4 {
        let header = [&body[0..4]];
        if header[0][0..4] == *b"RIFF" || header[0][0..4] == *b"WAVE" {
            return (
                StatusCode::BAD_REQUEST,
                Json(TranscribeError {
                    error: "Received WAV file format. Server expects raw PCM f32 little-endian 16kHz mono audio."
                        .to_string(),
                }),
            )
                .into_response();
        }
    }

    // Check for out-of-range samples (catches s16 data misinterpreted as f32)
    let samples = bytemuck::cast_slice::<u8, f32>(&body);
    if samples.iter().any(|s| s.abs() > 4.0 && s.is_finite()) {
        return (
            StatusCode::BAD_REQUEST,
            Json(TranscribeError {
                error: format!(
                    "Audio samples out of expected range [-1, 1]. Max absolute value: {:.2}. \
                     Ensure you're sending raw f32 PCM at 16kHz mono.",
                    samples.iter().map(|s| s.abs()).fold(0.0_f32, f32::max)
                ),
            }),
        )
            .into_response();
    }

    // Check if model is loading
    if state.engine.is_loading() {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(TranscribeError {
                error: "model loading".to_string(),
            }),
        )
            .into_response();
    }

    // Wait for semaphore with timeout
    let permit = match tokio::time::timeout(
        Duration::from_secs(60),
        state.transcribe_sem.acquire(),
    )
    .await
    {
        Ok(permit) => permit.map_err(|_| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(TranscribeError {
                    error: "semaphore closed".to_string(),
                }),
            )
        }),
        Err(_) => {
            return (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(TranscribeError {
                    error: "busy — transcription queue timeout (60s)".to_string(),
                }),
            )
                .into_response();
        }
    };

    let permit = match permit {
        Ok(p) => p,
        Err(resp) => return resp.into_response(),
    };

    // Parse language from header
    let language = headers
        .get("X-Language")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("auto")
        .to_string();

    let audio: Vec<f32> = samples.to_vec();
    let audio_len = audio.len();

    // Run inference in blocking thread
    let engine = state.engine.clone();
    let lang_clone = language.clone();
    let result = tokio::task::spawn_blocking(move || {
        engine.transcribe(&audio, &lang_clone, false)
    })
    .await;

    drop(permit);

    match result {
        Ok(Ok(text)) => {
            tracing::debug!(
                "Transcription complete: {} samples (~{:.1}s) -> {} chars",
                audio_len,
                audio_len as f64 / 16000.0,
                text.len()
            );
            Json(TranscribeResponse {
                text,
                language: Some(language),
            })
            .into_response()
        }
        Ok(Err(e)) => {
            tracing::error!("Transcription failed: {}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(TranscribeError {
                    error: e.to_string(),
                }),
            )
                .into_response()
        }
        Err(e) => {
            tracing::error!("Transcription task panicked: {}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(TranscribeError {
                    error: "transcription task failed".to_string(),
                }),
            )
                .into_response()
        }
    }
}

async fn list_models(State(state): State<ServerState>, headers: HeaderMap) -> Response {
    if !token_valid(&headers, &state.token) {
        return unauthorized();
    }

    let models = state.catalog.list(false);
    let current_model = state.engine.get_current_model();

    let entries: Vec<ModelEntry> = models
        .into_iter()
        .map(|m| {
            let installed = match &m.engine_type {
                crate::catalog::EngineType::TranscribeCpp => {
                    state.store.is_gguf_installed(&m.id)
                }
                _ => state.store.is_onnx_installed(&m.slug),
            };

            ModelEntry {
                id: m.id.clone(),
                name: m.name.clone(),
                languages: if m.languages.is_empty() {
                    None
                } else {
                    Some(m.languages.clone())
                },
                engine: m.engine_type.as_str().to_string(),
                installed,
                loaded: current_model.as_deref() == Some(&m.id),
            }
        })
        .collect();

    Json(ModelsResponse { models: entries }).into_response()
}

async fn load_model(
    State(state): State<ServerState>,
    headers: HeaderMap,
    body: String,
) -> Response {
    if !token_valid(&headers, &state.token) {
        return unauthorized();
    }

    let request: LoadRequest = match serde_json::from_str(&body) {
        Ok(r) => r,
        Err(e) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(TranscribeError {
                    error: format!("Invalid request body: {}", e),
                }),
            )
                .into_response();
        }
    };

    // Check if already loading
    if state.engine.is_loading() {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(TranscribeError {
                error: "model loading".to_string(),
            }),
        )
            .into_response();
    }

    let model = match state.catalog.get(&request.id) {
        Some(m) => m.clone(),
        None => {
            return (
                StatusCode::NOT_FOUND,
                Json(TranscribeError {
                    error: format!("Model not found: {}", request.id),
                }),
            )
                .into_response();
        }
    };

    // Gate ONNX models
    if model.engine_type != crate::catalog::EngineType::TranscribeCpp
        && !state.config.allow_onnx
    {
        return (
            StatusCode::BAD_REQUEST,
            Json(TranscribeError {
                error: format!(
                    "ONNX model '{}' is gated. Run with --allow-onnx to enable \
                     (requires passing selftest first).",
                    request.id
                ),
            }),
        )
            .into_response();
    }

    // Spawn async load task
    let engine = state.engine.clone();
    let store = Store::new();
    let model_id = model.id.clone();
    let model_id_for_response = model_id.clone();
    let model_type = model.engine_type.clone();
    let hf_repo = model.hf_repo.clone();
    let default_file = model
        .files
        .first()
        .map(|f| f.filename.clone())
        .unwrap_or_default();

    tokio::spawn(async move {
        match model_type {
            crate::catalog::EngineType::TranscribeCpp => {
                let path = store.resolve_gguf_path(&model_id);

                // Download if not installed
                if !store.is_gguf_installed(&model_id) {
                    tracing::info!(
                        "Model {} not installed, downloading from HF...",
                        model_id
                    );
                    if let Err(e) = download_gguf_from_hf(
                        &hf_repo,
                        &default_file,
                        &path,
                        model.files.first().map(|f| f.size_bytes),
                    )
                    .await
                    {
                        tracing::error!("Download failed: {}", e);
                        return;
                    }
                }

                if let Err(e) = engine.load_gguf(&model_id, &path) {
                    tracing::error!("Failed to load model: {}", e);
                }
            }
            _ => {
                // ONNX models - not yet implemented for standalone server
                tracing::warn!("ONNX model loading not yet available");
            }
        }
    });

    (
        StatusCode::ACCEPTED,
        Json(serde_json::json!({
            "status": "loading",
            "model": model_id_for_response,
        })),
    )
        .into_response()
}

async fn unload_model(
    State(state): State<ServerState>,
    headers: HeaderMap,
) -> Response {
    if !token_valid(&headers, &state.token) {
        return unauthorized();
    }

    match state.engine.unload() {
        Ok(()) => Json(serde_json::json!({ "status": "unloaded" })).into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(TranscribeError {
                error: e.to_string(),
            }),
        )
            .into_response(),
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
