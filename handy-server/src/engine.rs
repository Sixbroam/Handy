use anyhow::{anyhow, Result};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

/// Backend selection for transcribe-cpp.
#[derive(Debug, Clone, PartialEq)]
pub enum Backend {
    Auto,
    Cpu,
    Vulkan,
}

impl Backend {
    pub fn from_str(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "cpu" => Self::Cpu,
            "vulkan" => Self::Vulkan,
            _ => Self::Auto,
        }
    }
}

/// Configuration for the engine.
#[derive(Debug, Clone)]
pub struct EngineConfig {
    pub backend: Backend,
    pub gpu_device: Option<usize>,
    pub allow_onnx: bool,
}

impl Default for EngineConfig {
    fn default() -> Self {
        Self {
            backend: Backend::Auto,
            gpu_device: None,
            allow_onnx: false,
        }
    }
}

/// Language normalization: zh-Hans/zh-Hant -> zh.
fn normalize_cjk_language(language: &str) -> &str {
    match language {
        "zh-Hans" | "zh-Hant" => "zh",
        other => other,
    }
}

/// Resolve effective language for a model.
pub fn effective_language(
    intent: &str,
    supported_languages: &[String],
    supports_language_detection: bool,
) -> String {
    if supported_languages.is_empty() {
        return intent.to_string();
    }

    // Base language matching (strip region/script suffixes)
    fn base_lang(lang: &str) -> &str {
        lang.split('-').next().unwrap_or(lang)
    }

    if intent != "auto" {
        if let Some(code) = supported_languages
            .iter()
            .find(|l| base_lang(l) == base_lang(intent))
        {
            // Chinese script variants pass through unchanged
            if intent == "zh-Hans" || intent == "zh-Hant" {
                return intent.to_string();
            }
            return code.clone();
        }
    }

    if supports_language_detection {
        return "auto".to_string();
    }

    // Fallback: prefer English, then first available
    if let Some(en) = supported_languages
        .iter()
        .find(|l| base_lang(l) == "en")
    {
        return en.clone();
    }
    supported_languages[0].clone()
}

/// Transcribe-cpp run plan: task + language options.
#[allow(dead_code)]
pub struct RunPlan {
    pub task: transcribe_cpp::Task,
    pub language: Option<String>,
    #[allow(dead_code)]
    pub target_language: Option<String>,
}

/// Build a run plan from settings and model capabilities.
fn build_run_plan(
    translate_to_english: bool,
    effective_language: &str,
    model_languages: &[String],
    model_supports_translate: bool,
) -> RunPlan {
    let requested_language = match effective_language {
        "auto" => None,
        other => Some(normalize_cjk_language(other).to_string()),
    };

    // Only pass language if model advertises it
    let language = requested_language.filter(|lang| {
        model_languages.iter().any(|l| l == lang)
    });

    // Translation task
    let translate_to_en =
        translate_to_english && model_supports_translate && language.as_deref() != Some("en");

    let (task, target_language) = if translate_to_en {
        (
            transcribe_cpp::Task::Translate,
            Some("en".to_string()),
        )
    } else {
        (transcribe_cpp::Task::Transcribe, None)
    };

    RunPlan {
        task,
        language,
        target_language,
    }
}

/// A loaded GGUF model via transcribe-cpp.
#[allow(dead_code)]
pub(crate) struct LoadedCppModel {
    #[allow(dead_code)]
    model: Arc<std::sync::Mutex<transcribe_cpp::Model>>,
    session: Arc<std::sync::Mutex<transcribe_cpp::Session>>,
    capabilities: transcribe_cpp::Capabilities,
    #[allow(dead_code)]
    backend_name: String,
}

/// The currently loaded engine.
pub enum ActiveEngine {
    Cpp(LoadedCppModel),
    // ONNX variants gated by --allow-onnx (T1.5 selftest)
    #[allow(dead_code)]
    Onnx(OnnxVariant),
}

#[derive(Debug)]
#[allow(dead_code)]
pub enum OnnxVariant {
    Parakeet,
    Moonshine,
    MoonshineStreaming,
    SenseVoice,
    GigaAM,
    Canary,
    Cohere,
}

/// Thread-safe engine manager.
pub struct EngineManager {
    config: EngineConfig,
    engine: Mutex<Option<ActiveEngine>>,
    loading: AtomicBool,
    /// Currently loaded model ID (for /health)
    current_model: Mutex<Option<String>>,
    /// Backend name actually in use
    backend_name: Mutex<Option<String>>,
}

impl EngineManager {
    pub fn new(config: EngineConfig) -> Self {
        Self {
            config,
            engine: Mutex::new(None),
            loading: AtomicBool::new(false),
            current_model: Mutex::new(None),
            backend_name: Mutex::new(None),
        }
    }

    /// Initialize transcribe-cpp backends (call once at startup).
    pub fn init_backends(&self) {
        transcribe_cpp::init_logging();
        match transcribe_cpp::init_backends_default() {
            Ok(()) => {
                let devices = transcribe_cpp::devices();
                tracing::info!(
                    "transcribe-cpp initialized with {} device(s): [{}]",
                    devices.len(),
                    devices
                        .iter()
                        .map(|d| format!("{} ({})", d.name, d.kind))
                        .collect::<Vec<_>>()
                        .join(", ")
                );
            }
            Err(e) => {
                tracing::warn!("Failed to init transcribe-cpp backends: {}", e);
            }
        }
    }

    /// List available compute devices.
    pub fn list_devices(&self) -> Vec<String> {
        transcribe_cpp::devices()
            .iter()
            .map(|d| {
                let idx = d.index.map(|i| i.to_string()).unwrap_or("-".to_string());
                let name = if d.description.is_empty() {
                    d.name.clone()
                } else {
                    d.description.clone()
                };
                let vram_mb = d.memory_total / (1024 * 1024);
                format!(
                    "index={} kind={} name={} vram={}MB",
                    idx, d.kind, name, vram_mb
                )
            })
            .collect()
    }

    /// Check if currently loading a model.
    pub fn is_loading(&self) -> bool {
        self.loading.load(Ordering::SeqCst)
    }

    /// Get the currently loaded model ID.
    pub fn get_current_model(&self) -> Option<String> {
        self.current_model.lock().unwrap().clone()
    }

    /// Get the backend name in use.
    pub fn get_backend(&self) -> Option<String> {
        self.backend_name.lock().unwrap().clone()
    }

    /// Check if a model is loaded.
    pub fn is_loaded(&self) -> bool {
        self.engine.lock().unwrap().is_some()
    }

    /// Load a GGUF model.
    pub fn load_gguf(&self, model_id: &str, path: &Path) -> Result<()> {
        // Single-flight guard
        if self.loading.swap(true, Ordering::SeqCst) {
            return Err(anyhow!("Model is already loading"));
        }

        let result = self.do_load_gguf(model_id, path);
        self.loading.store(false, Ordering::SeqCst);

        match &result {
            Ok(()) => {
                *self.current_model.lock().unwrap() = Some(model_id.to_string());
            }
            Err(_) => {
                *self.current_model.lock().unwrap() = None;
            }
        }

        result
    }

    fn do_load_gguf(&self, model_id: &str, path: &Path) -> Result<()> {
        // Drop existing model before loading new one (D6)
        {
            let mut engine = self.engine.lock().unwrap();
            *engine = None;
        }

        // Select backend
        let backend = match &self.config.backend {
            Backend::Auto => {
                #[cfg(not(target_os = "macos"))]
                let candidates = [
                    transcribe_cpp::Backend::Cuda,
                    transcribe_cpp::Backend::Vulkan,
                ];
                #[cfg(target_os = "macos")]
                let candidates = [transcribe_cpp::Backend::Metal];

                candidates
                    .iter()
                    .find(|&&b| transcribe_cpp::backend_available(b))
                    .copied()
                    .unwrap_or(transcribe_cpp::Backend::Auto)
            }
            Backend::Cpu => transcribe_cpp::Backend::Cpu,
            Backend::Vulkan => transcribe_cpp::Backend::Vulkan,
        };

        let gpu_device = self.config.gpu_device.unwrap_or(0) as i32;

        tracing::info!(
            "Loading GGUF model {} (backend={:?}, gpu_device={})",
            path.display(),
            backend,
            gpu_device
        );

        let st = std::time::Instant::now();

        let options = transcribe_cpp::ModelOptions {
            backend,
            gpu_device,
        };

        let model = transcribe_cpp::Model::load_with(path, &options)
            .map_err(|e| anyhow!("Failed to load model {}: {}", path.display(), e))?;

        let session = model.session()
            .map_err(|e| anyhow!("Failed to create session: {}", e))?;
        let capabilities = session.model().capabilities();
        let backend_name = model.backend().to_string();

        let elapsed = st.elapsed();
        tracing::info!(
            "Model loaded in {:.1}s: {} (backend={}, languages={})",
            elapsed.as_secs_f64(),
            model_id,
            backend_name,
            capabilities.languages.len()
        );

        // Store backend name
        *self.backend_name.lock().unwrap() = Some(backend_name.clone());

        // Replace engine (drop-before-load already done above)
        {
            let mut engine = self.engine.lock().unwrap();
            *engine = Some(ActiveEngine::Cpp(LoadedCppModel {
                model: Arc::new(std::sync::Mutex::new(model)),
                session: Arc::new(std::sync::Mutex::new(session)),
                capabilities,
                backend_name,
            }));
        }

        Ok(())
    }

    /// Transcribe audio using the loaded model.
    pub fn transcribe(
        &self,
        audio: &[f32],
        language: &str,
        translate_to_english: bool,
    ) -> Result<String> {
        if audio.is_empty() {
            return Ok(String::new());
        }

        let engine = self.engine.lock().unwrap();
        match engine.as_ref() {
            Some(ActiveEngine::Cpp(model)) => {
                // Determine effective language
                let model_langs = model.capabilities.languages.clone();

                let eff_lang = effective_language(
                    language,
                    &model_langs,
                    model.capabilities.supports_language_detect,
                );

                let plan = build_run_plan(
                    translate_to_english,
                    &eff_lang,
                    &model_langs,
                    model.capabilities.supports_translate,
                );

                tracing::debug!(
                    "Transcribing: {} samples (~{:.1}s), lang={:?}, task={:?}",
                    audio.len(),
                    audio.len() as f64 / 16000.0,
                    plan.language,
                    plan.task
                );

                let options = transcribe_cpp::RunOptions {
                    task: plan.task,
                    language: plan.language,
                    target_language: plan.target_language,
                    timestamps: transcribe_cpp::TimestampKind::None,
                    ..Default::default()
                };

                let result = model.session.lock().unwrap().run(audio, &options)
                    .map_err(|e| anyhow!("Transcription failed: {}", e))?;

                Ok(result.text)
            }
            Some(ActiveEngine::Onnx(_)) => {
                Err(anyhow!("ONNX models require --allow-onnx and passing selftest"))
            }
            None => Err(anyhow!("No model loaded")),
        }
    }

    /// Unload the current model.
    pub fn unload(&self) -> Result<()> {
        let mut engine = self.engine.lock().unwrap();
        *engine = None;
        *self.current_model.lock().unwrap() = None;
        *self.backend_name.lock().unwrap() = None;
        tracing::info!("Model unloaded");
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn languages(codes: &[&str]) -> Vec<String> {
        codes.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn test_normalize_cjk() {
        assert_eq!(normalize_cjk_language("zh-Hans"), "zh");
        assert_eq!(normalize_cjk_language("zh-Hant"), "zh");
        assert_eq!(normalize_cjk_language("en"), "en");
    }

    #[test]
    fn test_run_plan_transcribe() {
        let plan = build_run_plan(false, "en", &languages(&["en", "es"]), true);
        assert!(matches!(plan.task, transcribe_cpp::Task::Transcribe));
        assert_eq!(plan.language.as_deref(), Some("en"));
        assert_eq!(plan.target_language, None);
    }

    #[test]
    fn test_run_plan_translate_non_english() {
        let plan = build_run_plan(true, "es", &languages(&["en", "es"]), true);
        assert!(matches!(plan.task, transcribe_cpp::Task::Translate));
        assert_eq!(plan.language.as_deref(), Some("es"));
        assert_eq!(plan.target_language.as_deref(), Some("en"));
    }

    #[test]
    fn test_run_plan_skip_english_translation() {
        let plan = build_run_plan(true, "en", &languages(&["en", "es"]), true);
        assert!(matches!(plan.task, transcribe_cpp::Task::Transcribe));
        assert_eq!(plan.target_language, None);
    }

    #[test]
    fn test_run_plan_requires_translation_support() {
        let plan = build_run_plan(true, "es", &languages(&["en", "es"]), false);
        assert!(matches!(plan.task, transcribe_cpp::Task::Transcribe));
        assert_eq!(plan.target_language, None);
    }

    #[test]
    fn test_run_plan_chinese_variant() {
        let plan = build_run_plan(false, "zh-Hant", &languages(&["zh"]), true);
        assert!(matches!(plan.task, transcribe_cpp::Task::Transcribe));
        assert_eq!(plan.language.as_deref(), Some("zh"));
    }

    #[test]
    fn test_effective_language_auto_detect() {
        let langs = languages(&["en", "fr", "de"]);
        assert_eq!(
            effective_language("auto", &langs, true),
            "auto"
        );
    }

    #[test]
    fn test_effective_language_fallback_no_detect() {
        let langs = languages(&["en", "fr"]);
        assert_eq!(
            effective_language("auto", &langs, false),
            "en"
        );
    }
}
