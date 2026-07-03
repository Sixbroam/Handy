use clap::Parser;
use std::path::PathBuf;

#[derive(Parser, Debug, Clone, Default)]
#[command(name = "handy", about = "Handy - Speech to Text")]
pub struct CliArgs {
    /// Start with the main window hidden
    #[arg(long)]
    pub start_hidden: bool,

    /// Disable the system tray icon
    #[arg(long)]
    pub no_tray: bool,

    /// Toggle transcription on/off (sent to running instance)
    #[arg(long)]
    pub toggle_transcription: bool,

    /// Toggle transcription with post-processing on/off (sent to running instance)
    #[arg(long)]
    pub toggle_post_process: bool,

    /// Cancel the current operation (sent to running instance)
    #[arg(long)]
    pub cancel: bool,

    /// Enable debug mode with verbose logging
    #[arg(long)]
    pub debug: bool,

    /// Transcribe this WAV (16 kHz mono) headlessly and exit. Runs the same
    /// batch transcription path as the app — no mic, no VAD, no download
    /// (the model must already be installed).
    #[arg(short = 'f', long, value_name = "WAV")]
    pub transcribe_file: Option<PathBuf>,

    /// Model id to load for --transcribe-file (default: the selected model).
    #[arg(long)]
    pub model: Option<String>,

    /// Hard-select the compute device for --transcribe-file by its registry
    /// index (see --list-devices). Omit to use the persisted accelerator
    /// setting. transcribe-cpp (whisper-family) models only.
    #[arg(long, value_name = "N")]
    pub device_index: Option<usize>,

    /// List the transcribe-cpp compute devices (with indices) and exit.
    #[arg(long)]
    pub list_devices: bool,

    /// List the available models (with ids) and exit. Pass an id to --model.
    /// Honors --json for machine-readable output.
    #[arg(long)]
    pub list_models: bool,

    /// Repeat the transcription N times (best_ms reports the fastest run).
    #[arg(long, value_name = "N")]
    pub repeat: Option<usize>,

    /// Emit --transcribe-file results as JSON.
    #[arg(long)]
    pub json: bool,

    /// Run as a headless transcription server: load the selected model and
    /// expose it over HTTP for other Handy instances (set the other instance's
    /// backend to Remote). Pair with --serve-host/--serve-port and the
    /// `remote_server_*` settings. Designed to run under systemd on a
    /// GPU box accessible over LAN/Tailscale.
    #[arg(long)]
    pub serve: bool,

    /// Override the `remote_server_listen_addr` host for --serve (e.g. `0.0.0.0`
    /// to expose on all interfaces, or a specific Tailscale IP). Port still
    /// comes from --serve-port / the setting unless given here as `host:port`.
    #[arg(long, value_name = "HOST | HOST:PORT")]
    pub serve_host: Option<String>,

    /// Override the `remote_server_listen_addr` port for --serve.
    #[arg(long, value_name = "PORT")]
    pub serve_port: Option<u16>,

    /// Download a model headlessly and exit. Takes the same model id shown by
    /// --list-models. Useful for provisioning a --serve box over SSH before the
    /// first request (the server auto-downloads the selected model too, but this
    /// lets you fetch it explicitly and inspect progress in the journal).
    #[arg(long, value_name = "MODEL_ID")]
    pub download_model: Option<String>,
}
