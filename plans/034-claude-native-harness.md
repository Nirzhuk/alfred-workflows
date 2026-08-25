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
> - [Anthropic API authentication](https://platform.claude.com/docs/en/api/authentication/overview)

## Status

- **Priority**: P1
- **Effort**: XL
- **Risk**: CRITICAL
- **Depends on**: Plans 030–032
- **Category**: native provider / policy gate
- **Planned at**: 2026-08-24
- **Implementation**: TODO

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

Manual tests must use a disposable provider account/credential and verify that
no CLI installation is required for native mode, while the existing CLI mode
still works independently.

## Done criteria

- [ ] Native auth mode is provider-approved and documented.
- [ ] Subscription and API-key billing boundaries are explicit.
- [ ] Native execution does not invoke or inspect Claude CLI state.
- [ ] Tools, approvals, cancellation, and events pass Plan 032.
- [ ] Native Claude remains visibly separate from Claude Code CLI.
- [ ] If blocked, the plan records the exact provider decision and does not ship
      a misleading partial implementation.
