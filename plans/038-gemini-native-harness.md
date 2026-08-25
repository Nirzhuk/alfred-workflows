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
- **Implementation**: **BLOCKED (gemini_api_key_account_intake_unavailable; gemini_live_api_key_smoke_missing)**

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

- [x] Official native auth/runtime surface is selected.
- [x] Consumer subscription versus API/Vertex billing is explicit.
- [x] Native mode has no CLI dependency or credential scraping.
- [x] Streaming, tools, safety states, cancellation, and redaction have bounded
  Plan 032 fixtures.
- [x] CLI mode remains unchanged.
- [ ] Settings can securely register an Alfred-managed Gemini API key account.
- [ ] A live paid-project API-key smoke has passed without Gemini CLI installed.

## Implementation evidence (2026-08-25)

### Official surface decision

Official documentation was re-read on 2026-08-25:

- Gemini Developer API authentication requires `x-goog-api-key`. Standard keys
  identify a Cloud project for billing/quota; authorization keys bind a service
  account, are restricted to the Generative Language API by default, and become
  the required key type in September 2026. The selected native surface is the
  fixed host `generativelanguage.googleapis.com`, API `v1beta`, with an
  Alfred-managed user-supplied key.
- `models.list` is the authoritative live catalog. Alfred requests the maximum
  documented page size and filters models to `generateContent`; an empty or
  malformed catalog is `model_unavailable`, never a static CLI fallback.
- `streamGenerateContent?alt=sse` is the selected streaming endpoint. Function
  calls are model predictions: Alfred executes them behind the shared tool and
  approval boundary, echoes provider call IDs and opaque thought signatures,
  and never emits thought/reasoning content.
- `usageMetadata` is per-response token accounting, not an authoritative
  remaining-quota API. Rate limits are project-scoped and visible in AI Studio;
  native account usage therefore stays `unavailable` rather than inferred.
- Gemini API data terms distinguish unpaid and paid services. Unpaid content may
  be used for product improvement and human review; paid content is not used for
  product improvement. Paid-data handling applies in the EEA/Switzerland/UK even
  to unpaid quota, but API clients made available there must use Paid Services.
- Google OAuth's installed-app guide is explicitly a simplified testing flow,
  requires a registered desktop client configuration, and demonstrates ADC/token
  files plus a broad `cloud-platform` scope. Alfred has no registered/verified
  public-client package or least-privilege production flow, so
  `gemini_oauth_client_packaging_unresolved` remains BLOCKED.
- Standard Vertex AI requires a Cloud project, location, billing, IAM identity,
  and ADC/service-account/authorization-key choice. Vertex Express Mode is a
  separate Preview API-key onboarding surface. The shared account shape cannot
  yet distinguish or display those choices, so
  `gemini_vertex_project_binding_unresolved` remains BLOCKED.
- Gemini CLI Google-account login and consumer Gemini/Google AI subscription
  entitlement are not Gemini Developer API credentials. Gemini CLI's official
  terms prohibit third-party software from directly accessing the services
  powering the CLI by piggybacking its OAuth, so
  `gemini_consumer_subscription_prohibited` remains BLOCKED.

Primary references:

- <https://ai.google.dev/api>
- <https://ai.google.dev/api/generate-content>
- <https://ai.google.dev/api/models>
- <https://ai.google.dev/gemini-api/docs/api-key>
- <https://ai.google.dev/gemini-api/docs/oauth>
- <https://ai.google.dev/gemini-api/docs/function-calling>
- <https://ai.google.dev/gemini-api/docs/rate-limits>
- <https://ai.google.dev/gemini-api/terms>
- <https://docs.cloud.google.com/vertex-ai/generative-ai/docs/start/quickstart>
- <https://docs.cloud.google.com/docs/authentication>
- <https://github.com/google-gemini/gemini-cli/blob/main/docs/resources/tos-privacy.md>

### Code and safety gates

- `credential.rs` accepts only an Alfred-managed resolved credential, validates
  a bounded key shape without assuming the retiring `AIza` prefix, gives the
  key a redacted `Debug`, and scrubs the exact account secret from provider text.
- `transport.rs` fixes the API host/version/header, disables redirects, never
  accepts workflow URLs or headers, bounds the model catalog, and checkpoints
  cancellation while sending and reading the response stream.
- `protocol.rs` bounds SSE frames/chunks, chunk count, function arguments,
  function calls, tool rounds, and model catalog data. It maps auth/revocation,
  429, model absence, malformed streams, and safety/prompt blocks to stable
  native errors; safety blocks cannot become empty successful turns.
- `runtime.rs` implements `NativeAgentRuntime`, live model discovery, streamed
  assistant events, per-turn (not quota) usage metadata, Alfred-owned tools,
  approval denial, cooperative cancellation/deadline checks, result replay, and
  usage-unavailable state. No session/resume, OAuth, MCP, subagent, CLI, or
  consumer-subscription capability is claimed.
- `tests.rs` drives the real shared registry with a scripted transport. Fixtures
  cover auth failure/revocation, content block, function call, thought-signature
  replay, approval denial, malformed/partial/oversized stream, oversized output,
  429, timeout, cancellation, usage unavailable, exact-key redaction, secret
  tool-argument rejection, and split SSE frames.

The runtime and production HTTP transport modules are provider-private,
fixture construction is test-only, and public registration fails closed.
Native readiness is false until both exact gates pass:

1. `gemini_api_key_account_intake_unavailable` — Settings has no approved secure
   API-key-native registration flow, and this provider slice is forbidden from
   adding a generic secret UI or changing the shared account schema.
2. `gemini_live_api_key_smoke_missing` — no approved paid-project test key was
   supplied, so no live turn/model/tool/disconnect smoke can be claimed.

Vertex, desktop OAuth/ADC, and consumer OAuth remain separate BLOCKED methods;
none is silently substituted for the selected Developer API key surface.

### Verification evidence

The provider-filtered command allowed by this dispatch passed on 2026-08-25:

```bash
cargo test --locked --manifest-path src-tauri/Cargo.toml agents::native::providers::gemini
```

Result: 13 passed, 0 failed, 607 filtered out. The full suite, broad
builds/checks, formatters, desktop dev, and packaging were skipped as required.
No live credential was available, so the live smoke remains BLOCKED rather
than simulated.
