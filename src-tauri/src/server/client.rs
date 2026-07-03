//! HTTP client that delegates speech-to-text inference to a remote Handy
//! instance running in `--serve` mode. Owned (and cached) by the client-side
//! [`TranscriptionManager`](crate::managers::transcription::TranscriptionManager)
//! and rebuilt automatically when the URL/token settings change.

use crate::server::protocol::{ServerHealth, TranscribeResponse, PROTOCOL_SAMPLE_RATE};
use anyhow::{anyhow, Result};
use std::time::Duration;

/// A speech-to-text client for a remote Handy server.
///
/// Holds a dedicated current-thread tokio runtime so the blocking `reqwest`
/// calls stay off Tauri's async runtime — `transcribe()` runs on a worker
/// thread, never inside an async context, so blocking here is safe and isolated.
pub struct RemoteClient {
    pub url: String,
    pub token: Option<String>,
    http: reqwest::Client,
    rt: tokio::runtime::Runtime,
}

impl RemoteClient {
    pub fn new(url: String, token: Option<String>) -> Self {
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(180))
            .connect_timeout(Duration::from_secs(10))
            .build()
            .unwrap_or_else(|_| reqwest::Client::new());
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("failed to build remote-client tokio runtime");
        Self {
            url,
            token,
            http,
            rt,
        }
    }

    fn endpoint(&self, path: &str) -> String {
        let base = self.url.trim_end_matches('/');
        format!("{base}{path}")
    }

    fn auth_header(&self) -> Option<String> {
        self.token.as_ref().map(|t| format!("Bearer {t}"))
    }

    /// POST a 16 kHz mono f32 buffer to the server and return the transcribed
    /// text. The samples are sent as raw little-endian f32 bytes
    /// (`application/octet-stream`).
    pub fn transcribe(&self, audio: &[f32], language: &str) -> Result<String> {
        if audio.is_empty() {
            return Ok(String::new());
        }
        let bytes: Vec<u8> = bytemuck::cast_slice(audio).to_vec();
        let url = self.endpoint("/transcribe");
        let auth = self.auth_header();
        let http = self.http.clone();
        self.rt.block_on(async move {
            let mut req = http
                .post(&url)
                .header("Content-Type", "application/octet-stream")
                .header("X-Sample-Rate", PROTOCOL_SAMPLE_RATE.to_string())
                .header("X-Language", language)
                .body(bytes);
            if let Some(v) = auth {
                req = req.header("Authorization", v);
            }
            let resp = req
                .send()
                .await
                .map_err(|e| anyhow!("remote request failed: {e}"))?;
            let status = resp.status();
            if status.is_success() {
                let parsed = resp
                    .json::<TranscribeResponse>()
                    .await
                    .map_err(|e| anyhow!("remote response parse failed: {e}"))?;
                Ok(parsed.text)
            } else {
                let code = status.as_u16();
                let body = resp.text().await.unwrap_or_default();
                Err(anyhow!("remote server error {code}: {body}"))
            }
        })
    }

    /// Reach `GET /health` to verify connectivity + auth. Used by the settings
    /// UI's "Test connection" action.
    pub fn health(&self) -> Result<ServerHealth> {
        let url = self.endpoint("/health");
        let auth = self.auth_header();
        let http = self.http.clone();
        self.rt.block_on(async move {
            let mut req = http.get(&url);
            if let Some(v) = auth {
                req = req.header("Authorization", v);
            }
            let resp = req
                .send()
                .await
                .map_err(|e| anyhow!("health request failed: {e}"))?;
            let status = resp.status();
            if !status.is_success() {
                return Err(anyhow!("health check failed: HTTP {}", status.as_u16()));
            }
            resp.json::<ServerHealth>()
                .await
                .map_err(|e| anyhow!("health response parse failed: {e}"))
        })
    }
}
