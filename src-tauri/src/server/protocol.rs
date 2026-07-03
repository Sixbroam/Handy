use serde::{Deserialize, Serialize};
use specta::Type;

/// Sample rate Handy's audio pipeline produces (mono f32, VAD-trimmed). Both
/// the client wire payload and the server's transcription path assume this.
pub const PROTOCOL_SAMPLE_RATE: u32 = 16_000;

/// Wire response for `POST /transcribe`.
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct TranscribeResponse {
    pub text: String,
    /// Language the server transcribed in (echoes the request header or the
    /// server's configured language). Informational only.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub language: Option<String>,
}

/// Wire response for `GET /health` and `GET /status`.
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct ServerHealth {
    pub status: String,
    /// Id of the model currently loaded on the server, if any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    pub loaded: bool,
    /// Engine family serving the model (e.g. "transcribe-cpp", "onnx"). Best
    /// effort — surfaced for diagnostics, not load-bearing.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub engine: Option<String>,
}

/// Wire error body, returned as JSON for any non-2xx response.
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct TranscribeError {
    pub error: String,
}
