//! Codex app-server protocol support for the Alfred native harness.
//!
//! This module intentionally does not register a runtime yet. The pinned
//! upstream artifacts do not provide a verifiable signing chain for every
//! desktop target, so the release gate remains closed. The bounded protocol,
//! account, event, and runtime-home primitives are kept usable by fake-server
//! tests without consulting a user Codex installation or credential home.

mod account;
mod events;
mod protocol;
mod runtime;
mod transport;

#[cfg(test)]
mod fake_app_server;

pub use account::*;
pub use events::*;
pub use protocol::*;
pub use runtime::*;
pub use transport::*;

/// Official stable release frozen for Plan 033 on 2026-08-25.
pub const CODEX_APP_SERVER_VERSION: &str = "0.149.1";
pub const CODEX_APP_SERVER_TAG: &str = "rust-v0.149.1";

/// The app-server schema is version-specific rather than independently
/// versioned. This label names the exact schema freeze used by this client.
pub const CODEX_PROTOCOL_REVISION: &str = "rust-v0.149.1/app-server-schema";
