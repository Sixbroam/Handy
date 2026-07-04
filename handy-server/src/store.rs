use anyhow::{anyhow, Result};
use std::fs;
use std::path::{Path, PathBuf};

/// Model store: manages the on-disk model directory.
#[allow(dead_code)]
pub struct Store {
    models_dir: PathBuf,
}

impl Store {
    /// Resolve the models directory: `$HANDY_MODELS_DIR` or `~/.local/share/com.pais.handy/models`.
    pub fn new() -> Self {
        let models_dir = if let Ok(dir) = std::env::var("HANDY_MODELS_DIR") {
            PathBuf::from(dir)
        } else {
            dirs::data_dir()
                .unwrap_or_else(|| PathBuf::from("."))
                .join("com.pais.handy")
                .join("models")
        };

        fs::create_dir_all(&models_dir).ok();
        Self { models_dir }
    }

    /// Get the models directory path.
    #[allow(dead_code)]
    pub fn dir(&self) -> &Path {
        &self.models_dir
    }

    /// Resolve a GGUF model file path from its catalog ID.
    /// E.g. `handy-computer/canary-1b-v2-gguf/canary-1b-v2-Q5_K_M.gguf` -> `<models>/canary-1b-v2-Q5_K_M.gguf`
    pub fn resolve_gguf_path(&self, model_id: &str) -> PathBuf {
        let parts: Vec<&str> = model_id.split('/').collect();
        let filename = parts.last().unwrap_or(&model_id);
        self.models_dir.join(filename)
    }

    /// Resolve an ONNX model directory path from its slug.
    pub fn resolve_onnx_path(&self, slug: &str) -> PathBuf {
        self.models_dir.join(slug)
    }

    /// Check if a GGUF model is fully installed (no `.part` file).
    pub fn is_gguf_installed(&self, model_id: &str) -> bool {
        let path = self.resolve_gguf_path(model_id);
        let part_path = path.with_extension("part");
        path.exists() && path.is_file() && !part_path.exists()
    }

    /// Check if an ONNX model directory is fully installed.
    pub fn is_onnx_installed(&self, slug: &str) -> bool {
        let path = self.resolve_onnx_path(slug);
        let part_path = path.with_extension("part");
        path.exists() && path.is_dir() && !part_path.exists()
    }

    /// Get the size of an installed model on disk.
    #[allow(dead_code)]
    pub fn get_model_size(&self, path: &Path) -> Result<u64> {
        if path.is_file() {
            Ok(path.metadata()?.len())
        } else if path.is_dir() {
            let mut total = 0;
            for entry in fs::read_dir(path)? {
                let entry = entry?;
                let meta = entry.metadata()?;
                if meta.is_file() {
                    total += meta.len();
                }
            }
            Ok(total)
        } else {
            Err(anyhow!("Path is not a file or directory: {}", path.display()))
        }
    }

    /// Remove an installed model.
    pub fn remove_model(&self, path: &Path) -> Result<()> {
        if path.is_file() {
            fs::remove_file(path)?;
        } else if path.is_dir() {
            fs::remove_dir_all(path)?;
        }
        Ok(())
    }

    /// Format bytes to human-readable string.
    pub fn format_size(bytes: u64) -> String {
        if bytes >= 1024 * 1024 * 1024 {
            format!("{:.1} GB", bytes as f64 / (1024.0 * 1024.0 * 1024.0))
        } else if bytes >= 1024 * 1024 {
            format!("{:.1} MB", bytes as f64 / (1024.0 * 1024.0))
        } else if bytes >= 1024 {
            format!("{:.1} KB", bytes as f64 / 1024.0)
        } else {
            format!("{} B", bytes)
        }
    }
}

impl Default for Store {
    fn default() -> Self {
        Self::new()
    }
}
