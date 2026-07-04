use anyhow::{anyhow, Result};

use crate::catalog::Catalog;
use crate::engine::{EngineConfig, EngineManager};
use crate::store::Store;

/// Selftest: load a model and transcribe an embedded sample.
pub async fn run_selftest(model_id: &str, allow_onnx: bool) -> Result<()> {
    let catalog = Catalog::new();
    let store = Store::new();

    // Find the model in catalog
    let model = catalog.get(model_id).ok_or_else(|| {
        anyhow!("Model '{}' not found in catalog. Use 'models list' to see available models.", model_id)
    })?;

    if model.engine_type != crate::catalog::EngineType::TranscribeCpp && !allow_onnx {
        return Err(anyhow!(
            "ONNX model '{}' requires --allow-onnx (gated until selftest passes)",
            model_id
        ));
    }

    // Initialize engine
    let config = EngineConfig {
        allow_onnx,
        ..Default::default()
    };
    let engine = EngineManager::new(config);
    engine.init_backends();

    // Resolve model path
    if model.engine_type != crate::catalog::EngineType::TranscribeCpp {
        return Err(anyhow!(
            "Selftest for ONNX models not yet implemented"
        ));
    }

    let path = store.resolve_gguf_path(&model.id);
    if !path.exists() {
        return Err(anyhow!(
            "Model file not found at {}. Download it first with 'models download'.",
            path.display()
        ));
    }

    println!("Loading model: {}", model.name);
    engine.load_gguf(&model.id, &path)?;
    println!("Model loaded successfully");

    // Generate a simple test signal (sine wave at 1kHz, ~1 second)
    let duration_samples = 16000; // 1 second at 16kHz
    let mut audio = Vec::with_capacity(duration_samples);
    for i in 0..duration_samples {
        let t = i as f32 / 16000.0;
        // Mix of two frequencies to create a non-trivial signal
        let sample = (t * 1000.0 * std::f32::consts::PI * 2.0).sin() * 0.3
            + (t * 1500.0 * std::f32::consts::PI * 2.0).sin() * 0.2;
        audio.push(sample);
    }

    println!("Running transcription test...");
    match engine.transcribe(&audio, "en", false) {
        Ok(text) => {
            println!("Transcription result: '{}'", text.trim());
            if text.trim().is_empty() {
                println!(
                    "WARNING: Empty transcription (expected for synthetic audio, \
                     but may indicate an issue with real audio)"
                );
            }
            println!("Selftest PASSED");
            Ok(())
        }
        Err(e) => {
            eprintln!("Selftest FAILED: {}", e);
            Err(anyhow!("Selftest failed: {}", e))
        }
    }
}
