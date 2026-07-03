//! Remote transcription: one Handy instance serves inference over HTTP, another
//! consumes it.
//!
//! - [`client`] — the client side (`RemoteClient`), used by the transcription
//!   manager when `transcription_backend == Remote`.
//! - [`serve`] — the server side (`--serve`), an axum router over the
//!   transcription manager.
//! - [`protocol`] — shared wire types.

pub mod client;
pub mod protocol;
pub mod serve;

pub use client::RemoteClient;
pub use protocol::ServerHealth;
pub use serve::start_server;
