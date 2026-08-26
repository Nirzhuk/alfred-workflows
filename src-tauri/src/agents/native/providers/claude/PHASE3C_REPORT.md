# Phase 3C managed Claude Code subscription runtime

## Outcome

This phase adds backend primitives for `claude_code_subscription` around the
exact, unmodified publisher binary Claude Code 2.1.246. The existing
`claude_api` direct Messages API runtime remains a separate API-billed product;
its execution implementation was not changed. Production subscription
registration deliberately remains fail-closed.

The managed runtime:

- freezes all eight official 2.1.246 desktop artifacts, upstream commit/build
  metadata, sizes, SHA-256 digests, publisher identities, detached-manifest
  signature source, signing-key fingerprint, license/notice expectations,
  update policy, and rollback policy in provider-owned code;
- accepts only a sealed `RuntimePackageVerification` produced through a shared
  publisher-verifier boundary, stages through `RuntimePackageStore`, and starts
  only `RuntimePackageSelection::verified_active_executable_path()`; it has no
  executable-name or installed-CLI fallback;
- launches the binary directly in a real cross-platform PTY with an isolated,
  account-bound `RuntimeProfile`, `CLAUDE_CONFIG_DIR`, isolated HOME/TEMP,
  code-owned PATH, cleared ambient environment, and disabled self-update;
- exposes opaque session ID, snapshot, resize, bounded base64 byte output,
  bounded byte input, cancel, and bounded wait operations for a later Tauri
  terminal relay. PTY bytes are never interpreted as OAuth URLs, codes, tokens,
  or provider prompts and are removed from the bounded queue when read;
- exposes unmodified onboarding, interactive use, `claude auth login`, and
  `claude auth logout`, plus bounded documented `claude auth status` JSON via
  `ManagedRuntimeSupervisor`;
- classifies safe status fields without reading a Claude profile, credential
  file, keychain, token, or auth code, and reports `ANTHROPIC_API_KEY` precedence
  as `environment_api_key` with warning code
  `claude_api_key_overrides_subscription` rather than claiming subscription
  billing; and
- performs Unix process-group and Windows kill-on-close Job Object cleanup on
  cancellation, output overflow, crash, normal exit with descendants, setup
  failure, and session drop.

There is no `claude -p` or Agent SDK custom-renderer subscription path.
Interactive onboarding supplies no flags, so every publisher-built auth choice
remains available inside Claude Code's own TUI.

## Changed paths

- `src-tauri/Cargo.toml`
- `src-tauri/Cargo.lock`
- `src-tauri/src/agents/native/providers/claude/mod.rs`
- `src-tauri/src/agents/native/providers/claude/auth.rs`
- `src-tauri/src/agents/native/providers/claude/package.rs`
- `src-tauri/src/agents/native/providers/claude/status.rs`
- `src-tauri/src/agents/native/providers/claude/subscription.rs`
- `src-tauri/src/agents/native/providers/claude/subscription_tests.rs`
- `src-tauri/src/agents/native/providers/claude/terminal.rs`
- `src-tauri/src/agents/native/providers/claude/resources/CLAUDE_CODE_LICENSE.txt`
- `src-tauri/src/agents/native/providers/claude/resources/NOTICE.txt`
- `src-tauri/src/agents/native/providers/claude/PHASE3C_REPORT.md`

## Dependency rationale

`portable-pty = 0.9.0` is the sole new direct dependency. It is the maintained
WezTerm PTY crate and provides one boring API over Unix PTYs and Windows
ConPTY, including direct command construction, native resize, split reader and
writer handles, child identity, and lifecycle control. Provider code adds the
required process-tree boundary: the PTY-created Unix session/process group is
signalled by exact group ID, while Windows places the exact spawned child in a
kill-on-close Job Object. The lockfile additions are only portable-pty's
resolved transitive graph; existing ambiguous `cfg_aliases` and `winreg`
references were version-qualified by Cargo.

## Pinned publisher evidence

- Version: `2.1.246`
- Commit: `1ba9d2211ae14e591bd1d60451c217c51f415e86`
- Build date: `2026-08-25T18:46:33Z`
- Release manifest:
  `https://downloads.claude.ai/claude-code-releases/2.1.246/manifest.json`
- Detached signature:
  `https://downloads.claude.ai/claude-code-releases/2.1.246/manifest.json.sig`
- Signing key: `https://downloads.claude.ai/keys/claude-code.asc`
- Pinned fingerprint: `31DD DE24 DDFA B679 F42D 7BD2 BAA9 29FF 1A7E CACE`
- Official install/signature guidance:
  `https://code.claude.com/docs/en/installation`
- Official auth/CLI guidance:
  `https://code.claude.com/docs/en/authentication` and
  `https://code.claude.com/docs/en/cli-usage`

The release-manifest parser permits publisher-added top-level fields but
requires the exact pinned version, commit, build date, exactly eight platform
entries, and exact binary filename, size, and digest for every entry. A
downloaded manifest or serialized boolean cannot mint the substrate's sealed
verification capability.

## Fake runtime coverage added

Provider-local fixtures exercise the sealed fake package and copied test binary
through the real PTY and shared supervisor for:

- first-login choice display, byte input, byte output, and resize;
- already logged-in status, logged-out status, and API-key precedence
  disclosure;
- exact status and logout commands;
- input and output bounds;
- crash exit reporting, cancellation, setup/drop process-tree cleanup;
- publisher manifest, detached signature, and sealed package mismatch;
- profile/product mismatch; and
- missing active managed executable with no installed `claude` fallback.

## Release gates and remaining integration hooks

`register_subscription` is intentionally blocked by four independent codes:

1. `claude_commercial_terms_unconfirmed`
2. `claude_managed_package_integration_missing`
3. `claude_publisher_verification_integration_missing`
4. `claude_packaged_no_cli_smoke_missing`

`claude_native_workflow_renderer_approval_missing` is reported separately and
does not get converted into a managed terminal permission. Written Anthropic
approval is still required before adding an Agent SDK or `claude -p` custom
renderer.

Later integration must:

1. implement the shared platform publisher verifier that authenticates the
   detached manifest signature/fingerprint, artifact digest, Apple Developer ID
   plus notarization, or Windows Authenticode publisher as applicable, then
   mints the sealed substrate verification;
2. download and stage the exact target artifact with the embedded legal
   resources, then connect verified selection/rollback to the desktop package
   lifecycle;
3. resolve the opaque account profile reference to an active `RuntimeProfile`
   in the account service without putting profile paths or contents in a DTO;
4. add a backend-owned Tauri session manager and small command/event relay for
   session start, input, resize, output drain, status, cancel, wait, and logout;
5. obtain and record applicable commercial/distribution authorization, then run
   signed packaged smoke tests on every supported desktop target with no user
   CLI installed, including login/account switch/disconnect/reinstall/profile
   deletion and Windows Job Object behavior; and
6. only after those gates pass, call registration from the shared provider
   registry. Native workflow rendering remains a different approval-gated
   integration.

## Validation status and commands

Per dispatch instruction, no formatter, tests, Cargo checks, or builds were run.
`cargo metadata --manifest-path src-tauri/Cargo.toml --format-version 1` was run
only to resolve the new dependency into `Cargo.lock`; official manifest and
embedded legal-resource hashes were inspected read-only. A path-scoped
`git diff --check` passed for the owned Cargo and Claude-provider changes.

Recommended validation after review:

```sh
cargo test --locked --manifest-path src-tauri/Cargo.toml agents::native::providers::claude::subscription_tests --no-fail-fast
cargo test --locked --manifest-path src-tauri/Cargo.toml agents::native::providers::claude --no-fail-fast
cargo test --locked --manifest-path src-tauri/Cargo.toml runtime_package --no-fail-fast
cargo test --locked --manifest-path src-tauri/Cargo.toml runtime_profile --no-fail-fast
cargo test --locked --manifest-path src-tauri/Cargo.toml managed_runtime --no-fail-fast
cargo check --locked --manifest-path src-tauri/Cargo.toml
git diff --check
```

The required final acceptance remains the signed, packaged, no-installed-CLI
smoke matrix rather than fixture success alone.
