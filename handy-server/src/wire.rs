use serde::{Deserialize, Serialize};

/// Sample rate Handy's audio pipeline produces (mono f32, VAD-trimmed).
#[allow(dead_code)]
pub const PROTOCOL_SAMPLE_RATE: u32 = 16_000;

/// Wire response for `POST /transcribe`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TranscribeResponse {
    pub text: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub language: Option<String>,
}

/// Wire response for `GET /health` and `GET /status`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerHealth {
    pub status: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    pub loaded: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub engine: Option<String>,
    // --- Additive fields (backward-compatible) ---
    /// Server version string
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub server_version: Option<String>,
    /// Backend actually in use (e.g. "Vulkan0", "CPU")
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub backend: Option<String>,
    /// Protocol version (1 for v1)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub protocol_version: Option<u32>,
}

/// Wire error body, returned as JSON for any non-2xx response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TranscribeError {
    pub error: String,
}

/// Single model entry in `GET /models` response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelEntry {
    pub id: String,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub languages: Option<Vec<String>>,
    pub engine: String,
    #[serde(default)]
    pub installed: bool,
    #[serde(default)]
    pub loaded: bool,
}

/// Response for `GET /models`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelsResponse {
    pub models: Vec<ModelEntry>,
}

/// Request body for `POST /models/load`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoadRequest {
    pub id: String,
}
