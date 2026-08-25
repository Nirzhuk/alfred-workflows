//! Plan 037: native GitHub Copilot harness (`AgentHarness::Alfred`).
//!
//! This slice is **separate from** the `copilot` CLI adapter in
//! `crate::agents::github_copilot`, which is untouched and keeps working for
//! `AgentHarness::Cli`. Nothing here reads the CLI's keychain entry, its
//! `~/.config/copilot` state, or scans `PATH`.
//!
//! # Step 1 evidence (re-read 2026-08-25)
//!
//! Source: <https://github.com/github/copilot-sdk>, its `rust/README.md`, and
//! <https://docs.github.com/en/copilot/how-tos/copilot-sdk/auth/authenticate>.
//!
//! - **Rust integration path**: official first-party crate `github-copilot-sdk`
//!   (MIT), published on crates.io; latest stable `1.0.11`, latest preview
//!   `1.0.12-preview.0` (2026-08-20). Six official SDKs exist (Node, Python,
//!   Go, .NET, Rust, Java).
//! - **Runtime**: the SDK speaks JSON-RPC to the Copilot CLI in `--server`
//!   mode and "manages the CLI process lifecycle automatically".
//! - **CLI custody**: the Rust crate's default `bundled-cli` feature "embeds
//!   the verified child-process runtime in your compiled crate" and lazily
//!   extracts it to a per-user cache keyed by the pinned CLI version. Binary
//!   resolution order is explicit path → `COPILOT_CLI_PATH` → bundled archive;
//!   the README states "There is no PATH scanning." So native mode does **not**
//!   require a user-installed CLI.
//! - **Version pin**: stable SDK `1.0.11` requires Rust 1.94 and its published
//!   `cli-version.txt` pins Copilot CLI `1.0.79`. Updates therefore ride an
//!   Alfred dependency bump, not a silent background download.
//! - **Redistribution**: the CLI itself is proprietary but its licence grants
//!   redistribution when the software is unmodified, shipped as part of an
//!   application with material functionality beyond it, not standalone, with
//!   the licence copy and notices retained, and with the host application
//!   licensed independently. Alfred satisfies all five conditions.
//! - **Token custody**: the SDK exposes **no** login/device-flow API. Alfred
//!   therefore runs its own documented GitHub OAuth device flow ([`auth`]) and
//!   the future linked transport must use `ClientOptions::with_github_token`
//!   plus `with_use_logged_in_user(false)`. It must not fall back to a CLI
//!   keychain/config or ambient GitHub token. Classic `ghp_` tokens are not
//!   supported by the SDK and are rejected here.
//! - **Tool policy**: SDK defaults expose Copilot CLI first-party tools. The
//!   linked transport must instead use `ClientMode::Empty` and admit only the
//!   `custom:alfred_*` tools in [`transport::CopilotSessionPolicy`].
//! - **Local vs cloud**: the CLI server runs locally; tool execution stays on
//!   the Alfred tool boundary ([`crate::agents::native::AlfredToolExecutor`]).
//!   Model inference is remote, as with every provider.
//!
//! ## Remaining gate
//!
//! The provider remains **BLOCKED** on the shared packaging action: adding
//! `github-copilot-sdk` to `src-tauri/Cargo.toml` changes the **shared** build
//! (the default `bundled-cli` feature downloads and embeds the pinned CLI at
//! build time), and Alfred must add the proprietary CLI license and notices to
//! every installer. Neither action belongs to this provider slice. Until both
//! are verified, [`runtime::GithubCopilotNativeRuntime`] is constructed with
//! [`transport::UnlinkedSdkTransport`], which fails closed with
//! `provider_unavailable` and never invents a direct HTTP call to Copilot.
//! Swapping in a real `SdkTransport` is a single [`transport::CopilotTransport`]
//! implementation; every mapping, bound, and state below is already exercised
//! by [`tests`].

pub mod auth;
pub mod entitlement;
pub mod events;
pub mod runtime;
pub mod transport;

#[cfg(test)]
mod tests;

/// Build-time public client id for Alfred's Copilot OAuth app.
///
/// Deliberately separate from `ALFRED_GITHUB_APP_CLIENT_ID` (the connected-apps
/// GitHub App): a Copilot seat grant is a different authorization surface with
/// different org approval, and mixing them would let one revocation silently
/// break the other.
pub fn copilot_client_id() -> Option<&'static str> {
    option_env!("ALFRED_COPILOT_CLIENT_ID")
        .map(str::trim)
        .filter(|value| !value.is_empty())
}
