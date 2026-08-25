//! OpenCode native-runtime policy and protocol boundary.
//!
//! OpenCode is a router. An Alfred account therefore names the real upstream
//! provider and billing owner, and every selected model repeats that provider
//! id. This module intentionally does not register a production runtime: the
//! release, account-entry, and tool-result bridge gates below are unresolved.

mod account;
mod launch;
mod package;
mod protocol;

pub use account::{
    OpenCodeAccountBinding, OpenCodeAuthKind, OpenCodeRoute, MAX_BILLING_OWNER_BYTES,
    MAX_UPSTREAM_ID_BYTES,
};
pub use package::{
    native_release_gate, OpenCodeNativeReleaseGate, OpenCodePackagePlatform,
    OPENCODE_LICENSE, OPENCODE_RUNTIME_VERSION,
};
pub use launch::OpenCodeLaunchSpec;
pub use protocol::{
    decode_server_event, map_http_failure, OpenCodeProtocolEvent, OpenCodeServerFailure,
    OpenCodeToolPermission,
};

#[cfg(test)]
mod tests;
