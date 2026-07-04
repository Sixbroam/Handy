//! HTTP client that delegates speech-to-text inference to a remote Handy
//! server. Each request runs on a dedicated OS thread with its own tokio
//! runtime + reqwest client, avoiding the tokio panic that occurs when
//! block_on() is called from inside a tauri::async_runtime::spawn() task.

use crate::server::protocol::{ServerHealth, TranscribeResponse, PROTOCOL_SAMPLE_RATE};
use anyhow::{anyhow, Result};
use std::time::Duration;

pub struct RemoteClient {
    pub url: String,
    pub token: Option<String>,
}

impl RemoteClient {
    pub fn new(url: String, token: Option<String>) -> Self {
        Self { url, token }
    }

    fn endpoint(&self, path: &str) -> String {
        format!("{}{path}", self.url.trim_end_matches('/'))
    }

    fn auth_header(&self) -> Option<String> {
        self.token.as_ref().map(|t| format!("Bearer {t}"))
    }

    /// POST a 16 kHz mono f32 buffer to the server and return the transcribed
    /// text. Runs on a dedicated OS thread to avoid tokio runtime reentry panic.
    pub fn transcribe(&self, audio: &[f32], language: &str) -> Result<String> {
        if audio.is_empty() {
            return Ok(String::new());
        }
        let bytes: Vec<u8> = bytemuck::cast_slice(audio).to_vec();
        let url = self.endpoint("/transcribe");
        let auth = self.auth_header();
        let lang = language.to_string();

        let handle = std::thread::spawn(move || -> Result<String> {
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()?;

            let http = reqwest::Client::builder()
                .timeout(Duration::from_secs(180))
                .connect_timeout(Duration::from_secs(10))
                .build()?;

            rt.block_on(async move {
                let mut req = http
                    .post(&url)
                    .header("Content-Type", "application/octet-stream")
                    .header("X-Sample-Rate", PROTOCOL_SAMPLE_RATE.to_string())
                    .header("X-Language", &lang)
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
        });

        match handle.join() {
            Ok(result) => result,
            Err(e) => std::panic::resume_unwind(e),
        }
    }

    /// Reach `GET /health` to verify connectivity + auth.
    pub fn health(&self) -> Result<ServerHealth> {
        let url = self.endpoint("/health");
        let auth = self.auth_header();

        let handle = std::thread::spawn(move || -> Result<ServerHealth> {
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()?;

            let http = reqwest::Client::builder()
                .timeout(Duration::from_secs(30))
                .connect_timeout(Duration::from_secs(10))
                .build()?;

            rt.block_on(async move {
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
        });

        match handle.join() {
            Ok(result) => result,
            Err(e) => std::panic::resume_unwind(e),
        }
    }
}
