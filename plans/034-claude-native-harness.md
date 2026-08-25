# Plan 034: Add a native Claude harness without weakening CLI support

> **Executor instructions**: Preserve the existing Claude Code CLI path as a
> first-class harness. Do not copy Claude Code subscription credentials from
> the CLI or imply that Alfred can offer Claude.ai subscription rate limits
> without provider approval.
>
> **Official references**:
>
> - [Claude Code authentication](https://code.claude.com/docs/en/authentication)
> - [Claude Agent SDK overview](https://code.claude.com/docs/en/agent-sdk/overview)
> - [Claude Code legal and compliance](https://code.claude.com/docs/en/legal-and-compliance)
> - [Anthropic API authentication](https://platform.claude.com/docs/en/manage-claude/authentication)
> - [Messages API](https://platform.claude.com/docs/en/api/messages/create)
> - [Streaming Messages](https://platform.claude.com/docs/en/build-with-claude/streaming)
> - [Client tool loop](https://platform.claude.com/docs/en/agents-and-tools/tool-use/handle-tool-calls)
> - [Models API](https://platform.claude.com/docs/en/api/models/list)
> - [API errors](https://platform.claude.com/docs/en/api/errors)
> - [Usage and Cost Admin API](https://platform.claude.com/docs/en/manage-claude/usage-cost-api)
> - [API pricing](https://platform.claude.com/docs/en/about-claude/pricing)

## Status

- **Priority**: P1
- **Effort**: XL
- **Risk**: CRITICAL
- **Depends on**: Plans 030–032
- **Category**: native provider / policy gate
- **Planned at**: 2026-08-24
- **Implementation**: **BLOCKED**
  (`claude_api_key_account_intake_unavailable`;
  `claude_live_api_key_smoke_missing`) — subscription OAuth also lacks
  Anthropic third-party approval

## Goal

Define and, only if officially supported, implement Alfred-native Claude
execution. Keep users who prefer Claude Code's own CLI harness fully supported.

## Provider reality

Claude Code subscription login and Anthropic API authentication are different
surfaces. The official Agent SDK is available for TypeScript and Python, while
Alfred's core runtime is Rust/Tauri. The Agent SDK documentation also contains
an explicit restriction against third parties offering Claude.ai login or rate
limits without approval.

Therefore this plan has two possible native products:

1. **Approved subscription-native Claude runtime**: only after Anthropic grants
   the required permission and documents the integration contract.
2. **API-native Claude runtime**: Alfred uses an approved Anthropic API
   credential, owns the tool loop, and clearly labels usage-based billing.

The second path must not be marketed as Claude Pro/Max subscription access.

## Recorded provider decision (re-verified 2026-08-25)

### Authentication and policy

- **Claude.ai subscription OAuth: BLOCKED.** Anthropic's Agent SDK overview
  says third-party developers may not offer Claude.ai login or rate limits
  unless previously approved. Its legal and compliance page is stricter and
  says third parties may not offer Claude.ai login in their applications,
  route Free/Pro/Max credentials for users, or collect, store, or intermediate
  Claude.ai credentials or session tokens. Alfred has no written approval on
  record. There is therefore no native subscription login, token exchange,
  subscription-rate-limit claim, or OAuth fallback in this plan.
- **Direct API-key protocol: SUPPORTED, live setup BLOCKED.** Anthropic
  documents a standard Claude Console API key (`sk-ant-api...`) sent in the
  `x-api-key` header with `anthropic-version: 2023-06-01`. The key owner's
  calls are Anthropic API usage-based billing, not Claude Free/Pro/Max usage.
  Plan 031 provides secure credential storage and resolution after a provider
  grant, but its approved commands expose no secret-bearing input and it
  explicitly excludes a generic React paste-token field. Live API-key account
  registration is therefore **BLOCKED on missing approved non-React
  secret-entry seam**. No key is accepted through React state, command DTOs,
  CLI environment variables, Claude CLI state, or credential import.
- **Token custody:** when that seam exists, the key must be entered directly
  into trusted native code and stored as an Alfred-managed Plan 031 credential.
  The existing provider-local runtime consumes only the resolved in-memory
  credential and never reads `ANTHROPIC_API_KEY`,
  `CLAUDE_CODE_OAUTH_TOKEN`, `~/.claude`, keychain entries owned by Claude, or
  a user-installed `claude` binary. Disconnect may delete only Alfred's own
  credential; Anthropic documents key revocation in Console, so Alfred must not
  claim remote revocation without a separately approved API.

### Runtime and redistribution

- **Direct Rust client: selected protocol boundary.** The documented REST API
  supports `POST /v1/messages`, stateless multi-turn messages, SSE streaming,
  client `tool_use` / `tool_result` loops, and `GET /v1/models`. That is enough
  for an Alfred-owned bounded loop without Claude CLI or an Agent SDK bridge.
- **Agent SDK bridge: not selected; redistribution/platform packaging
  unresolved.** The official Agent SDK surface is Python and TypeScript and
  currently bundles a native Claude Code binary. The public documentation does
  not by itself settle Alfred desktop redistribution and per-platform packaging
  approval. This plan makes no bridge-shipping claim.
- **Branding:** the runtime is described as “Claude native · Anthropic API” and
  never as Claude Code. The existing Claude Code CLI harness remains a separate
  first-class choice.

### Model, usage, and error truthfulness

- Models come from the documented `GET /v1/models` response; an empty or
  invalid catalog and an unavailable selected model are explicit errors. No
  hard-coded fallback silently substitutes a model.
- A normal Messages API key receives per-response token counts, but Anthropic's
  organization Usage and Cost API requires a distinct Admin API key and is not
  available to individual accounts. The account usage surface therefore stays
  **usage unavailable**; it does not infer a quota or subscription allowance
  from local history.
- The provider-local mapping follows the documented HTTP/SSE errors: invalid
  authentication (401), billing (402), permission (403), request too large
  (413), rate limit (429), server/timeout failures (5xx/504), and overload
  (529). Provider response text is not exposed because it can echo secrets or
  prompt content.

## Implementation checkpoint

Provider-local, unreachable-by-default artifacts now exist under
`src-tauri/src/agents/native/providers/claude/`:

- documented fixed-host Messages and Models HTTP transport with redirects
  disabled, separate connect/read/total deadlines, and bounded response bodies;
- bounded SSE decoder and assistant-text output;
- bounded tool input/output and eight-iteration Alfred-owned tool loop;
- Plan 032 approval, permission, cancellation-aware request/chunk reads,
  deadlines, event normalization, and redaction integration;
- explicit model, usage-unavailable, stable error, and no-reasoning surfaces;
- registry-path fixtures for text streaming, tool allow/deny, cancellation,
  timeout, invalid auth, 429, overload, context limit, oversized output, and
  redaction, plus loopback production-policy fixtures for redirect refusal,
  bounded model bodies, and a 512-entry catalog with bounded IDs/labels.

The production HTTP constructor is provider-private and registration fails
closed with both exact gate codes, so fixture construction cannot become a
production bypass. Shipping a runtime that no user can securely connect would
be a misleading partial UI.
Native-ready remains **BLOCKED** until the live API-key account-intake gate is
approved and tested. Subscription-native remains independently **BLOCKED**
until Anthropic grants explicit third-party approval. These blocked states do
not weaken or replace the Claude Code CLI harness.

| Native-ready gate | Status | Evidence / remaining condition |
| --- | --- | --- |
| Subscription auth policy | **BLOCKED** | No Anthropic third-party approval is on record. |
| Direct API protocol | PASS (fixture) | Official Messages, streaming, client-tool, and Models surfaces cover the selected boundary. |
| Live API-key account registration | **BLOCKED** | `claude_api_key_account_intake_unavailable`: missing approved non-React secret-entry seam. |
| Runtime and tool loop | PASS (fixture) | Bounded provider-local registry fixtures pass. |
| Cancellation / timeout | PASS (fixture) | Mid-stream cancellation and deadline fixtures pass. |
| Redaction / no reasoning | PASS (fixture) | Output/error redaction and thinking suppression fixtures pass. |
| Live API smoke | **BLOCKED** | `claude_live_api_key_smoke_missing`: no approved credential was supplied for model/turn/tool/disconnect validation. |
| Desktop/package release | NOT CLAIMED | Public registration fails closed; no bridge redistribution claim or packaging gate was run. |
| Native UI | BLOCKED DISCLOSURE ONLY | Connect/Reconnect stay unavailable; no secret-entry control is rendered. |

## Scope

**In scope**:

- Provider/API capability and policy decision.
- Native account method registration.
- Direct streaming model client or approved Agent SDK bridge.
- Alfred-owned tools, permissions, skills, context, and event normalization.
- Model list, usage metadata, retry, cancellation, and redaction.
- Native Claude UI states.

**Out of scope**:

- Reading `~/.claude/.credentials.json` or macOS Claude keychain entries.
- Reusing `CLAUDE_CODE_OAUTH_TOKEN` without provider approval.
- Claiming Claude subscription quotas for API-key calls.
- Running the Claude CLI in Alfred mode.
- Reimplementing every Claude Code feature in the first native release.

## Implementation steps

### Step 1: Obtain the provider decision

Record one of:

- approved subscription-native third-party integration;
- API-key-native implementation;
- blocked pending provider approval.

Document allowed client type, redirect rules, token custody, rate-limit policy,
SDK redistribution, and branding constraints.

**STOP** if the only way to use a Claude subscription is to impersonate Claude
Code, scrape its credentials, or rely on an undocumented endpoint.

### Step 2: Choose the runtime boundary

Evaluate:

- direct Anthropic API client in Rust;
- bundled TypeScript Agent SDK bridge;
- approved provider runtime process.

Prefer a direct Rust client when the API contract supports all required tool,
stream, cancellation, and usage behavior. Use a bridge only when its lifecycle,
packaging, licensing, and credential handling are explicit.

**Verify**: the native runtime has no dependency on a user-installed `claude`
binary and can expose bounded events to Plan 032.

### Step 3: Implement account/auth lifecycle

For API mode, add the approved API-key setup using secure Alfred account
storage, with clear usage-based billing copy. If OAuth is approved, use the
provider's registered public-client flow and Plan 031's account lifecycle.

Never import CLI credentials. Native logout must revoke/delete only the Alfred
account credential it owns.

### Step 4: Implement the native turn loop

Support the smallest useful native feature set:

- text prompt;
- model selection;
- bounded streaming;
- Alfred file/shell tools behind permission profiles;
- approval requests;
- cancellation and timeout;
- skill loading by Alfred;
- bounded context and final output;
- normalized activity events.

Do not expose raw reasoning content as ordinary activity.

### Step 5: Implement model/usage/error surfaces

Use provider-documented model and usage endpoints. If Claude does not expose a
reliable subscription usage API for the chosen auth method, show “usage
unavailable” rather than infer it from local history.

Classify overloaded, rate-limited, invalid-auth, context-limit, and provider
unavailable responses into stable Alfred errors.

### Step 6: Add UI and compatibility tests

Show separate entries for:

- Claude Code CLI;
- Claude native API or approved native subscription mode.

The UI must display the billing/auth method and native capability gaps.

## Subagent-ready ownership slices

- **Policy/research**: provider approval and official auth/runtime contract.
- **Runtime**: direct client or SDK bridge, streaming, tools, cancellation.
- **Account**: native credential and refresh/revoke integration.
- **UI**: auth method, account state, billing copy, model picker.
- **Conformance**: tool permissions, redaction, context, and error fixtures.

No implementation slice may begin before the policy slice records the selected
native auth mode.

## STOP conditions

- Third-party Claude.ai subscription login/rate limits are not approved.
- The implementation needs CLI credential scraping.
- The only direct protocol is undocumented.
- API-key usage could be mistaken for subscription usage.
- SDK/bridge redistribution or license terms are unresolved.
- Native tool permissions cannot match Alfred's safety contract.

## Verification

```bash
bun test
bun run build:frontend
cargo test --locked --manifest-path src-tauri/Cargo.toml agents runner
cargo check --locked --manifest-path src-tauri/Cargo.toml
bun run check
```

Focused executor verification on 2026-08-25:

```text
cargo test --locked --manifest-path src-tauri/Cargo.toml agents::native::providers::claude --no-fail-fast
34 passed; 0 failed; 587 filtered out
```

Per the coordinated execution boundary, formatters, linters, broad builds,
unfiltered tests, desktop development, and packaging were not run here. The
coordinator owns the shared validation pass after provider siblings settle.

Manual tests must use a disposable provider account/credential and verify that
no CLI installation is required for native mode, while the existing CLI mode
still works independently.

## Done criteria

- [ ] Native auth mode is provider-approved and documented.
- [x] Subscription and API-key billing boundaries are explicit.
- [x] Provider-local native execution does not invoke or inspect Claude CLI
      state.
- [x] Tools, approvals, cancellation, and events pass focused Plan 032
      registry fixtures.
- [ ] Native Claude remains visibly separate from Claude Code CLI.
- [x] If blocked, the plan records the exact provider decision and does not ship
      a misleading partial implementation.
