//! Managed OpenCode Go runtime boundary.
//!
//! The adapter is deliberately fail-closed in production until the commercial,
//! sealed-package, and packaged live-smoke gates recorded in
//! [`native_release_gate`] are complete. Supervisor HTTP, secret-entry, and
//! host-approval bridges are implemented and wired by the control plane.
//! Its testable components never inspect a user's OpenCode installation or
//! route requests to Zen or another upstream provider.

mod account;
mod launch;
mod package;
mod protocol;
mod runtime;
mod transport;

pub use account::*;
pub use launch::*;
pub use package::*;
pub use protocol::*;
pub use runtime::*;
pub use transport::*;

#[cfg(test)]
pub(crate) mod fake_server;
#[cfg(test)]
mod tests;

use crate::agents::native::{NativeErrorCode, NativeRuntimeError, NativeRuntimeRegistry};

/// Production registration stays blocked until every release gate is proven.
pub fn register(_registry: &NativeRuntimeRegistry) -> Result<(), NativeRuntimeError> {
    Err(NativeRuntimeError::new(
        NativeErrorCode::ProviderUnavailable,
        format!("{COMMERCIAL_GATE_CODE}; {PACKAGE_GATE_CODE}; {LIVE_SMOKE_GATE_CODE}"),
        false,
    ))
}
