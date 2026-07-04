use serde::Deserialize;
use std::collections::HashMap;

/// Raw catalog entry from `catalog.json`.
#[derive(Debug, Clone, Deserialize)]
struct RawCatalogModel {
    id: String,
    slug: String,
    name: String,
    #[serde(default)]
    description: String,
    languages: Option<Vec<String>>,
    capabilities: Option<RawCapabilities>,
    files: Vec<RawQuantFile>,
    default_quant: Option<String>,
    speed_score: f32,
    accuracy_score: f32,
    recommended: bool,
    #[serde(default)]
    recommended_rank: Option<u32>,
}

#[derive(Debug, Clone, Deserialize)]
struct RawCapabilities {
    #[serde(default)]
    streaming: bool,
    #[serde(default)]
    translate: bool,
    #[serde(default)]
    lang_detect: bool,
    #[allow(dead_code)]
    #[serde(default)]
    timestamps: String,
}

#[derive(Debug, Clone, Deserialize)]
struct RawQuantFile {
    filename: String,
    quant: String,
    size_bytes: u64,
}

/// Engine type for a model.
#[derive(Debug, Clone, PartialEq)]
pub enum EngineType {
    TranscribeCpp,
    Parakeet,
    Moonshine,
    MoonshineStreaming,
    SenseVoice,
    GigaAM,
    Canary,
    Cohere,
}

impl EngineType {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::TranscribeCpp => "transcribe-cpp",
            Self::Parakeet => "parakeet",
            Self::Moonshine => "moonshine",
            Self::MoonshineStreaming => "moonshine-streaming",
            Self::SenseVoice => "sense-voice",
            Self::GigaAM => "gigaam",
            Self::Canary => "canary",
            Self::Cohere => "cohere",
        }
    }
}

/// A single downloadable quantization of a model.
#[derive(Debug, Clone)]
pub struct QuantFile {
    pub filename: String,
    pub quant: String,
    pub size_bytes: u64,
}

/// Parsed catalog model entry.
#[allow(dead_code)]
#[derive(Debug, Clone)]
pub struct CatalogModel {
    /// Full model ID (e.g. `handy-computer/canary-1b-v2-gguf/canary-1b-v2-Q5_K_M.gguf`)
    pub id: String,
    /// Short slug for ONNX builtins (e.g. `parakeet-tdt-0.6b-v3`)
    pub slug: String,
    pub name: String,
    pub description: String,
    pub engine_type: EngineType,
    pub languages: Vec<String>,
    pub supports_translation: bool,
    pub supports_streaming: bool,
    pub supports_language_detection: bool,
    pub files: Vec<QuantFile>,
    pub default_quant: Option<String>,
    pub speed_score: f32,
    pub accuracy_score: f32,
    pub recommended: bool,
    pub recommended_rank: Option<u32>,
    /// HuggingFace repo ID for downloads (e.g. `handy-computer/canary-1b-v2-gguf`)
    pub hf_repo: String,
}

/// The full model catalog: GGUF from catalog.json + ONNX builtins.
#[derive(Clone)]
pub struct Catalog {
    models: HashMap<String, CatalogModel>,
}

impl Catalog {
    /// Build the catalog from the bundled `catalog.json` plus ONNX builtins.
    pub fn new() -> Self {
        let raw: serde_json::Value =
            serde_json::from_str(include_str!("../../src-tauri/src/catalog/catalog.json"))
                .expect("catalog.json must be valid JSON");

        let mut models = HashMap::new();

        // Parse GGUF models from catalog.json
        if let Some(model_array) = raw["models"].as_array() {
            for val in model_array {
                let entry: RawCatalogModel =
                    serde_json::from_value(val.clone()).expect("catalog entry must be valid");
                Self::add_catalog_model(&entry, &mut models);
            }
        }

        // Add ONNX builtins (replicated from model.rs)
        Self::add_onnx_builtins(&mut models);

        Self { models }
    }

    fn add_catalog_model(entry: &RawCatalogModel, models: &mut HashMap<String, CatalogModel>) {
        let langs = entry.languages.clone().unwrap_or_default();
        let caps = entry.capabilities.as_ref();
        let supports_translation = caps.map(|c| c.translate).unwrap_or(false);
        let supports_streaming = caps.map(|c| c.streaming).unwrap_or(false);
        let supports_language_detection = caps.map(|c| c.lang_detect).unwrap_or(false);

        let files: Vec<QuantFile> = entry
            .files
            .iter()
            .map(|f| QuantFile {
                filename: f.filename.clone(),
                quant: f.quant.clone(),
                size_bytes: f.size_bytes,
            })
            .collect();

        // Extract HF repo from id: "handy-computer/repo/file.gguf" -> "handy-computer/repo"
        let parts: Vec<&str> = entry.id.split('/').collect();
        let hf_repo = if parts.len() >= 2 {
            format!("{}/{}", parts[0], parts[1])
        } else {
            entry.id.clone()
        };

        // Default ID: repo/default_quant_file.gguf
        let default_file = Self::default_quant_file(&files, entry.default_quant.as_deref());
        let full_id = if let Some(f) = default_file {
            format!("{}/{}", hf_repo, f.filename)
        } else {
            entry.id.clone()
        };

        // Also register each individual quant as an alternative ID
        for file in &files {
            let quant_id = format!("{}/{}", hf_repo, file.filename);
            let model = CatalogModel {
                id: full_id.clone(),
                slug: entry.slug.clone(),
                name: entry.name.clone(),
                description: entry.description.clone(),
                engine_type: EngineType::TranscribeCpp,
                languages: langs.clone(),
                supports_translation,
                supports_streaming,
                supports_language_detection,
                files: files.clone(),
                default_quant: entry.default_quant.clone(),
                speed_score: entry.speed_score,
                accuracy_score: entry.accuracy_score,
                recommended: entry.recommended,
                recommended_rank: entry.recommended_rank,
                hf_repo: hf_repo.clone(),
            };
            models.insert(quant_id, model);
        }

        // The slug also resolves to the default quant
        let model = models.get(&full_id).cloned();
        if let Some(m) = model {
            models.insert(entry.slug.clone(), m);
        }
    }

    fn default_quant_file<'a>(
        files: &'a [QuantFile],
        default_quant: Option<&str>,
    ) -> Option<&'a QuantFile> {
        files
            .iter()
            .find(|f| Some(f.quant.as_str()) == default_quant)
            .or_else(|| files.first())
    }

    fn add_onnx_builtins(models: &mut HashMap<String, CatalogModel>) {
        // ONNX builtins from model.rs (hardcoded constants)
        struct OnnxDef<'a> {
            slug: &'a str,
            name: &'a str,
            desc: &'a str,
            filename: &'a str,
            engine: EngineType,
            langs: &'a [&'a str],
            lang_detect: bool,
        }

        let onnx_models: Vec<OnnxDef> = vec![
            OnnxDef {
                slug: "parakeet-tdt-0.6b-v2",
                name: "Parakeet V2",
                desc: "English only. The best model for English speakers.",
                filename: "parakeet-tdt-0.6b-v2-int8",
                engine: EngineType::Parakeet,
                langs: &["en"],
                lang_detect: false,
            },
            OnnxDef {
                slug: "parakeet-tdt-0.6b-v3",
                name: "Parakeet V3",
                desc: "Fast and accurate. Supports 25 European languages.",
                filename: "parakeet-tdt-0.6b-v3-int8",
                engine: EngineType::Parakeet,
                langs: &[
                    "bg", "hr", "cs", "da", "nl", "en", "et", "fi", "fr", "de", "el", "hu", "it",
                    "lv", "lt", "mt", "pl", "pt", "ro", "sk", "sl", "es", "sv", "ru", "uk",
                ],
                lang_detect: true,
            },
            OnnxDef {
                slug: "moonshine-base",
                name: "Moonshine Base",
                desc: "Very fast, English only. Handles accents well.",
                filename: "moonshine-base",
                engine: EngineType::Moonshine,
                langs: &["en"],
                lang_detect: false,
            },
            OnnxDef {
                slug: "moonshine-tiny-streaming-en",
                name: "Moonshine V2 Tiny",
                desc: "Ultra-fast, English only",
                filename: "moonshine-tiny-streaming-en",
                engine: EngineType::MoonshineStreaming,
                langs: &["en"],
                lang_detect: false,
            },
            OnnxDef {
                slug: "moonshine-small-streaming-en",
                name: "Moonshine V2 Small",
                desc: "Fast, English only. Good balance of speed and accuracy.",
                filename: "moonshine-small-streaming-en",
                engine: EngineType::MoonshineStreaming,
                langs: &["en"],
                lang_detect: false,
            },
            OnnxDef {
                slug: "moonshine-medium-streaming-en",
                name: "Moonshine V2 Medium",
                desc: "English only. High quality.",
                filename: "moonshine-medium-streaming-en",
                engine: EngineType::MoonshineStreaming,
                langs: &["en"],
                lang_detect: false,
            },
            OnnxDef {
                slug: "sense-voice-int8",
                name: "SenseVoice",
                desc: "Very fast. Chinese, English, Japanese, Korean, Cantonese.",
                filename: "sense-voice-int8",
                engine: EngineType::SenseVoice,
                langs: &["zh", "en", "yue", "ja", "ko"],
                lang_detect: true,
            },
            OnnxDef {
                slug: "gigaam-v3-e2e-ctc",
                name: "GigaAM v3",
                desc: "Russian speech recognition. Fast and accurate.",
                filename: "giga-am-v3-int8",
                engine: EngineType::GigaAM,
                langs: &["ru"],
                lang_detect: true,
            },
            OnnxDef {
                slug: "canary-180m-flash",
                name: "Canary 180M Flash",
                desc: "Very fast. English, German, Spanish, French. Supports translation.",
                filename: "canary-180m-flash",
                engine: EngineType::Canary,
                langs: &["en", "de", "es", "fr"],
                lang_detect: true,
            },
            OnnxDef {
                slug: "canary-1b-v2",
                name: "Canary 1B v2",
                desc: "Accurate multilingual. 25 European languages. Supports translation.",
                filename: "canary-1b-v2",
                engine: EngineType::Canary,
                langs: &[
                    "bg", "hr", "cs", "da", "nl", "en", "et", "fi", "fr", "de", "el", "hu", "it",
                    "lv", "lt", "mt", "pl", "pt", "ro", "sk", "sl", "es", "sv", "ru", "uk",
                ],
                lang_detect: false,
            },
            OnnxDef {
                slug: "cohere-int8",
                name: "Cohere",
                desc: "A large, slower, but very accurate multilingual model.",
                filename: "cohere-int8",
                engine: EngineType::Cohere,
                langs: &[
                    "en", "fr", "de", "it", "es", "pt", "el", "nl", "pl", "zh", "ja", "ko", "vi",
                    "ar",
                ],
                lang_detect: true,
            },
        ];

        for def in onnx_models {
            let slug = def.slug;
            let name = def.name;
            let desc = def.desc;
            let filename = def.filename;
            let engine = def.engine;
            let langs = def.langs;
            let lang_detect = def.lang_detect;
            let model = CatalogModel {
                id: slug.to_string(),
                slug: slug.to_string(),
                name: name.to_string(),
                description: desc.to_string(),
                engine_type: engine.clone(),
                languages: langs.iter().map(|s| s.to_string()).collect(),
                supports_translation: matches!(engine, EngineType::Canary | EngineType::Parakeet),
                supports_streaming: false,
                supports_language_detection: lang_detect,
                files: vec![QuantFile {
                    filename: filename.to_string(),
                    quant: "int8".to_string(),
                    size_bytes: 0,
                }],
                default_quant: None,
                speed_score: 0.8,
                accuracy_score: 0.8,
                recommended: false,
                recommended_rank: None,
                hf_repo: slug.to_string(),
            };
            models.insert(slug.to_string(), model);
        }
    }

    /// Get a model by ID or slug. Resolves partial IDs (e.g. just the quant filename).
    pub fn get(&self, id: &str) -> Option<&CatalogModel> {
        // Direct match first
        if let Some(m) = self.models.get(id) {
            return Some(m);
        }

        // Try matching by slug
        self.models.values().find(|m| m.slug == id)
    }

    /// List all models (deduplicated by slug).
    pub fn list(&self, recommended_only: bool) -> Vec<&CatalogModel> {
        let mut seen = std::collections::HashSet::new();
        let mut result: Vec<&CatalogModel> = self
            .models
            .values()
            .filter(|m| {
                if !seen.insert(m.slug.clone()) {
                    return false;
                }
                if recommended_only && !m.recommended {
                    return false;
                }
                true
            })
            .collect();

        result.sort_by(|a, b| {
            let ra = a.recommended_rank.unwrap_or(u32::MAX);
            let rb = b.recommended_rank.unwrap_or(u32::MAX);
            ra.cmp(&rb)
                .then(a.name.cmp(&b.name))
        });

        result
    }

    /// Check if a model ID exists in the catalog.
    #[allow(dead_code)]
    pub fn contains(&self, id: &str) -> bool {
        self.get(id).is_some()
    }
}

impl Default for Catalog {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalog_contains_canary_gguf() {
        let catalog = Catalog::new();
        assert!(catalog.contains("handy-computer/canary-1b-v2-gguf/canary-1b-v2-Q5_K_M.gguf"));
    }

    #[test]
    fn catalog_contains_parakeet_onnx() {
        let catalog = Catalog::new();
        assert!(catalog.contains("parakeet-tdt-0.6b-v3"));
    }

    #[test]
    fn catalog_list_returns_models() {
        let catalog = Catalog::new();
        let models = catalog.list(false);
        assert!(!models.is_empty());
    }
}
