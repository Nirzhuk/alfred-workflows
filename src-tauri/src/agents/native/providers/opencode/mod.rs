//! Managed OpenCode Go runtime boundary.
//!
//! The adapter is deliberately fail-closed in production until the commercial,
//! sealed-package, supervisor-capability, account-entry, approval-bridge, and
//! packaged live-smoke gates recorded in [`native_release_gate`] are complete.
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
mod fake_server;
#[cfg(test)]
mod tests;

use crate::agents::native::{NativeErrorCode, NativeRuntimeError, NativeRuntimeRegistry};

/// Production registration stays blocked until every release gate is proven.
pub fn register(_registry: &NativeRuntimeRegistry) -> Result<(), NativeRuntimeError> {
    Err(NativeRuntimeError::new(
        NativeErrorCode::ProviderUnavailable,
        format!(
            "{COMMERCIAL_GATE_CODE}; {PACKAGE_GATE_CODE}; {SUPERVISOR_HTTP_GATE_CODE}; \
             {ACCOUNT_GATE_CODE}; {APPROVAL_GATE_CODE}; {LIVE_SMOKE_GATE_CODE}"
        ),
        false,
    ))
}
