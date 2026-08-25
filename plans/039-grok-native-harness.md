# Plan 039: Add a native Grok harness

> **Executor instructions**: Preserve Grok Build CLI as a first-class CLI
> harness. Native mode must use a documented xAI API/SDK/auth surface. Do not
> assume a consumer Grok subscription can be reused by a third-party desktop
> harness.
>
> **Official reference to re-check**:
> [xAI API documentation](https://docs.x.ai/)

## Status

- **Priority**: P2
- **Effort**: L–XL
- **Risk**: HIGH
- **Depends on**: Plans 030–032
- **Category**: native provider / API policy gate
- **Planned at**: 2026-08-24
- **Implementation**: BLOCKED. Provider mapping and fixtures exist, but live
  API-key account setup has no approved secret-entry seam in the frozen Plan
  031 contract

## Official evidence re-check (2026-08-25)

Decision: use the documented xAI inference API-key route when the account gate
is resolved. Do not use Grok Build OAuth/device credentials, Grok consumer
credentials, a management key, or private web/CLI endpoints.

- **Authentication**: the [Inference REST API](https://docs.x.ai/developers/rest-api-reference/inference)
  requires `Authorization: Bearer <xAI API key>` for `https://api.x.ai`.
  [xAI's API-key reference](https://docs.x.ai/developers/rest-api-reference/management/auth)
  says keys are team-bound, associated with their creator, returned in full
  only when created, and limited by endpoint/model ACLs. The reviewed developer
  API documentation exposes no provider-approved OAuth public-client flow for
  third-party desktop inference.
- **CLI boundary**: [Grok Build](https://docs.x.ai/build/overview) documents its
  own browser login and a separate direct `XAI_API_KEY` option. Its
  [enterprise guide](https://docs.x.ai/build/enterprise) identifies the CLI
  login proxy and `auth.x.ai` separately from the direct `api.x.ai` API-key
  path. Those CLI OAuth/device sessions are not authorization for Alfred to
  reuse or impersonate.
- **Billing**: the [API billing guide](https://docs.x.ai/console/billing) states
  that API consumption is charged to the selected xAI team through prepaid
  credits or monthly invoiced billing. The
  [billing FAQ](https://docs.x.ai/developers/faq/billing) says request cost is
  deducted or invoiced as API usage occurs. A consumer Grok subscription and a
  Grok Build login do not pay Alfred's inference bill.
- **Models**: the documented
  [`GET /v1/language-models`](https://docs.x.ai/developers/rest-api-reference/inference/models)
  returns the language models available to the authenticating API key. The
  provider mapping uses that account-specific catalog and does not hard-code a
  model promise.
- **Streaming and tools**: xAI documents SSE streaming for text-output models
  in [Streaming](https://docs.x.ai/developers/model-capabilities/text/streaming)
  and client-side custom functions in
  [Function Calling](https://docs.x.ai/developers/tools/function-calling).
  Function calls pause for client execution, streaming returns a complete call,
  and parallel calls can be disabled. The bounded fixture transport therefore
  uses `store: false`, disables parallel calls, caps output at 16,384 tokens,
  caps provider frames and tool rounds, and maps only named Alfred tools through
  the shared approval host.
- **Rate limits and provider errors**: xAI applies per-model RPS and TPM limits
  by team tier and returns HTTP 429 when a limit is exceeded, per
  [Rate Limits](https://docs.x.ai/developers/rate-limits). The provider fixture
  maps 429 to retryable provider unavailability, while auth/revocation, safety,
  timeout, cancellation, malformed stream, and oversized stream states remain
  distinct and redacted.
- **Usage availability**: inference responses expose per-request usage and
  charged cost through [Cost Tracking](https://docs.x.ai/developers/cost-tracking),
  but historical team usage uses a separate Management API key at the
  [billing usage endpoint](https://docs.x.ai/developers/rest-api-reference/management/billing).
  An inference API key cannot supply the account-wide token window represented
  by Plan 032, so native Grok reports usage as unavailable rather than asking
  for a broader management credential.
- **Data policy**: xAI's [API security FAQ](https://docs.x.ai/developers/faq/security)
  says API inputs and outputs are not used for training without explicit
  permission, default request/response retention is 30 days for abuse auditing,
  and team-wide Zero Data Retention disables stateful features. The first
  release is therefore ephemeral and sets `store: false`; it does not advertise
  sessions or resume.
- **Rotation and revocation**: xAI's
  [security guidance](https://docs.x.ai/console/faq/security) instructs users to
  disable or delete the old key, create a replacement, and update applications.
  Alfred must treat a 401/disabled key as disconnected and require secure
  replacement. It cannot claim remote revocation merely because local metadata
  was removed.

## BLOCKED gate

The official native route is viable, but it is not reachable in the current
product. Plan 031 deliberately freezes `AgentAuthMethod` without an API-key
method and exposes no approved non-React secret-entry command. Passing an xAI
key through a workflow header, React state, or an ordinary Tauri DTO would
violate the credential boundary. Shipping the runtime now would also leave the
existing gated Grok registration mislabeled as OAuth.

Unblock only after the account owner adds and reviews a provider-neutral secure
API-key entry path that writes directly to `com.alfred.agent-harness`, returns
redacted metadata only, supports key replacement and local cleanup, and never
imports Grok Build state. Until then the Grok runtime is not registered in
production. The safe artifacts are the xAI protocol/transport mapping,
provider-specific redaction and conformance fixtures, and gated UI copy that
states the billing/auth boundary without accepting a secret.

## Implementation evidence

- `src-tauri/src/agents/native/providers/grok/mod.rs` maps the documented
  Responses SSE/function-call protocol onto Plan 032 events, tools, approvals,
  cancellation, error codes, model discovery, and honest usage-unavailable
  state. It accepts production secrets only through the shared resolved native
  credential type; fixtures use a test-only `NativeCredential`.
- Provider fixtures cover invalid and revoked credentials, 429, provider and
  safety failures, timeout, cancellation, malformed and oversized streams,
  documented function calls, approval denial, model discovery,
  usage-unavailable state, and xAI-key/error redaction.
- `src/features/agent-accounts/grok-native-disclosure.tsx` makes API billing and
  the Grok subscription/Grok Build separation visible while setup is gated. It
  contains no key input or credential DTO field.
- Focused UI verification on 2026-08-25: `bun test
  src/features/agent-accounts/grok-native-disclosure.test.tsx
  src/features/agent-accounts/native-agent-settings.test.tsx` passed 3 tests.
- Focused Rust verification on 2026-08-25: `cargo test --locked
  --manifest-path src-tauri/Cargo.toml agents::native::providers::grok` passed
  9 tests with 0 failures. The first attempt was blocked by concurrent
  provider files that were still incomplete; the same focused command passed
  after their owners finished. No broad formatter, linter, build, or test
  command was run.

## Goal

Provide a native Alfred Grok runtime when xAI exposes a supported direct agent
contract, while retaining the current Grok Build CLI path.

## Provider reality

The repository currently treats Grok as a CLI provider. xAI API access and
consumer Grok access may have different authentication, billing, models, rate
limits, and tool capabilities. Native mode must not promise subscription reuse
until xAI documents it for external clients.

The likely first native route is an xAI API credential with usage-based billing,
not consumer OAuth. That distinction must be visible in the product.

## Scope

**In scope**:

- Official xAI native API/SDK feasibility and policy decision.
- Secure API-key/OAuth account integration if supported.
- Streaming responses and tool/function calls.
- Alfred-owned filesystem/shell tools and permissions.
- Model list, usage/rate-limit state, errors, cancellation, and redaction.
- Native Grok UI distinct from Grok Build CLI.

**Out of scope**:

- Grok Build CLI credential scraping.
- Consumer Grok subscription impersonation.
- Undocumented endpoints inferred from CLI traffic.
- Full parity with proprietary Grok Build behavior before capability evidence.

## Implementation steps

### Step 1: Complete provider drift and policy review

Record the current official xAI auth methods, API endpoints, supported model
families, streaming/tool schema, rate limits, data policy, and whether desktop
public clients may use OAuth.

Choose API key, OAuth, or blocked. Record billing ownership and credential
rotation behavior.

**STOP** if only CLI/consumer web credentials exist for agent execution or if
native use requires an undocumented endpoint.

### Step 2: Add native account integration

Register the selected auth method with Plan 031. API keys must be stored in the
agent credential store and never passed through generic workflow HTTP headers.
OAuth, if supported, must use a provider-approved public-client flow.

### Step 3: Implement the native runtime

Support a bounded first release:

- prompt/context;
- selected model;
- streamed text;
- documented tool/function calls;
- Alfred permission profiles;
- approval and cancellation;
- output/context limits;
- normalized activity events.

Map xAI safety, auth, quota, and provider failures to stable Alfred errors.

### Step 4: Implement models and usage

Use authoritative xAI APIs for model discovery and usage. If account usage is
not exposed, display unavailable. Never derive subscription quota from Grok CLI
output or local history.

### Step 5: Add UI and fixtures

Show Grok Build CLI and Grok native independently. Display the auth method,
billing owner, model availability, usage limitations, and native capability
gaps.

Fixtures cover auth failure, 429, timeout, malformed stream, tool call,
permission denial, cancellation, oversized output, and revoked key.

## Subagent-ready ownership slices

- **Provider research**: official xAI auth/API/policy surface.
- **Account**: secure credentials and refresh/revoke.
- **Runtime**: streaming/tools/cancellation/error mapping.
- **Usage/model**: authoritative catalog/quota state.
- **UI/tests**: native settings and conformance fixtures.

## STOP conditions

- Native mode requires Grok Build CLI or private web credential scraping.
- Consumer subscription OAuth is not documented for external clients.
- Tool/function behavior is undocumented or cannot be permission-bounded.
- Usage/billing ownership cannot be stated accurately.
- API-key or OAuth redistribution violates xAI policy.

## Verification

```bash
bun test
bun run build:frontend
cargo test --locked --manifest-path src-tauri/Cargo.toml agents runner
cargo check --locked --manifest-path src-tauri/Cargo.toml
bun run check
```

Manual smoke must run native mode without Grok Build CLI installed using a
non-production test credential, then run the existing CLI mode separately.

## Done criteria

- [x] Official xAI native route is selected or the plan is explicitly blocked.
- [x] Subscription/API billing boundary is visible.
- [x] Native mode has no CLI/private-endpoint dependency.
- [x] Tools, streaming, cancellation, usage, and redaction pass Plan 032.
- [x] Grok Build CLI remains unchanged.
