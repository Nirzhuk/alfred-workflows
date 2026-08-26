//! Stable Codex Python SDK candidate plus retained raw app-server evidence.
//!
//! The shipping candidate is the hermetic Python sidecar backed only by the
//! public exported `openai-codex==0.147.0` surface with experimental APIs
//! disabled. Production registration is still blocked by the audited missing
//! host-approval surface and release/package gates. The version-specific raw
//! app-server implementation remains here only as non-shipping research
//! evidence and never supplies the module's `register()` function.

mod account;
mod events;
mod protocol;
mod runtime;
mod sdk_package;
mod sdk_protocol;
mod sdk_runtime;
mod transport;

#[cfg(test)]
mod fake_app_server;
#[cfg(test)]
mod fake_sdk_sidecar;

pub use account::*;
pub use events::*;
pub use protocol::*;
pub use runtime::*;
pub use sdk_package::*;
pub use sdk_protocol::*;
pub use sdk_runtime::*;
pub use transport::*;

/// Official stable release frozen for Plan 033 on 2026-08-25.
pub const CODEX_APP_SERVER_VERSION: &str = "0.149.1";
pub const CODEX_APP_SERVER_TAG: &str = "rust-v0.149.1";

/// The app-server schema is version-specific rather than independently
/// versioned. This label names the exact schema freeze used by this client.
pub const CODEX_PROTOCOL_REVISION: &str = "rust-v0.149.1/app-server-schema";
