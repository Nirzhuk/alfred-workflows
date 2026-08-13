# Plan 017: Expose selected Agentflow workflows to Microsoft Copilot

> **Executor instructions**: Copilot is a remote control surface, not a local
> coding-agent provider. Do not add it to `AgentProviderId` or invoke a Copilot
> CLI from runner nodes. This plan is blocked until Plan 011's remote API,
> identity, device pairing, and queue are production-ready.
>
> **Drift check (run first)**: verify Plan 011's approved ADR and remote
> `list/start/status/cancel` contracts, plus explicit workflow publication.
> Re-read current Microsoft 365 Copilot and Copilot Studio extensibility,
> authentication, custom connector, publishing, MCP, and A2A documentation.

## Status

- **Priority**: P1 after core connected apps
- **Effort**: XL
- **Risk**: CRITICAL
- **Depends on**: Plans 008 and 011; Plan 009 only if workflows call connected apps
- **Category**: integration / distribution surface
- **Planned at**: 2026-08-11

## Product outcome

A user can ask an Agentflow Copilot Studio agent something like “Run my release
readiness workflow,” confirm the action, and receive a request ID plus status.
The paired desktop performs the workflow locally. Copilot never connects to
`127.0.0.1`, reads the workflow graph, or receives provider/CLI credentials.

Recommended v1: **Copilot Studio agent + OpenAPI custom connector/tool** backed
by Plan 011. MCP and agent-to-agent protocols are later adapters over the same
authorization and invocation service, not the first implementation.

Official references: [Microsoft 365 Copilot extensibility overview](https://learn.microsoft.com/en-us/microsoft-365/copilot/extensibility/overview),
[add tools/custom REST APIs in Copilot Studio](https://learn.microsoft.com/en-us/microsoft-copilot-studio/library-add-actions),
[MCP in Copilot Studio](https://learn.microsoft.com/en-us/microsoft-copilot-studio/mcp-add-components-to-agent),
[Entra on-behalf-of custom connectors](https://learn.microsoft.com/en-us/microsoft-copilot-studio/advanced-custom-connector-on-behalf-of),
and [publish to Teams/Microsoft 365 Copilot](https://learn.microsoft.com/en-us/microsoft-copilot-studio/publication-add-bot-to-microsoft-teams).

## Commands you will need

- Plan 011's approved relay lint/test/contract commands.
- `bun test`
- `bun run build:frontend`
- `cargo test --manifest-path src-tauri/Cargo.toml`
- OpenAPI validation and Copilot Studio evaluation commands documented by the
  selected environment before implementation begins.

## Scope

**In scope**:

- Entra-protected Agentflow remote API and tenant/user mapping.
- Explicit workflow publication and safe input schemas.
- OpenAPI definition/custom connector.
- Copilot Studio agent/tool instructions, confirmation rules, async status UX,
  Teams/Microsoft 365 publication, admin/consent docs, and evaluation suite.
- Optional design note mapping the API to future MCP/A2A.

**Out of scope**:

- Adding “Microsoft Copilot” as a workflow execution provider.
- Sending workflow graphs, arbitrary prompts, local paths, code, logs, or agent
  output to Copilot by default.
- Running workflows in the cloud, unattended destructive operations, or tenant-
  wide admin/application permissions.
- Direct Copilot-to-desktop localhost/tunnel connections.

## Git workflow

Keep remote API hardening, desktop publication UX, connector definition,
Copilot agent configuration, and tenant rollout as separate reviewable changes.
Do not deploy, commit, push, or publish to a tenant without the approvals in
this plan. Preserve a fully functional local-only app path.

## Implementation steps

### Step 1: Define the Copilot-specific security model

Extend Plan 011's ADR with:

- supported Microsoft tenants (single-tenant pilot vs multitenant product);
- Entra app registrations per environment and verified publisher plan;
- delegated scope such as `Workflow.Invoke` and admin-consent expectations;
- exact mapping from Entra `tid` + stable user subject to Agentflow relay user;
- device choice when a user has multiple paired desktops;
- destructive workflow classification and confirmation policy;
- output disclosure policy (recommend status/summary only in v1);
- audit records visible to the user and tenant administrator.

Do not rely on email address as identity. Reject guest/cross-tenant ambiguity
until explicitly designed.

**Verify**: cross-tenant threat tests and named Microsoft tenant administrator/
security approval for the pilot.

### Step 2: Finalize the remote workflow contract

Expose only explicitly published workflows:

- `GET /v1/workflows` — ID, name, safe description, input schema, risk label,
  availability; no graph/node/provider details.
- `POST /v1/runs` — workflow ID, validated inputs, target device optional,
  idempotency key; returns `202` and request ID.
- `GET /v1/runs/{requestId}` — queued/awaiting-confirmation/running/succeeded/
  failed/cancelled/expired and sanitized summary.
- `POST /v1/runs/{requestId}/cancel` — best-effort cancel.

Use coarse, stable schemas that Copilot can call reliably. Inputs support simple
typed fields and enums only in v1. Reject extra fields/oversize text. Never let
the caller submit a workflow graph, shell command, agent provider override, or
arbitrary node input.

**Verify**: OpenAPI contract tests cover unpublished/not-owned IDs, invalid
input, duplicate idempotency, offline/expired queue, cancel races, and no output
or error content outside the published schema.

### Step 3: Add desktop publication and confirmation UX

Add workflow settings for:

- “Available to Microsoft Copilot/remote assistants” off by default;
- user-written safe description;
- allow-listed input schema/defaults;
- risk class `read-only|writes external data|local side effects`;
- target-device behavior;
- always-confirm toggle, mandatory for side-effecting workflows in v1.

On remote request, show the calling Microsoft identity/tenant, workflow,
inputs, expiry, and side-effect warning. Confirmation happens on the paired
desktop for risky workflows; timeout declines. Record local audit metadata but
not Entra tokens. Unpublishing immediately blocks new requests and invalidates
queued requests not yet accepted.

**Verify**: frontend/store tests for publish/unpublish, required descriptions,
risk defaults, confirmation timeout/reject, and queue invalidation.

### Step 4: Register the Entra API and custom connector

Register the relay API under the chosen tenant model. Validate issuer, audience,
tenant, user, scopes/roles, timestamps, and signing keys server-side. Follow
current Copilot Studio connector authentication guidance; use on-behalf-of only
if the connector architecture actually requires downstream delegated calls.
The Agentflow API itself should accept a token intended for its own audience.

Generate/import the reviewed OpenAPI definition into a custom connector. Give
operations clear names/descriptions and no hidden parameters. Configure per-
user authentication; never use one shared API key for all Copilot users.

**Verify**: wrong audience/tenant/scope/user and expired/replayed token tests;
connector test console can list only the signed-in user's published workflows.

### Step 5: Build the Copilot Studio agent/tool behavior

Create a versioned agent definition/instructions that:

- lists workflows when the user has not named one unambiguously;
- summarizes workflow/risk and asks confirmation before start;
- sends only schema-valid inputs;
- reports request ID and queued/offline status honestly;
- polls status with a bounded interval/count rather than holding one request;
- never claims success until API status is `succeeded`;
- gives reconnect/open-Agentflow guidance for offline/expired cases;
- does not invent workflow names, inputs, results, or remediation.

Use deterministic tool descriptions and examples with synthetic data. Avoid
putting secrets or production workflow names in the exported agent package.

**Verify**: evaluation prompts cover ambiguous workflow, missing input,
destructive confirmation, duplicate user request, offline desktop, long run,
failure/cancel, prompt injection in workflow description/input, and tenant
isolation. Track tool-call correctness, not just conversational quality.

### Step 6: Pilot and publish to Teams/Microsoft 365

Start in a dedicated test tenant and small internal group. Complete admin
consent/DLP review, connector environment configuration, privacy/support links,
and tenant allow-list. Publish to Teams/Microsoft 365 Copilot using Microsoft's
current channel process only after evaluation/security gates pass.

Collect privacy-safe operational metrics: auth failures, tool validation
errors, queue latency, confirmation rate, completion status, and duplicate
suppression. Do not capture prompts, workflow inputs, names, or output by default.

**Verify**: two users/two devices/two tenants, revoke consent, unlink device,
unpublish workflow, offline expiry, and incident rollback exercises.

### Step 7: Decide later protocol adapters

After v1 usage, document whether MCP adds value for tool discovery or whether
Copilot Studio's agent-to-agent protocol fits a richer Agentflow agent. Any
adapter must reuse the same Entra identity, publication allow-list, input
validation, idempotency, confirmation, queue, and audit boundaries. Do not
expose the internal action registry wholesale.

## Test plan

- Plan 011 relay/security tests and desktop build/test gates.
- OpenAPI schema/lint/contract suite.
- Entra JWT negative tests using test keys/tenant only.
- Copilot Studio automated conversation/tool evaluations.
- Manual Teams/M365 pilot across online/offline/reconnect and risky confirmation.
- Red-team prompt injection and confused-deputy/cross-tenant scenarios.

## Done criteria

- [ ] Copilot calls a public authenticated relay, never localhost.
- [ ] Only explicitly published workflows and typed inputs are visible.
- [ ] Execution stays local and risky runs require desktop confirmation.
- [ ] Entra tenant/user/scope mapping and revocation are tested.
- [ ] Copilot reports asynchronous state accurately and deduplicates requests.
- [ ] Pilot/admin/privacy/security gates pass before broad publication.

## STOP conditions

- Plan 011 is not production-ready or has no approved identity/tenant owner.
- The connector would use a shared static API key or unauthenticated endpoint.
- Product wants Copilot to run arbitrary shell commands/workflow graphs.
- Full agent output, code, paths, or run logs are requested without a separate
  disclosure/redaction design.
- Microsoft tenant/admin/DLP approval is unavailable for the pilot.

## Maintenance notes

- Copilot Studio/Microsoft 365 publishing and connector APIs evolve quickly;
  verify official docs and re-run evaluations before each release.
- Version the OpenAPI and agent instructions together.
- Keep MCP/A2A as adapters; the remote invocation policy remains the source of
  truth.
