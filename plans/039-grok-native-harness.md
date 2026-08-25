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
- **Implementation**: TODO

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

- [ ] Official xAI native route is selected or the plan is explicitly blocked.
- [ ] Subscription/API billing boundary is visible.
- [ ] Native mode has no CLI/private-endpoint dependency.
- [ ] Tools, streaming, cancellation, usage, and redaction pass Plan 032.
- [ ] Grok Build CLI remains unchanged.
