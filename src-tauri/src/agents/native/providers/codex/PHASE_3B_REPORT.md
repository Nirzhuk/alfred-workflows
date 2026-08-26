# Phase 3B — stable Codex Python SDK managed provider slice

## Outcome

The shipping candidate now uses a hermetic, account-scoped Python sidecar
source package pinned to `openai-codex==0.147.0` and
`openai-codex-cli-bin==0.147.0`. It imports only names exported from the
`openai_codex` package root and explicitly sets
`CodexConfig.experimental_api=False`. The raw 0.149.1 App Server modules remain
as research evidence, but their registration function was renamed to
`register_app_server_evidence`; the provider's production `register()` now
belongs to the stable SDK candidate and remains fail-closed.

No formatter, test, build, package build, or commit was run, as required by the
dispatch. `git diff --check` and source-only searches are the only validations
performed during this slice.

## Exact supported public surface

The sidecar's versioned JSONL method allowlist is:

- `capabilities`
- `login_start` for browser and device-code ChatGPT login
- `login_wait` and `login_cancel`
- `account`
- `logout`
- `models`
- `thread_start` and `thread_resume`
- `turn_start` with assistant-text streaming
- `turn_cancel`, backed by public `TurnHandle.interrupt()`
- `approval_decide`, which always returns the stable blocker below
- `shutdown`

The sidecar uses the public `Codex`, `CodexConfig`, `ApprovalMode`, `Sandbox`,
and `__version__` exports only. It projects browser/device ceremonies,
token-free ChatGPT account data, model id/label/default state, thread and turn
ids, assistant deltas, and terminal turn state. It drops reasoning, raw
responses, tool payloads, usage, warnings, and all unknown SDK notifications.
Rust maps the remaining turn notifications through `NativeEventNormalizer`,
which bounds and redacts assistant text before it can reach native consumers.

Turn execution is deliberately limited to `ApprovalMode.deny_all` and
`Sandbox.read_only`. This is a non-shipping safety posture, not a claim that the
SDK has a complete no-tools mode.

## Stable blockers

- `codex_python_sdk_host_approval_unavailable`: the public exported 0.147.0
  surface has `deny_all` and `auto_review`, but no exported host approval
  callback. Private `CodexClient`, generated protocol types, raw JSON-RPC, and
  direct ChatGPT backend calls are prohibited and were not used.
- `codex_python_sdk_public_capability_audit_blocked`: production cannot claim
  the native approval/tool contract while the required host decision surface
  is absent. Linux keyring behavior under the supervisor's cleared environment
  and strict no-tools confinement also require packaged audit evidence.
- `codex_python_sdk_known_client_enterprise_clearance_missing`: the
  `alfred_desktop` client identity and enterprise/managed-account behavior have
  not been cleared with the provider.
- `codex_python_sdk_sealed_package_unverified`: source pins and wheel hashes do
  not constitute a signed, notarized, target-specific sealed package.
- `codex_python_sdk_packaged_smoke_missing`: macOS, Windows, and Linux packages
  have not passed no-system-Python/no-installed-Codex/no-user-CLI smoke tests.

The retained raw App Server evidence keeps its older blockers and is not a
fallback candidate. The SDK sidecar has no OpenAI Platform API or API-key
fallback.

## Isolation and protocol contract

- `ManagedRuntimeSupervisor` launches only a sealed
  `RuntimePackageSelection`, with an explicit canonical cwd, typed fail-closed
  stdout frames, an exact readiness frame, a bounded shutdown frame, cleared
  environment, and process-tree cleanup.
- The sidecar requires the supervisor-supplied, account-scoped `CODEX_HOME`,
  rejects symlinked roots and binaries, rejects ambient OpenAI/Codex API
  credentials and base URLs, and never searches global `PATH` for Codex.
- ChatGPT is forced as the login method. Both CLI auth and MCP OAuth custody are
  forced to keyring, self-update and web search are disabled, and any existing
  or newly-created `CODEX_HOME/auth.json` fails closed.
- Every JSONL frame is capped at 256 KiB. Both sides reject malformed frames,
  duplicate keys, unknown fields/methods, invalid identifiers, mismatched
  protocol/SDK versions, request/operation overflow, unknown correlations, and
  mismatched thread/turn events.
- Background login waits and streamed turns start only after the matching
  response frame is flushed, so the Rust side can establish operation identity
  before an event arrives.
- Logout is rejected while login or turn work is live. Rust can create a
  `CodexSdkLogoutReceipt` only after the token-free logout acknowledgement and
  supervisor stop; `purge_logged_out_codex_profile` requires that receipt and
  delegates the matching account-scoped deletion to `RuntimeProfileStore`.

## Package and legal inputs

`src-tauri/sidecars/codex-sdk/runtime-package.source.json` pins the SDK source
commit, SDK wheel/sdist SHA-256 values, all eight CLI target wheel SHA-256
values, package layout, Apache-2.0 legal hashes, and release blockers.
`sbom.cdx.json` is the checked-in minimum CycloneDX source-component
expectation; a final target SBOM is still mandatory for every sealed package.
`LEGAL.md` lists the exact upstream LICENSE and NOTICE hashes plus CPython,
dependency, and bundler legal requirements.

`CodexSdkPackageVerifier` is the provider-local boundary for a shared,
code-owned verifier. The provider preflights the exact checked-in source
manifest and legal digests, but cannot manufacture
`RuntimePackageVerification`; only shared platform verification may return the
sealed capability accepted by `RuntimePackageStore`.

## Shared integration hooks still required

1. Build one target package containing a pinned CPython 3.10+ distribution,
   the frozen sidecar executable, the matching 0.147.0 CLI binary, all locked
   Python dependencies, LICENSE/NOTICE files, and a final target SBOM.
2. Generate the final `RuntimePackageManifest` with hashes for every packaged
   file, authenticate publisher/build provenance and platform signatures, and
   implement `CodexSdkPackageVerifier` in the shared verifier layer.
3. Install/activate through `RuntimePackageStore` and pass only its sealed
   `RuntimePackageSelection` to the provider candidate launch boundary.
4. Wire the packaged executable/resource location and target build into the
   desktop bundler without adding a CLI/PATH fallback.
5. Run packaged smoke coverage with system Python, Codex installations,
   `~/.codex`, ambient OpenAI variables, and network fallback assumptions
   removed. Include OS-keyring login/logout on each desktop target.
6. Update the shared capability report and production registration only after
   all blocker codes are retired by evidence, not by provider-local flags.

## Tests added but not run

The Rust `FakePythonSidecar` fixture has no process, Python, Codex, network,
keyring, or user-profile dependency. Its tests cover both login ceremonies,
token-free DTOs, account/model/logout projection, streamed event normalization
and redaction, reasoning rejection, turn cancellation, the explicit approval
blocker, malformed/duplicate/oversized frames, unknown correlation, simulated
crash cleanup, release blockers, and target wheel pins.

Suggested validation commands for the integration owner, when running tests
and builds is permitted:

```sh
cargo test --manifest-path src-tauri/Cargo.toml agents::native::providers::codex::fake_sdk_sidecar
cargo test --manifest-path src-tauri/Cargo.toml agents::native::providers::codex
cargo check --manifest-path src-tauri/Cargo.toml
git diff --check -- src-tauri/src/agents/native/providers/codex src-tauri/sidecars/codex-sdk
rg -n 'CodexClient|generated|json.?rpc|OPENAI_API_KEY|api.openai.com' src-tauri/sidecars/codex-sdk src-tauri/src/agents/native/providers/codex/sdk_*.rs
```

The packaged smoke must invoke the sealed executable directly with an empty
ambient auth environment, an Alfred-created runtime profile, an explicit cwd,
no system Python, no installed Codex package/CLI, and no user `auth.json`.

## Changed paths

- `src-tauri/src/agents/native/providers/codex/mod.rs`
- `src-tauri/src/agents/native/providers/codex/runtime.rs`
- `src-tauri/src/agents/native/providers/codex/sdk_package.rs`
- `src-tauri/src/agents/native/providers/codex/sdk_protocol.rs`
- `src-tauri/src/agents/native/providers/codex/sdk_runtime.rs`
- `src-tauri/src/agents/native/providers/codex/fake_sdk_sidecar.rs`
- `src-tauri/src/agents/native/providers/codex/PHASE_3B_REPORT.md`
- `src-tauri/sidecars/codex-sdk/pyproject.toml`
- `src-tauri/sidecars/codex-sdk/README.md`
- `src-tauri/sidecars/codex-sdk/LEGAL.md`
- `src-tauri/sidecars/codex-sdk/runtime-package.source.json`
- `src-tauri/sidecars/codex-sdk/sbom.cdx.json`
- `src-tauri/sidecars/codex-sdk/src/alfred_codex_sidecar/__init__.py`
- `src-tauri/sidecars/codex-sdk/src/alfred_codex_sidecar/main.py`
