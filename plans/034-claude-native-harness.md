# Plan 034: Add managed Claude products without weakening CLI support

> **Executor instructions**: Preserve the existing Claude Code CLI adapter.
> Subscription-native means an unmodified managed Claude Code binary in an
> isolated profile. It does not mean scraping CLI credentials or relabelling
> Anthropic API usage as a subscription.

## Status

- **Priority**: P1
- **Effort**: XL
- **Risk**: CRITICAL
- **Depends on**: Plans 030–032
- **Category**: native provider / managed subscription and API routes
- **Architecture revised**: 2026-08-26
- **Implementation**: BLOCKED — Phase 1 defines the routes; neither managed
  Claude Code nor Claude API is registered as an available native runtime.

## Decision history

- **2026-08-25:** Alfred recorded that a custom third-party Claude.ai login,
  subscription-rate-limit surface, Agent SDK renderer, or `claude -p` renderer
  needs Anthropic approval, and no such approval is on record. The official
  [legal and compliance](https://code.claude.com/docs/en/legal-and-compliance)
  and [Agent SDK](https://code.claude.com/docs/en/agent-sdk/overview) pages are
  the policy sources. Direct
  [Anthropic API authentication](https://platform.claude.com/docs/en/manage-claude/authentication)
  remains the separate API-billed route.
- **2026-08-26:** Research selected an exact, unmodified Claude Code binary
  with provider-owned login as the subscription candidate, subject to the
  official [authentication](https://code.claude.com/docs/en/authentication),
  [setup](https://code.claude.com/docs/en/setup), commercial-term, packaging,
  isolation, and smoke gates. This does not approve a custom renderer or let
  Alfred read or intermediate Claude credentials.

The direct API path remains closed under the exact stable codes
`claude_api_key_account_intake_unavailable` and
`claude_live_api_key_smoke_missing`. Shipping diagnostics currently emit
`claude_live_api_key_smoke_missing`; the subscription route has no enabled
registration or fallback.

## Product routes

| Product ID | Auth and custody | Billing | Runtime |
| --- | --- | --- | --- |
| `claude_code_subscription` | Login performed by an unmodified isolated Claude Code binary | Claude subscription account | `claude_code_managed` `2.1.246` |
| `claude_api` | Alfred-managed Anthropic API key | Anthropic API credential owner | Existing direct API design, still release-gated |

The subscription row stores an opaque `runtime_profile_ref` and no
`credential_ref`. Alfred never reads Claude's token store, emits session
tokens, or imports `~/.claude`. The API product stores a secret reference and
must never claim Claude Pro/Max entitlement or billing.

## Runtime decision

The approved subscription architecture is the exact, unmodified Claude Code
binary running in an Alfred-owned isolated profile. Login may use the runtime's
own PTY/browser flow. Alfred controls process lifecycle, environment isolation,
bounds, cancellation, redaction, and profile deletion without interposing on
the runtime's subscription credentials.

A custom renderer over the Claude Agent SDK or `claude -p` is a distinct route
and remains approval-gated. Do not silently substitute it for the unmodified
binary. The direct Anthropic Messages API remains the separate `claude_api`
product with usage-based billing and Alfred-executed tools.

## Tool ownership

- `claude_code_subscription` may declare
  `runtime_executed_with_host_approval` only if Alfred can observe every tool
  request, enforce workspace/permission policy before execution, deny it, and
  cancel it. Otherwise ship `no_tools` or remain blocked.
- `claude_api` declares `alfred_executed`; the direct client owns the model
  loop while Alfred owns file, shell, patch, and delegated tool execution.

## Implementation sequence

1. Freeze Claude Code `2.1.246`, artifact sources, checksums, signatures,
   redistribution terms, notices, supported platforms, rollback, and updates.
2. Launch only the packaged absolute binary with a dedicated profile and no
   ambient Claude home, PATH lookup, or credential import.
3. Drive runtime-owned login and account status without reading credentials;
   persist only safe identity/entitlement observations and a profile reference.
4. Prove bounded PTY/process output, cancellation, profile cleanup, and the
   descriptor's exact tool-owner behavior.
5. Keep the `claude_api` account, billing, secret, models, usage, and execution
   route separate end to end.
6. Pass signed-package no-user-CLI smoke before enabling registration or UI.

## Release gates

- Unmodified binary and isolated login route are legally and operationally
  shippable on every desktop platform.
- Profile references never cross command DTOs; raw profile contents never
  enter SQLite, logs, React, or workflow graphs.
- Subscription and API billing cannot be confused or automatically switched.
- Runtime-executed tools are host-approved and bounded, or explicitly absent.
- Packaged login, turn, cancellation, disconnect, profile deletion, reinstall,
  and account-switch smoke passes.
- Native failure never calls the Claude CLI adapter.

## STOP conditions

- Implementation needs credential scraping or `CLAUDE_CODE_OAUTH_TOKEN`.
- A custom renderer is shipped without the required approval.
- Subscription access is represented by an Alfred-managed secret.
- Claude API usage could be shown as subscription quota or billing.
- Tool execution can occur before Alfred's declared approval boundary.

## Done criteria

- [x] Subscription and API products, billing, custody, and runtimes are
  explicit.
- [x] The unmodified managed-binary route is distinct from custom SDK/print
  integrations.
- [x] Existing CLI support and no-fallback behavior remain mandatory.
- [ ] Packaged runtime, isolated profile lifecycle, and approval contract pass.
- [ ] Release registration remains disabled until packaged smoke passes.
