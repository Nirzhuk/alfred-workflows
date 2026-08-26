//! Native Gemini provider harness (Plan 038).
//!
//! The Gemini CLI adapter in `crate::agents::gemini` is untouched and stays the
//! only path that talks to a locally installed `gemini` binary. This module is
//! a separate harness that speaks the official Gemini API directly.
//!
//! Selected surface: a user-supplied Gemini API auth key sent as
//! `x-goog-api-key` to `generativelanguage.googleapis.com`. The other three
//! documented surfaces — Google desktop OAuth/ADC, Vertex AI, and the consumer
//! Gemini plan that Gemini CLI's Google-account login grants — are recorded as
//! BLOCKED with their reasons in [`surface`]. See `plans/038-gemini-native-harness.md`.

mod credential;
mod protocol;
mod runtime;
pub mod surface;
mod transport;

#[cfg(test)]
mod tests;

pub use runtime::{native_gates, native_ready, register};
pub use surface::{
    blocked_surface_codes, GeminiAuthSurface, GeminiSurfaceEvidence, GeminiSurfaceStatus,
    GEMINI_AUTH_SURFACES, SELECTED_SURFACE,
};

/// Version of this harness, independent of the Gemini CLI adapter's version.
pub const GEMINI_NATIVE_RUNTIME_ID: &str = "gemini-native";
pub const GEMINI_NATIVE_RUNTIME_VERSION: &str = "1.0.0";

/// Host and version prefix for the selected surface. Never configurable from
/// workflow JSON: a workflow cannot redirect a turn at another endpoint.
pub const GEMINI_API_HOST: &str = "https://generativelanguage.googleapis.com";
pub const GEMINI_API_VERSION: &str = "v1beta";
/// The only credential header this harness ever sets.
pub const GEMINI_API_KEY_HEADER: &str = "x-goog-api-key";
