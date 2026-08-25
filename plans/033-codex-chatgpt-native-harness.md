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
- **Implementation**: TODO

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
