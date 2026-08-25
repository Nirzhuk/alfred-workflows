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
- **Implementation**: BLOCKED (safe Cloud Agents v1 protocol artifacts and fixtures added; runtime intentionally not registered)

### Drift check and frozen surface (2026-08-25)

The selected official surface is the **Cursor Cloud Agents API v1 public
beta over HTTPS**. This route does not require Cursor Agent CLI, the Cursor IDE,
Node, an SDK bridge, or a user-installed Cursor runtime.

- **Authentication**: The API accepts Basic or Bearer authentication with a
  Cursor user API key or service-account API key. `GET /v1/me` identifies the
  key owner. Team Admin API keys are a separate API and are not accepted by the
  SDK. Cursor CLI browser login remains CLI-only and is not an Alfred native
  credential.
- **Billing owner**: The user or service account that owns the API key owns the
  Cloud Agent activity and billing. Cursor documents Cloud Agents as charged at
  API pricing for the selected model. Team/admin spend data belongs to separate
  enterprise/admin APIs; personal subscription quota is not available from the
  Cloud Agents API.
- **Local/cloud behavior**: `POST /v1/agents` creates a Cursor-managed cloud
  agent. Alfred must send only an explicitly confirmed remote repository URL,
  starting ref, and bounded prompt. It must never translate a local path into an
  upload or discover a Git remote implicitly.
- **Repository requirements**: A source-control integration must grant Cursor
  access to the repository. The frozen adapter accepts an explicit HTTPS GitHub
  repository URL and starting ref only. The repository listing endpoint has
  strict rate limits and is not a safe implicit workspace-discovery mechanism.
- **Models and usage**: `GET /v1/models` returns supported model IDs and
  variants. `GET /v1/agents/{agentId}/usage` returns per-run and total input,
  output, cache-read, and cache-write token counts. It does not return a quota
  reset or personal plan balance.
- **Events and cancellation**: `GET
  /v1/agents/{agentId}/runs/{runId}/stream` is documented SSE with `status`,
  `assistant`, `thinking`, `tool_call`, `result`, `error`, `heartbeat`, and
  `done` events. Alfred drops thinking content. `POST
  /v1/agents/{agentId}/runs/{runId}/cancel` is terminal cancellation.
- **Packaging**: Direct HTTP avoids the Cursor SDK packaging gate. The
  alternative TypeScript SDK currently requires Node 22.13+ and ships
  per-platform packages; the official SDK bridge publishes standalone platform
  archives, but neither is needed for this cloud-only choice.

### Exact BLOCKED gates

Plan 035 is not native-ready and no Cursor runtime or account handler is
registered.

1. The shared account contract has no API-key auth method or approved non-React
   secret-entry seam and currently advertises Cursor as
   `runtime`/`runtime_managed`. Storing a Cursor user or service-account API key
   under that identity would misstate credential custody and billing. Passing a
   key through a React/Tauri DTO is prohibited. Plan 035 cannot change the
   shared account schema.
2. The shared native request carries a local working directory but no explicit
   remote repository URL/ref consent field. Deriving the Git remote, sending
   arbitrary local context, or uploading the workspace would violate the local
   data boundary. Plan 035 cannot change the shared request contract.
3. Cloud Agents v1 emits server-side tool activity but documents no per-tool
   approval callback that can implement Alfred's `Ask` policy. Shipping it with
   approval capability would be false support.

Safe artifacts live under
`src-tauri/src/agents/native/providers/cursor/`: the frozen policy, explicit
repository binding and bounded payload builder, stable HTTP/transport error
mapping, model/usage decoders, cancellation endpoint builder, SSE mapping,
reasoning suppression, and fake fixtures. These artifacts compile as provider
code but cannot become reachable execution until all three gates pass.

### Verification evidence (2026-08-25)

The provider-only command below compiled the shared native contracts and ran
only Cursor fixtures. Result: **8 passed, 0 failed, 576 filtered out**.

```bash
cargo test --locked --manifest-path src-tauri/Cargo.toml agents::native::providers::cursor --lib
```

The fixtures exercise bounded success, 401, 403, 429, timeout, terminal
cancellation, workspace/repository mismatch, cloud tool failure, oversized
provider and shared event output, revoked key, `crsr_` key redaction, model
discovery, token usage, and the blocked readiness decision. `git diff --check`
passes for the Cursor provider and this plan. The module contains no
`NativeAgentRuntime` implementation or registration function, so the blocked
artifacts cannot advertise a runnable provider.

Native settings now disclose the selected boundary without mutating the frozen
account schema: a user/team Cursor API key owns Cursor Cloud billing, execution
is remote against an explicitly confirmed repository/ref, local Cursor CLI
credentials are not reused, and Connect/Reconnect remain disabled while the
three gates above are blocked.

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

- [x] One official native Cursor API/SDK surface is selected.
- [x] Native authentication and billing semantics are explicit.
- [x] Native mode has no CLI/private-bundle dependency.
- [ ] Workspace boundaries, events, tools, and cancellation pass Plan 032.
- [ ] CLI mode remains unchanged.
- [x] Unsupported subscription OAuth is documented rather than implied.
