# Plan 035: Add a native Cursor harness

> **Executor instructions**: Keep Cursor Agent CLI as a first-class `cli`
> harness. Native mode must not require `cursor-agent`/`agent` on PATH and must
> not scrape Cursor installation bundles or credential files.
>
> **Official references**:
>
> - [Cursor CLI authentication](https://cursor.com/docs/cli/reference/authentication)
> - [Cursor APIs](https://cursor.com/docs/api)
> - [Cursor SDK](https://cursor.com/docs/sdk/typescript)
> - [Cursor Cloud Agents API](https://cursor.com/docs/cloud-agent/api/endpoints)

## Status

- **Priority**: P1
- **Effort**: XL
- **Risk**: HIGH
- **Depends on**: Plans 030–032
- **Category**: native provider / API integration
- **Planned at**: 2026-08-24
- **Implementation**: TODO

## Goal

Provide a native Cursor execution path in Alfred while preserving the existing
Cursor CLI path and making the authentication/billing surface honest.

## Provider reality

Cursor documents browser authentication for its CLI and API-key authentication
for automation/public APIs. Its Cloud Agents API and SDK are agent surfaces,
not a generic model-completions API. A browser login to Cursor CLI must not be
assumed to grant Alfred a supported third-party native API credential.

The native plan therefore starts with the officially documented API/SDK route.
Subscription OAuth is a separate decision and is blocked until Cursor documents
it for external clients.

## Scope

**In scope**:

- Native Cursor account/API-key capability decision.
- Secure account metadata and keychain credential integration.
- Cursor Cloud Agents or SDK runtime adapter.
- Workspace/repository input mapping.
- Bounded streaming, tools, approval, cancellation, and events.
- Model list, usage/spend metadata, and stable errors.
- Native Cursor UI distinct from Cursor CLI.

**Out of scope**:

- Reading Cursor CLI credentials or private JavaScript bundles.
- Scraping IDE `state.vscdb` as a native account source.
- Claiming browser subscription login works for Alfred without documentation.
- Recreating Cursor's proprietary local agent behavior from observed output.

## Implementation steps

### Step 1: Freeze the supported Cursor surface

Choose one official surface:

- Cursor Cloud Agents API;
- Cursor TypeScript/Python SDK through a packaged bridge;
- another documented provider runtime approved during drift check.

Record auth type, account/team scope, repository requirements, model selection,
stream/event format, tool permissions, and pricing/usage semantics.

**STOP** if native execution requires undocumented private endpoints or a
user-installed Cursor runtime.

### Step 2: Implement native account setup

Use Plan 031's secure account store. Prefer API-key entry through a deliberate
secure flow if that is the official supported method; never place the key in
workflow JSON or generic HTTP headers.

If Cursor later documents a public OAuth client flow for external apps, add it
as a separate auth method with an explicit migration/reconnect path. Do not
silently reinterpret existing API-key accounts.

### Step 3: Implement the runtime adapter

Map Alfred native requests to the selected Cursor API/SDK:

- workspace/repository identity;
- prompt and bounded context;
- model/auto selection;
- tool permissions;
- approval and cancellation;
- streamed agent activity;
- final result and changed-file metadata.

The adapter must report whether execution is local, cloud, or hybrid. A cloud
agent cannot silently receive arbitrary local paths or full Alfred workflow
history.

### Step 4: Implement model and usage behavior

Use documented model/catalog endpoints where available. Distinguish:

- included plan usage;
- API-key usage;
- team/admin spend data;
- unavailable personal usage.

Never infer quota from Cursor CLI internals. Preserve the existing CLI usage
probe only in `cli` mode.

### Step 5: Add native UI and conformance fixtures

Show:

- Cursor Agent CLI;
- Cursor native API/Cloud Agent;
- auth method and billing boundary;
- local/cloud execution label;
- repository/account requirements;
- unsupported native capabilities.

Add fake HTTP/SDK fixtures for success, 401, 403, 429, timeout, tool failure,
workspace mismatch, cancellation, oversized output, and revoked key.

## Subagent-ready ownership slices

- **Provider drift/policy**: official API/SDK contract and billing semantics.
- **Account**: API-key/OAuth method and secure storage.
- **Runtime**: Cursor API/SDK transport and event mapping.
- **Workspace safety**: repository selection and local-data boundary.
- **Frontend/tests**: native settings, model/usage UI, contract fixtures.

## STOP conditions

- Native mode can only be implemented by scraping Cursor CLI or IDE internals.
- A cloud API would receive arbitrary local data without explicit user scope.
- API-key/team billing cannot be shown clearly.
- Cursor requires a confidential secret that cannot be protected in a desktop
  public client.
- SDK/bridge packaging is not supportable on all Alfred platforms.

## Verification

```bash
bun test
bun run build:frontend
cargo test --locked --manifest-path src-tauri/Cargo.toml agents integrations runner
cargo check --locked --manifest-path src-tauri/Cargo.toml
bun run check
```

Manual smoke must verify native Cursor works with no Cursor CLI installed while
an existing CLI workflow still works in a separate harness selection.

## Done criteria

- [ ] One official native Cursor API/SDK surface is selected.
- [ ] Native authentication and billing semantics are explicit.
- [ ] Native mode has no CLI/private-bundle dependency.
- [ ] Workspace boundaries, events, tools, and cancellation pass Plan 032.
- [ ] CLI mode remains unchanged.
- [ ] Unsupported subscription OAuth is documented rather than implied.
