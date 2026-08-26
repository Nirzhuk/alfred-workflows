# Plan 033: Run Codex through Alfred with ChatGPT OAuth

> **Executor instructions**: This is the first native-provider implementation.
> It must preserve the existing Codex CLI adapter and add a separate Alfred
> harness path. Re-read the current OpenAI app-server protocol before coding;
> do not rely on copied private HTTP endpoints or reverse-engineered CLI files.
>
> **Official references**:
>
> - [Codex authentication](https://developers.openai.com/codex/auth)
> - [Codex app-server protocol](https://github.com/openai/codex/blob/main/codex-rs/app-server/README.md)
> - [Codex repository license](https://github.com/openai/codex/blob/main/LICENSE)

## Status

- **Priority**: P0
- **Effort**: XL
- **Risk**: HIGH
- **Depends on**: Plans 030–032
- **Category**: native provider / OAuth
- **Planned at**: 2026-08-24
- **Implementation**: BLOCKED / packaged-runtime **NO-GO** (safe provider
  protocol/fixtures implemented; native runtime registration and release
  claims remain disabled)

## 2026-08-26 go-live re-check: packaged runtime NO-GO

Decision: **NO-GO**. Alfred must not bundle or register Codex app-server for
production in this release.

The latest stable official release is still app-server `0.149.1` at
[`rust-v0.149.1`](https://github.com/openai/codex/releases/tag/rust-v0.149.1),
published 2026-08-24. Newer releases visible during this review were
`0.150.0-alpha.*` prereleases, so the stable protocol freeze and six archive
digests remain unchanged.

The current first-party [Codex App Server documentation](https://learn.chatgpt.com/docs/app-server)
now documents app-server as the integration interface for authentication,
conversation history, approvals, and streaming. It also says the app-server
command and WebSocket transport are experimental and unsupported for
production workloads. Stdio remains the documented JSONL transport, but the
production warning names the app-server command itself. That is a Plan 033 STOP
condition for a shipping integration unless OpenAI documents a supported
production contract or approves Alfred's use.

The re-check corrected one earlier assumption about the release assets:

- The extracted macOS arm64 app-server passed `codesign --verify --strict` and
  carries `Developer ID Application: OpenAI OpCo, LLC (2DC432GLL2)`. The tagged
  release workflow signs and notarizes both macOS app-server targets.
- The extracted Windows x86_64 executable has an Authenticode security
  directory and an OpenAI OpCo, LLC code-signing certificate issued through
  Microsoft Identity Verification. The tagged Windows workflow sends every
  app-server target through Azure Trusted Signing.
- Linux release assets include Sigstore bundles. The inspected arm64 bundle
  binds the extracted binary SHA-256
  `8ef6f416012aae811595454de5004602ae73949980f9d4791d7a4497c0f86fe7`
  to OpenAI's `rust-release.yml` at `rust-v0.149.1` through Rekor.
- GitHub's release SHA-256 values were rechecked for the macOS arm64 archive,
  Windows x86_64 zip, Linux arm64 archive, and macOS app-server package. No
  separate GitHub artifact-attestation record was verified for these digests.

Those upstream signing inputs do not clear Alfred's package gate. Alfred has no
implemented pre-launch verification for Developer ID, Authenticode, and
Sigstore across all six targets. The inspected official macOS app-server
package archive also contains no `LICENSE` or `NOTICE`; the tagged source
repository contains both, but `src-tauri/tauri.conf.json` does not package them
or a Codex runtime. No signed Alfred package containing the runtime has passed
the required macOS, Windows, and Linux no-CLI smoke matrix.

The official auth API remains suitable for a future supported integration:
`account/login/start` documents managed `chatgpt` browser OAuth and
`chatgptDeviceCode`; Codex persists and refreshes those tokens. Alfred would
still have to keep that custody inside its isolated account-scoped
`CODEX_HOME`, and must never import ambient CLI state.

Consequences of the NO-GO:

- production registration remains fail-closed with stable block-reason codes;
- no runtime resource, account provider, login UI, or native-ready claim is
  added;
- the existing explicit Codex CLI adapter remains unchanged;
- the bounded protocol, fake app-server, event, approval, account, cleanup, and
  runtime-home fixtures remain non-production evidence only.

## 2026-08-25 official-source freeze and gate decision

Protocol/runtime freeze: **Codex app-server 0.149.1** at tag
[`rust-v0.149.1`](https://github.com/openai/codex/releases/tag/rust-v0.149.1),
published 2026-08-24. The frozen schema label in Alfred is
`rust-v0.149.1/app-server-schema`; the app-server documentation says generated
TypeScript/JSON schemas are specific to the Codex version that generated them,
and the initialization result does not advertise an independent protocol
version. Alfred therefore gates the initialize response shape and dedicated
runtime home, while the packaged artifact version and digest must be verified
before launch.

Official sources re-read on 2026-08-25:

| Gate | Status | Official evidence and decision |
| --- | --- | --- |
| Auth | Supported by protocol | [OpenAI Codex authentication](https://developers.openai.com/codex/auth) distinguishes ChatGPT subscription access from API-key usage-based access. The pinned [app-server protocol](https://github.com/openai/codex/blob/rust-v0.149.1/codex-rs/app-server/README.md) documents `account/login/start` with `chatgpt` and `chatgptDeviceCode`, completion/cancel/logout notifications, and states that Codex owns and refreshes the persisted ChatGPT tokens. Alfred must therefore use runtime-managed custody inside a dedicated Alfred-owned runtime home; it must not import a CLI home or store raw tokens as account metadata. |
| Runtime artifacts | Supported as release inputs | The official [0.149.1 release](https://github.com/openai/codex/releases/tag/rust-v0.149.1) publishes dedicated `codex-app-server` artifacts for aarch64/x86_64 macOS, aarch64/x86_64 Windows, and aarch64/x86_64 Linux. Native mode does not need a user Codex CLI or `find_bin`. |
| Checksums | Supported as integrity inputs | The same official GitHub release exposes SHA-256 digests for every pinned app-server archive. The exact six digests are frozen in `src-tauri/src/agents/native/providers/codex/runtime.rs`; mismatches fail closed. |
| License | Supported with notice obligations | The pinned [Apache License 2.0](https://github.com/openai/codex/blob/rust-v0.149.1/LICENSE) permits redistribution of object form subject to the license/notice and modification-notice conditions. The tagged repository includes an upstream [NOTICE](https://github.com/openai/codex/blob/rust-v0.149.1/NOTICE) attributing OpenAI Codex and Ratatui-derived MIT code; Alfred packaging must carry the license and this notice. |
| Protocol | Supported, version-coupled | The pinned [app-server README](https://github.com/openai/codex/blob/rust-v0.149.1/codex-rs/app-server/README.md) documents bounded-backpressure JSONL JSON-RPC over stdio, initialize/initialized, account/model/rate-limit queries, thread/turn streaming, server-initiated approvals, and `turn/interrupt`. WebSocket transport is explicitly experimental/unsupported and is not used. |
| Package signing | **Blocked** | The 0.149.1 release has embedded Developer ID signatures on macOS, embedded Authenticode signatures on Windows, and `.sigstore` bundles for Linux. Alfred still has no implemented, cross-platform verification and repackaging route; see the 2026-08-26 re-check above. |
| Packaged smoke | **Blocked** | No signed Alfred packages containing the pinned runtime have passed the required macOS, Windows, and Linux no-CLI smoke matrix. |
| Native-ready claim | **Blocked** | Runtime registration, account-provider enablement, and the native-ready UI claim stay off until signing verification, packaging, runtime-owned credential cleanup, and packaged smoke gates pass on every shipping desktop platform. |

The 2026-08-26 inspection corrects the earlier inference from release-asset
names: macOS and Windows binaries carry embedded platform signatures, and
Linux publishes Sigstore bundles. The unresolved gate is Alfred's missing
cross-platform verification and repackaging path, not absent upstream
signatures or an Apache-2.0 redistribution prohibition.

Reachable artifacts delivered despite the release block:

- A closed app-server method enum: no arbitrary JSON-RPC passthrough.
- Bounded JSONL frames, pending/incoming queues, request IDs, deadlines,
  unknown-ID rejection, initialization/home checks, process-exit draining, and
  redacted bounded stderr retention.
- ChatGPT browser/device-code lifecycle projections, strict returned-URL
  allow-listing, account/logout projection, model and rate-limit parsing.
- Thread/turn/item notification mapping that drops reasoning/raw-response
  surfaces, maps interruption/failure, and uses the Plan 032 normalizer for
  final redaction.
- Approval projections and exact allow/deny/cancel replies with workspace-root
  checks. The provider is not registered while the release gate is blocked.
- A dedicated, versioned, account-scoped Alfred runtime-home primitive with
  private Unix permissions and no reference to a user/global Codex home.
- Focused fake-frame tests for malformed/oversized/unknown-ID/timeout/exit/
  cancel/protocol mismatch, queue overload, login denial/timeout/account
  switch/logout, models/rate limits, prompt/tool/approval/workspace behavior,
  cleanup, and redaction.

## Goal

Allow a user with a ChatGPT/Codex plan to run Codex workflows from Alfred
without installing the Codex CLI. The user authenticates through Alfred's native
harness, while existing users may continue using `codex` through the CLI
harness.

## Current state

- `src-tauri/src/agents/codex.rs` runs `codex exec` and parses JSONL output.
- `src-tauri/src/agents/usage.rs:825–900` already launches Codex app-server for
  `initialize`, `account/read`, and `account/rateLimits/read`.
- The official app-server exposes JSON-RPC thread/turn execution, auth/account
  methods, model discovery, approvals, skills, filesystem operations, and
  interruption.

## Product contract

The UI must distinguish:

- `Codex · CLI`: requires a user-installed/authenticated Codex CLI.
- `Codex · Alfred`: requires ChatGPT OAuth inside Alfred and no user CLI.

The native path must not silently call `find_bin("codex")`. The CLI path must
remain unchanged.

## Runtime decision gate

Evaluate these implementation options in order:

1. **Bundled app-server runtime**: package the official Codex app-server binary
   for each supported desktop platform and speak its documented JSON-RPC
   protocol.
2. **In-process Codex runtime**: use compatible Apache-2.0 Codex crates if the
   required app-server lifecycle can be embedded without violating packaging,
   licensing, or update constraints.
3. **Direct Alfred implementation**: only if the official app-server cannot be
   shipped and the direct protocol is officially documented and supported.

Do not select option 3 based on endpoint discovery or copied CLI source.

## Scope

**In scope**:

- Alfred-mode Codex account login/logout/status.
- ChatGPT browser OAuth and device-code flow through official app-server APIs.
- Dedicated app-server process lifecycle or approved in-process runtime.
- JSON-RPC transport/client with bounded messages.
- Thread/turn execution, streaming, interruption, approvals, model list, and
  rate limits.
- Mapping Codex events to Plan 032 normalized events.
- Native run history and account state.
- Packaged macOS, Windows, and Linux runtime handling.

**Out of scope**:

- Removing the Codex CLI adapter.
- Importing `~/.codex/auth.json`.
- Reusing private OpenAI web endpoints without an official contract.
- Cloud execution of Alfred workflows.
- Exposing full Codex reasoning content to Alfred logs or React.
- Supporting every experimental app-server method in the first release.

## Implementation steps

### Step 1: Freeze the protocol version and packaging decision

Pin a known Codex app-server version and record its protocol/schema version in
native runtime metadata. Confirm Apache-2.0 notices and redistribution terms.
Define the update policy: Alfred runtime updates must be signed, versioned, and
rolled back safely.

**STOP** if the binary cannot be redistributed for one shipping platform, if
its license notices cannot be shipped, or if the protocol lacks a stable
initialization/auth/turn surface.

### Step 2: Build the bounded app-server transport

Implement a native Codex JSON-RPC client with:

- process launch and dedicated runtime home;
- stdin/stdout JSONL transport;
- bounded frame size and queue backpressure;
- request IDs and timeout tracking;
- stderr capture with redaction;
- cancellation and process cleanup;
- initialization handshake and protocol-version checks;
- no arbitrary method passthrough from workflow JSON.

Extract common account/rate-limit behavior from the current usage probe instead
of maintaining a second protocol implementation.

**Verify**: fake server fixtures cover malformed JSON, unknown IDs, oversized
frames, server overload, timeout, process exit, cancellation, and protocol
mismatch.

### Step 3: Implement ChatGPT OAuth through app-server

Use the documented `account/login/start` flow with `type: "chatgpt"` or the
approved device-code variant. Open only the exact provider URL returned by the
runtime through Tauri's opener allow-list.

Wait for `account/login/completed` and `account/updated`. Persist only redacted
account metadata in SQLite. Decide explicitly whether the runtime owns refresh
tokens in an isolated `CODEX_HOME` or whether Alfred receives a supported
credential envelope.

Never read or migrate a pre-existing Codex CLI credential file.

**Verify**: success, denial, cancellation, timeout, duplicate login, expired
refresh, account switch, logout, and app restart behavior.

### Step 4: Implement account/model/usage commands

Expose safe native commands for:

- account status and plan type;
- model list;
- ChatGPT rate-limit windows;
- refresh/reconnect;
- logout/disconnect.

Do not describe API-key usage as ChatGPT subscription usage. If the app-server
reports API-key mode, show it as a separate auth method and billing boundary.

### Step 5: Implement native turn execution

Map Alfred's native request to `thread/start`/`thread/resume` and `turn/start`.
Map bounded app-server notifications to Plan 032 events:

- thread/turn lifecycle;
- assistant deltas;
- command/file/edit items;
- approval requests;
- tool completion;
- usage and final result;
- interruption/failure.

Use explicit Alfred permission profiles. Never translate a saved CLI flag such
as `--full-auto` into an implicit native permission grant.

Initially support ephemeral threads. Add resume only after thread IDs and
storage ownership are verified.

**Verify**: prompt-only turn, tool turn, approval accept/deny, cancellation,
workspace boundary, empty result, rate limit, and provider error fixtures.

### Step 6: Add frontend harness/account UX

Add Codex native to the harness selector. Provide:

- Sign in with ChatGPT;
- account/plan label;
- model picker;
- usage windows;
- reconnect/logout;
- clear distinction between Alfred and CLI modes;
- native unavailable state when the packaged runtime is missing or invalid.

### Step 7: Package and smoke test

Package the runtime on every supported desktop platform. Verify code signing,
resource lookup, update behavior, runtime-home permissions, crash cleanup, and
uninstall cleanup.

## Subagent-ready ownership slices

- **Protocol client**: JSON-RPC transport, bounded queues, fake server fixtures.
- **Runtime packaging**: platform artifacts, signing, version/checksum checks.
- **OAuth/account**: login lifecycle, account state, token custody.
- **Turn/events**: thread/turn mapping, approvals, cancellation, redaction.
- **Frontend**: native Codex account/harness UI.
- **Release QA**: packaged smoke matrix and license-notice verification.

The protocol owner freezes the app-server version before parallel provider work.

## STOP conditions

- Native mode requires a user-installed Codex CLI.
- Implementation depends on scraping `~/.codex/auth.json`.
- A direct backend endpoint is only reverse-engineered or undocumented.
- Token custody cannot be isolated and deleted.
- App-server events cannot be bounded/redacted before entering Alfred state.
- ChatGPT subscription mode is confused with API-key billing.
- Packaged runtime licensing/signing is unresolved.

## Verification

Focused tests should cover the fake JSON-RPC server, account lifecycle, event
mapping, cancellation, and redaction. Then run:

```bash
bun test
bun run build:frontend
cargo test --locked --manifest-path src-tauri/Cargo.toml agents runner
cargo check --locked --manifest-path src-tauri/Cargo.toml
bun run check
```

Manual smoke is required on packaged macOS, Windows, and Linux builds:

1. Install Alfred only; do not install Codex CLI.
2. Start Codex native login.
3. Complete ChatGPT OAuth.
4. Run a bounded workspace turn.
5. Exercise approval and cancellation.
6. Read usage.
7. Log out and verify credential cleanup.

## Done criteria

- [ ] Codex native login works without Codex CLI installed.
- [ ] Existing Codex CLI workflows remain unchanged.
- [ ] Native app-server/runtime is versioned, signed, and distributable.
- [ ] OAuth/account/usage state is redacted and isolated.
- [ ] Native turns stream safely and cancel reliably.
- [ ] Focused, full, and packaged smoke gates pass.
