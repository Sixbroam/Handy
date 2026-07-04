mod catalog;
mod download;
mod engine;
mod selftest;
mod server;
mod store;
mod wire;

use anyhow::Result;
use clap::{Parser, Subcommand};
use std::net::SocketAddr;
use std::sync::Arc;

#[derive(Parser)]
#[command(name = "handy-server", version, about = "Standalone Handy transcription server")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Start the HTTP transcription server
    Serve {
        /// Bind address (host:port)
        #[arg(long, env = "HANDY_BIND", default_value = "127.0.0.1:8756")]
        bind: String,
        /// Model ID to load on startup
        #[arg(long, env = "HANDY_MODEL")]
        model: Option<String>,
        /// Default language for transcription
        #[arg(long, env = "HANDY_LANGUAGE", default_value = "auto")]
        language: String,
        /// Backend: auto, cpu, vulkan
        #[arg(long, env = "HANDY_BACKEND", default_value = "auto")]
        device: String,
        /// GPU device index
        #[arg(long, env = "HANDY_GPU_DEVICE", value_name = "N")]
        gpu_device: Option<usize>,
        /// Bearer token for authentication
        #[arg(long, env = "HANDY_TOKEN")]
        token: Option<String>,
        /// Path to file containing bearer token
        #[arg(long, env = "HANDY_TOKEN_FILE")]
        token_file: Option<String>,
        /// Allow binding non-loopback without a token
        #[arg(long)]
        insecure: bool,
        /// Allow loading ONNX models (gated by default until selftest passes)
        #[arg(long)]
        allow_onnx: bool,
        /// Idle unload timeout in seconds (0 = never)
        #[arg(long, env = "HANDY_IDLE_UNLOAD", default_value_t = 0u64)]
        idle_unload: u64,
        /// Maximum request body size in bytes
        #[arg(long, env = "HANDY_BODY_LIMIT", default_value_t = 64 * 1024 * 1024)]
        body_limit: usize,
        /// Skip startup warmup/selftest
        #[arg(long)]
        no_warmup: bool,
    },
    /// Model management commands
    Models {
        #[command(subcommand)]
        command: ModelCommands,
    },
    /// List available compute devices
    Devices,
    /// Run selftest on a model
    Selftest {
        /// Model ID to test
        #[arg(long)]
        model: String,
        /// Allow ONNX models
        #[arg(long)]
        allow_onnx: bool,
    },
    /// Token utilities
    Token {
        #[command(subcommand)]
        command: TokenCommands,
    },
    /// Transcribe a local audio file (debug tool)
    Transcribe {
        /// Path to WAV file (s16/f32, mono, 16 kHz)
        file: String,
        /// Model ID
        #[arg(long)]
        model: Option<String>,
        /// Language
        #[arg(long)]
        language: Option<String>,
    },
}

#[derive(Subcommand)]
enum ModelCommands {
    /// List available models
    List {
        /// Show all models (including non-recommended)
        #[arg(long)]
        all: bool,
    },
    /// Download a model
    Download {
        /// Model ID
        id: String,
    },
    /// Remove a downloaded model
    Remove {
        /// Model ID
        id: String,
    },
}

#[derive(Subcommand)]
enum TokenCommands {
    /// Generate a random token (32 bytes, base64url)
    Generate,
}

fn read_token(token: Option<String>, token_file: Option<String>) -> Result<Option<String>> {
    if let Some(t) = token {
        return Ok(Some(t));
    }
    if let Some(path) = token_file {
        let content = std::fs::read_to_string(&path)?;
        return Ok(Some(content.trim().to_string()));
    }
    Ok(None)
}

fn generate_token() -> String {
    use rand::{Rng, SeedableRng};
    let mut rng = rand::rngs::StdRng::from_entropy();
    let bytes: [u8; 32] = rng.gen();
    base64_url_encode(&bytes)
}

fn base64_url_encode(bytes: &[u8]) -> String {
    // Simple base64url encoding without external dependency
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    let mut result = String::new();
    let chunks = bytes.chunks(3);
    for chunk in chunks {
        let b0 = chunk[0] as u32;
        let b1 = chunk.get(1).map_or(0, |&b| b as u32);
        let b2 = chunk.get(2).map_or(0, |&b| b as u32);
        let triple = (b0 << 16) | (b1 << 8) | b2;

        result.push(TABLE[((triple >> 18) & 0x3F) as usize] as char);
        result.push(TABLE[((triple >> 12) & 0x3F) as usize] as char);
        if chunk.len() > 1 {
            result.push(TABLE[((triple >> 6) & 0x3F) as usize] as char);
        }
        if chunk.len() > 2 {
            result.push(TABLE[(triple & 0x3F) as usize] as char);
        }
    }
    result
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    let cli = Cli::parse();

    match cli.command {
        Commands::Serve {
            bind,
            model,
            language: _language,
            device,
            gpu_device,
            token,
            token_file,
            insecure,
            allow_onnx,
            idle_unload: _idle_unload,
            body_limit,
            no_warmup,
        } => {
            let token = read_token(token, token_file)?;

            // Security check: non-loopback requires token (unless --insecure)
            let addr: SocketAddr = bind.parse()?;
            if !addr.ip().is_loopback() && token.is_none() && !insecure {
                eprintln!(
                    "ERROR: Binding to non-loopback address {} requires --token or HANDY_TOKEN env var.",
                    addr
                );
                eprintln!("Use --insecure to bypass this check.");
                std::process::exit(1);
            }

            // Initialize engine
            let config = engine::EngineConfig {
                backend: engine::Backend::from_str(&device),
                gpu_device,
                allow_onnx,
            };

            let engine_manager = Arc::new(engine::EngineManager::new(config.clone()));
            engine_manager.init_backends();

            // Load model on startup if specified
            if let Some(model_id) = &model {
                let store = store::Store::new();
                let catalog = catalog::Catalog::new();

                let cat_model = catalog.get(model_id).ok_or_else(|| {
                    anyhow::anyhow!(
                        "Model '{}' not found. Use 'models list' to see available models.",
                        model_id
                    )
                })?;

                if cat_model.engine_type != crate::catalog::EngineType::TranscribeCpp {
                    return Err(anyhow::anyhow!(
                        "ONNX model loading on startup not yet supported"
                    ));
                }

                let path = store.resolve_gguf_path(&cat_model.id);

                // Download if not installed
                if !store.is_gguf_installed(&cat_model.id) {
                    eprintln!("Model {} not found, downloading...", cat_model.name);
                    download::download_gguf_from_hf(
                        &cat_model.hf_repo,
                        &cat_model.files.first().map(|f| f.filename.clone()).unwrap_or_default(),
                        &path,
                        cat_model.files.first().map(|f| f.size_bytes),
                    )
                    .await?;
                }

                eprintln!("Loading model: {}", cat_model.name);
                engine_manager.load_gguf(&cat_model.id, &path)?;
                eprintln!("Model loaded successfully");

                // Warmup selftest
                if !no_warmup {
                    eprintln!("Running warmup transcription...");
                    let warmup_audio: Vec<f32> = (0..1600)
                        .map(|i| (i as f32 / 16000.0 * 440.0 * std::f32::consts::PI * 2.0).sin() * 0.5)
                        .collect();
                    match engine_manager.transcribe(&warmup_audio, "en", false) {
                        Ok(_) => eprintln!("Warmup complete"),
                        Err(e) => eprintln!("Warmup warning: {}", e),
                    }
                }
            }

            // Build server state
            let state = server::ServerState::new(engine_manager, token, config, body_limit);

            // Start server
            if let Err(e) = server::start_server(state, addr).await {
                eprintln!("Server error: {}", e);
                std::process::exit(1);
            }
        }

        Commands::Models { command } => match command {
            ModelCommands::List { all } => {
                let catalog = catalog::Catalog::new();
                let store = store::Store::new();
                let models = catalog.list(!all);

                println!("{:<60} {:<25} {:<10} {:<8} Status", "ID", "Name", "Engine", "Size");
                println!("{}", "-".repeat(130));

                for m in models {
                    let size = m.files.first().map_or("N/A".to_string(), |f| {
                        store::Store::format_size(f.size_bytes)
                    });

                    let installed = match &m.engine_type {
                        crate::catalog::EngineType::TranscribeCpp => {
                            if store.is_gguf_installed(&m.id) {
                                "installed"
                            } else {
                                "not installed"
                            }
                        }
                        _ => {
                            if store.is_onnx_installed(&m.slug) {
                                "installed"
                            } else {
                                "not installed"
                            }
                        }
                    };

                    println!(
                        "{:<60} {:<25} {:<10} {:<8} {}",
                        m.id, m.name, m.engine_type.as_str(), size, installed
                    );
                }
            }

            ModelCommands::Download { id } => {
                let catalog = catalog::Catalog::new();
                let store = store::Store::new();

                let model = catalog.get(&id).ok_or_else(|| {
                    anyhow::anyhow!("Model '{}' not found in catalog", id)
                })?;

                if model.engine_type == crate::catalog::EngineType::TranscribeCpp {
                    let path = store.resolve_gguf_path(&model.id);
                    if store.is_gguf_installed(&model.id) {
                        println!("Model '{}' is already installed at {}", model.name, path.display());
                        return Ok(());
                    }

                    println!("Downloading {} from HuggingFace...", model.name);
                    let filename = model.files.first().map(|f| f.filename.clone()).unwrap_or_default();
                    download::download_gguf_from_hf(
                        &model.hf_repo,
                        &filename,
                        &path,
                        model.files.first().map(|f| f.size_bytes),
                    )
                    .await?;
                    println!("Download complete: {}", path.display());
                } else {
                    return Err(anyhow::anyhow!(
                        "ONNX model download not yet implemented"
                    ));
                }
            }

            ModelCommands::Remove { id } => {
                let catalog = catalog::Catalog::new();
                let store = store::Store::new();

                let model = catalog.get(&id).ok_or_else(|| {
                    anyhow::anyhow!("Model '{}' not found in catalog", id)
                })?;

                if model.engine_type == crate::catalog::EngineType::TranscribeCpp {
                    let path = store.resolve_gguf_path(&model.id);
                    if !store.is_gguf_installed(&model.id) {
                        println!("Model '{}' is not installed", model.name);
                        return Ok(());
                    }
                    store.remove_model(&path)?;
                    println!("Removed: {}", path.display());
                } else {
                    let path = store.resolve_onnx_path(&model.slug);
                    if !store.is_onnx_installed(&model.slug) {
                        println!("Model '{}' is not installed", model.name);
                        return Ok(());
                    }
                    store.remove_model(&path)?;
                    println!("Removed: {}", path.display());
                }
            }
        },

        Commands::Devices => {
            let config = engine::EngineConfig::default();
            let engine_manager = engine::EngineManager::new(config);
            engine_manager.init_backends();

            let devices = engine_manager.list_devices();
            if devices.is_empty() {
                println!("No compute devices found");
            } else {
                for d in &devices {
                    println!("{}", d);
                }
            }
        }

        Commands::Selftest { model, allow_onnx } => {
            selftest::run_selftest(&model, allow_onnx).await?;
        }

        Commands::Token { command } => match command {
            TokenCommands::Generate => {
                let token = generate_token();
                println!("{}", token);
            }
        },

        Commands::Transcribe {
            file,
            model: _model,
            language: _language,
        } => {
            // Debug tool: read a WAV file and transcribe it
            // For now, just show how to use it
            eprintln!("NOTE: This is a debug tool. The server expects raw f32 PCM.");
            eprintln!("To test the server endpoint, use:");
            eprintln!(
                "  ffmpeg -i {} -f f32le -ac 1 -ar 16000 - | \\\n   curl -X POST http://localhost:8756/transcribe \\\n     -H 'Authorization: Bearer YOUR_TOKEN' \\\n     --data-binary @-",
                file
            );

            // Check if the file exists at least
            let path = std::path::Path::new(&file);
            if !path.exists() {
                return Err(anyhow::anyhow!("File not found: {}", file));
            }
        }
    }

    Ok(())
}
