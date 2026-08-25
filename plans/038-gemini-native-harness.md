# Plan 038: Add a native Gemini harness

> **Executor instructions**: Preserve the Gemini CLI harness. Native mode must
> use an official Google/Gemini API or SDK surface with explicit account and
> billing semantics. Do not assume Gemini CLI login or a consumer Gemini plan
> grants a supported third-party API credential.
>
> **Official references to re-check**:
>
> - [Gemini API documentation](https://ai.google.dev/gemini-api/docs)
> - [Gemini API authentication](https://ai.google.dev/gemini-api/docs/api-key)
> - [Gemini CLI repository](https://github.com/google-gemini/gemini-cli)
> - [Google Cloud Vertex AI authentication](https://cloud.google.com/vertex-ai/docs/authentication)

## Status

- **Priority**: P1
- **Effort**: L–XL
- **Risk**: HIGH
- **Depends on**: Plans 030–032
- **Category**: native provider / API authentication
- **Planned at**: 2026-08-24
- **Implementation**: TODO

## Goal

Provide a native Gemini execution path that does not require the user to
install Gemini CLI, while preserving the existing CLI harness and making the
API-key/Google-account billing boundary explicit.

## Provider reality

Gemini surfaces may include consumer Gemini plans, Gemini API keys, Google
Cloud/Vertex AI credentials, and Gemini CLI authentication. These are not
interchangeable by assumption.

The native plan must select one documented surface. Consumer subscription OAuth
is not considered supported until Google documents it for external desktop
clients and the required model/tool quotas.

## Scope

**In scope**:

- Official Gemini API or Vertex AI native runtime decision.
- Secure API-key or Google OAuth/ADC account storage.
- Streaming/function-calling/tool loop.
- Alfred workspace and permission mapping.
- Model catalog, usage, rate-limit, and billing states.
- Native Gemini UI distinct from Gemini CLI.

**Out of scope**:

- Importing Gemini CLI credentials/configuration.
- Scraping local CLI output for native usage.
- Presenting API-key or Vertex usage as consumer subscription usage.
- Broad Google Workspace OAuth; that belongs to Connected Apps plans.
- Supporting every Gemini multimodal capability initially.

## Implementation steps

### Step 1: Select an official auth/runtime surface

Evaluate:

- Gemini API with API key;
- Google OAuth/ADC for Vertex AI;
- another officially documented public-client route.

Record model availability, billing owner, quotas, data handling, regional
constraints, function-calling behavior, and desktop credential requirements.

**STOP** if native mode requires a confidential client secret or consumer
subscription token reuse that Google does not document for external clients.

### Step 2: Add account/auth integration

Use Plan 031's account store. API keys require an explicit secure entry flow;
Google OAuth/ADC must use a registered desktop/public-client configuration and
least privilege.

Never import or read Gemini CLI credential files. Account identity and billing
method must be displayed in native settings.

### Step 3: Implement native Gemini runtime

Support the first bounded capability set:

- text and streamed output;
- selected model;
- function/tool calls;
- Alfred-owned file/shell tools behind permission profiles;
- approval and cancellation;
- context and output limits;
- normalized events and stable errors.

Map provider safety/block responses to explicit Alfred states; never turn a
blocked response into an empty successful turn.

### Step 4: Implement models and usage

Use documented model listing and usage/quota APIs. If the chosen auth surface
has no authoritative usage endpoint, show unavailable rather than estimate from
turn history.

Keep CLI model/usage discovery isolated to CLI mode.

### Step 5: Add UI and provider fixtures

Show Gemini CLI and Gemini native separately. Native UI must label:

- API key versus Google account/Vertex auth;
- billing owner;
- region/project where relevant;
- model availability;
- quota unavailable/limited states.

Fixtures cover auth failure, blocked content, function call, malformed stream,
429, timeout, cancellation, oversized output, and revoked credential.

## Subagent-ready ownership slices

- **Auth research**: Gemini API/Vertex/consumer account boundaries.
- **Account**: secure credential and project/region metadata.
- **Runtime**: streaming/function calls/tools/cancellation.
- **Usage/model**: catalog, quota, billing display.
- **Frontend/tests**: native settings and contract fixtures.

## STOP conditions

- Native implementation requires Gemini CLI or CLI credential scraping.
- Consumer subscription OAuth is assumed without an official external-client
  contract.
- Billing/account ownership cannot be displayed accurately.
- Google OAuth would require broad unrelated Workspace scopes.
- Provider safety responses cannot be represented as stable Alfred states.

## Verification

```bash
bun test
bun run build:frontend
cargo test --locked --manifest-path src-tauri/Cargo.toml agents runner
cargo check --locked --manifest-path src-tauri/Cargo.toml
bun run check
```

Manual smoke must run native Gemini with no Gemini CLI installed, use a test
credential/project, perform one tool-using turn, inspect quota/account labels,
and disconnect. Run a separate CLI regression smoke.

## Done criteria

- [ ] Official native auth/runtime surface is selected.
- [ ] Consumer subscription versus API/Vertex billing is explicit.
- [ ] Native mode has no CLI dependency or credential scraping.
- [ ] Streaming, tools, safety states, cancellation, and redaction pass Plan 032.
- [ ] CLI mode remains unchanged.
