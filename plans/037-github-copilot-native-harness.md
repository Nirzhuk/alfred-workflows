# Plan 037: Add a native GitHub Copilot harness

> **Executor instructions**: Preserve the existing Copilot CLI adapter. Native
> mode must use an official Copilot SDK/runtime or documented API. Do not call
> undocumented Copilot endpoints or pretend a GitHub OAuth token alone is a
> complete agent runtime.
>
> **Official references**:
>
> - [Copilot CLI authentication](https://docs.github.com/en/copilot/how-tos/copilot-cli/set-up-copilot-cli/authenticate-copilot-cli)
> - [Copilot SDK](https://github.com/github/copilot-sdk)
> - [Copilot SDK authentication](https://docs.github.com/en/copilot/how-tos/copilot-sdk/auth/authenticate)

## Status

- **Priority**: P1
- **Effort**: XL
- **Risk**: HIGH
- **Depends on**: Plans 030–032
- **Category**: native provider / OAuth
- **Planned at**: 2026-08-24
- **Implementation**: **BLOCKED — shared SDK/runtime packaging gate**

## Step 1 evidence (re-read 2026-08-25) — SDK viable, packaging blocked

Sources: [`github/copilot-sdk`](https://github.com/github/copilot-sdk) and its
[`rust/README.md`](https://github.com/github/copilot-sdk/blob/main/rust/README.md);
official [SDK authentication](https://docs.github.com/en/copilot/how-tos/copilot-sdk/auth/authenticate)
and [GitHub OAuth setup](https://docs.github.com/en/copilot/how-tos/copilot-sdk/setup/github-oauth);
the [`github/copilot-cli` license](https://github.com/github/copilot-cli/blob/main/LICENSE.md);
and the crates.io package metadata inspected with `cargo info`.

| Question | Finding |
| --- | --- |
| Rust integration path | First-party crate `github-copilot-sdk` (MIT), published on crates.io. Stable `1.0.11`; preview `1.0.12-preview.0` (2026-08-20). Six official SDKs: Node, Python, Go, .NET, Rust, Java. |
| Does the SDK require a user-installed CLI? | **No, for Rust.** The default `bundled-cli` feature "embeds the verified child-process runtime in your compiled crate" and lazily extracts it to a per-user cache. Resolution order is explicit path → `COPILOT_CLI_PATH` → bundled archive; the README states "There is no PATH scanning." (The repo's "not bundled by default" note for Go/Java/Rust describes the *unbundled* build, which this plan does not use.) |
| Transport | JSON-RPC to the Copilot CLI in `--server` mode; the SDK "manages the CLI process lifecycle automatically". |
| Runtime pin/update | Stable SDK `1.0.11` requires Rust `1.94.0`; its published `cli-version.txt` pins CLI `1.0.79` with per-platform SHA-256 values. The current toolchain is Rust `1.96.0`. Updates ride an Alfred dependency bump, not a silent background download. |
| Redistribution | Copilot CLI is proprietary but its license grants redistribution only when unmodified, part of a materially larger application, non-standalone, independently licensed, and shipped with the CLI license plus notices. The legal terms are clear; Alfred's installers do **not yet have an evidenced Copilot-license payload**, so the package gate is not passed. |
| Token custody | The SDK has no login/device-flow API. Alfred may use documented GitHub OAuth and pass the resulting `gho_`/`ghu_` token through Rust `ClientOptions::with_github_token(...).with_use_logged_in_user(false)`. The provider must not use the CLI keychain/config or ambient token fallback. `github_pat_` is also accepted; classic `ghp_` is rejected. Token storage/refresh/expiry remain Alfred's responsibility. |
| Tool policy | SDK defaults expose first-party CLI tools similar to `--allow-all`. Rust `ClientMode::Empty` disables ambient CLI behavior; `available_tools` supports source-qualified `custom:*` filtering. The provider seam requires Empty mode and only `custom:alfred_*`. |
| Local vs cloud | CLI server and JSON-RPC transport are local. Alfred custom tool execution stays behind `NativeTurnHost`; model inference remains cloud unless BYOK targets a local provider. |

The SDK/runtime design does not require a user-installed CLI. However, native
mode is not ready merely because redistribution is permitted: the actual
dependency, embedded runtime, license/notice payload, and packaged smoke are
still absent from the shared build.

### Exact BLOCKED reason

`github-copilot-sdk = "=1.0.11"` is not linked in shared `src-tauri/Cargo.toml`,
and Alfred packaging has no verified step that includes the proprietary
Copilot CLI license/notices in every installer. Linking it would download and
embed CLI `1.0.79` during the shared build and requires the broad build/package
validation explicitly prohibited for this dispatch. Therefore
`transport::UnlinkedSdkTransport` reports an unavailable (not managed) runtime
and fails closed with `provider_unavailable`; no direct Copilot HTTP call is
fabricated. A live OAuth-app/seat/SSO smoke is also still required before the
account/entitlement states can be declared production-ready.

### Reachable artifacts completed in this slice

- Documented GitHub device start/poll state machine: success, pending,
  slow-down, denial, expiry, malformed token, identity mismatch, and logout
  zeroization.
- Bounded current SDK event names (`session.start`, `assistant.turn_start`,
  `assistant.message_delta`, `tool.execution_*`, `permission.completed`,
  `assistant.turn_end`, `session.idle`) plus reasoning suppression, malformed
  identifiers, oversized text refusal, rate-limit/account classification,
  approval allow/deny, and cancellation.
- Strict transport policy requiring `ClientMode::Empty`, explicit token
  custody with stored-login fallback disabled, and only `custom:alfred_*`
  tools. The existing CLI adapter is unchanged.

Targeted evidence: `cargo test --locked --manifest-path src-tauri/Cargo.toml
--lib github_copilot --no-fail-fast` passes all 42 provider fixtures. Broad
formatters, builds, and suites were intentionally not run.

### Provider-local secret boundary

`events::scrub` explicitly covers `gho_`, `ghu_`, and `github_pat_` in addition
to the shared redactor. More importantly, `runtime::run_alfred_tool` now rejects
secret material in every raw SDK field that could affect execution — invocation
ID, name, path, cwd, input, and arguments — before constructing an
`AlfredToolRequest` or calling `host.invoke_tool`. Provider fixtures exercise
each field, including structured secret-key markers. The remaining release
prerequisites are unchanged: link pinned SDK `1.0.11`/CLI `1.0.79`, ship the
required notices, run packaged platform smokes, and complete a live
OAuth/seat/SSO smoke.

## Goal

Allow users with GitHub Copilot access to authenticate through Alfred and run a
Copilot agent without manually installing or launching the Copilot CLI, while
keeping the current CLI harness available.

## Provider reality

GitHub documents OAuth device login for Copilot CLI and a Copilot SDK that
communicates with a Copilot CLI server through JSON-RPC. This may satisfy the
user experience requirement if the SDK/runtime can be bundled or managed by
Alfred, but it is not automatically a direct HTTP model API.

The native plan must decide whether the SDK's managed runtime is redistributable
and whether Alfred can securely own its lifecycle.

## Scope

**In scope**:

- Official Copilot OAuth/device authentication path.
- Copilot SDK/runtime transport evaluation.
- Bundled or SDK-managed runtime lifecycle.
- Local workspace/tool permission mapping.
- Streaming activity, approvals, cancellation, and final results.
- Model/catalog and usage/account state where officially available.
- Native account UI and billing/auth copy.

**Out of scope**:

- Calling Copilot APIs with a raw GitHub token without an SDK/API contract.
- Scraping Copilot CLI keychain/configuration.
- Assuming GitHub API OAuth equals Copilot entitlement.
- Remote Copilot control surfaces or cloud workflow execution.
- Replacing the current Copilot CLI adapter.

## Implementation steps

### Step 1: Verify SDK/runtime distribution

Read the current SDK repository and documentation. Record:

- supported languages and Rust integration path;
- whether the SDK downloads, launches, or requires Copilot CLI;
- runtime version pinning and update behavior;
- OAuth/token custody;
- workspace and tool policy;
- redistribution and packaging terms;
- local versus cloud execution.

**STOP** if a user-installed CLI remains mandatory for native mode and Alfred
cannot bundle or manage the required runtime.

### Step 2: Implement OAuth/account lifecycle

Use the documented OAuth device flow or SDK-managed login. Store Alfred's
account metadata and credential reference through Plan 031. Do not import an
existing `copilot-cli` keychain entry.

Show GitHub account identity, Copilot entitlement state, organization/SSO
requirements, and token expiry only when officially available.

### Step 3: Implement SDK/runtime adapter

Map the Plan 032 native request to the Copilot SDK/server:

- workspace root;
- prompt and skill/context block;
- model;
- permission/approval policy;
- tool calls and progress;
- cancellation;
- final response and changed files.

Normalize SDK JSON-RPC events. Bound all tool output and redact provider
credentials before Alfred persistence.

### Step 4: Implement account/model/usage states

Use supported SDK/account APIs. Distinguish:

- GitHub authentication;
- Copilot entitlement;
- API-key/BYOK mode;
- organization policy/SSO;
- unavailable quota.

Do not present GitHub login success as proof that a Copilot seat is active.

### Step 5: Add native UI and compatibility fixtures

Provide separate harness entries:

- GitHub Copilot CLI;
- GitHub Copilot · Alfred.

Fixtures must cover device-code success/denial, account mismatch, SSO/policy
denial, expired token, runtime startup failure, JSON-RPC event mapping,
approval, cancellation, rate limit, and logout.

## Subagent-ready ownership slices

- **SDK/runtime research**: official SDK, lifecycle, packaging, license.
- **OAuth/account**: device flow, entitlement, secure storage.
- **Transport**: SDK JSON-RPC, events, tools, cancellation.
- **Workspace safety**: local roots, approvals, data boundaries.
- **UI/release QA**: native settings, packaged runtime, fixtures.

## STOP conditions

- SDK requires an unbundled CLI for the promised user experience.
- Raw GitHub OAuth cannot prove Copilot entitlement or invoke supported agent
  execution.
- Runtime messages cannot be bounded/redacted.
- Organization/SSO policy cannot be surfaced safely.
- SDK/runtime licensing or update distribution is unresolved.

## Verification

```bash
bun test
bun run build:frontend
cargo test --locked --manifest-path src-tauri/Cargo.toml agents runner
cargo check --locked --manifest-path src-tauri/Cargo.toml
bun run check
```

Manual smoke must start Alfred with no Copilot CLI installed, complete the
supported login path, run a bounded workspace task, exercise an approval and
cancellation, then disconnect and verify cleanup. The existing CLI path gets a
separate regression smoke.

## Done criteria

- [x] Official Copilot SDK/API surface is selected and versioned (`github-copilot-sdk =1.0.11`, CLI `1.0.79`).
- [ ] Native mode does not require manual CLI installation.
- [x] OAuth, entitlement, SSO, and billing states are distinct in the provider state/fixtures.
- [ ] Runtime events/tools/cancellation pass Plan 032.
- [x] CLI mode remains unchanged.
- [ ] Packaged runtime and license/update gates pass.
