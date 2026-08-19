# Plan 009: Add a descriptor-driven app action framework

> **Executor instructions**: Implement only after Plan 008 is DONE. Run each
> verification gate and update `plans/README.md` on completion. Reconcile drift
> instead of copying old line numbers blindly.
>
> **Drift check (run first)**: verify Plan 008's postconditions, then run
> `shasum -a 256 src-tauri/src/runner/mod.rs
> src/features/workflow/types.ts src/features/workflow/add-step-items.ts
> src/features/workflow/components/node-types/index.ts
> src/features/workflow/components/node-settings-modal/node-settings-modal.tsx`.
> Baseline hashes at Git commit `36835c9` on 2026-08-13 begin `d19cbc76`,
> `42d759c4`, `9abd03b9`, `b24bd5bb`, and `8cd2f072`. Differences are expected
> if 008 touched shared types; re-read and
> retain its connection abstractions.

## Status

- **Priority**: P0
- **Effort**: L
- **Risk**: MED
- **Depends on**: Plan 008
- **Category**: architecture
- **Planned at**: 2026-08-11; reconciled at `36835c9`, 2026-08-13
- **Implementation status**: DONE (2026-08-19). Automated gates, macOS package
  build, and packaged command smoke are complete. Plan 008's signed
  macOS/Windows/Linux credential-store smoke remains on 008.

## Why this matters

Provider features need workflow actions such as “send Slack message,” “send
email,” or “create GitHub issue.” Adding one React Flow node type and one large
runner branch per action will make the existing manual node unions and runner
match unmaintainable. The framework should add exactly one generic node type
and let providers register descriptors and executors.

## Current state

- `src/features/workflow/types.ts:292-305` is a closed union of node payloads.
- `src/features/workflow/add-step-items.ts:27` has only Context, Agent, and Sink
  groups. App actions have no category.
- `node-types/index.ts` and `node-settings-modal.tsx` manually register and
  dispatch each type.
- `src-tauri/src/runner/mod.rs:479` contains a large match; the generic HTTP
  branch begins near line 949.
- HTTP-node headers live in workflow JSON. Connected app actions must resolve
  credentials by connection ID inside Rust instead.

## Design contract

Add one persisted node type, `appAction`, with non-secret data:

```ts
type AppActionNodeData = {
  type: "appAction";
  label: string;
  providerId: string;
  actionId: string;
  connectionId: string;
  input: Record<string, unknown>;
};
```

Provider/action descriptors drive labels, fields, selectors, validation, and
output metadata. Execution is owned by Rust. Descriptors are declarative data,
not JavaScript supplied by a provider. V1 supports scalar text, textarea,
boolean, enum, and provider resource selector fields; arbitrary nested forms or
provider-loaded frontend code are out of scope.

## Commands you will need

- `bun test`
- `bun run build:frontend`
- `cargo test --manifest-path src-tauri/Cargo.toml`
- `cargo check --manifest-path src-tauri/Cargo.toml`
- `rg -n '"appAction"|ActionDescriptor|ActionResult' src src-tauri tests`

## Scope

**In scope**:

- Generic `appAction` React Flow node, settings UI, palette group, serialization.
- Rust action registry/trait and one runner branch.
- Descriptor commands, provider resource-option loading, backend validation,
  output size limits, normalized errors, and redaction.
- A fake/demo provider available only in tests to prove the extension seam.

**Out of scope**:

- Real Slack/Microsoft/Google/GitHub actions.
- Integration triggers, cloud relay, arbitrary plug-in code, or third-party
  marketplace loading.
- Replacing generic HTTP or existing Git-host nodes.

## Git workflow

Use the current repository history and branch conventions. Preserve Plan 008
and unrelated user changes, do not commit/push without instruction, and keep
provider-specific implementations out of this framework diff. Update the index
after verification.

## Implementation steps

### Step 1: Define stable action descriptors

Under the Plan 008 Rust integration module, define:

- `ActionDescriptor { provider_id, action_id, label, description, fields,
  required_scopes, output_schema_version }`
- field descriptors with stable keys, kind, required/default, secret=false,
  optional dynamic option source, and interpolation support;
- `ActionRequest { connection_id, provider_id, action_id, input }`;
- `ActionResult { summary, output, artifacts, provider_request_id }` where
  `output` is JSON with explicit byte/depth limits.

The default provider-result limit is 64 KiB serialized with maximum JSON depth
8; providers may only tighten it. Reject an over-limit result before persistence
with stable `output_too_large`—never silently byte-slice it. UI previews may be
shorter and must carry an explicit truncation marker without changing the
stored action-result schema.

Resource-selector fields name the backend option source. Define
`list_app_action_resources(connection_id, provider_id, action_id, field_key,
query, page_token)` returning `{ items: [{id, label}], next_page_token }` with
hard query, page, and item limits. It receives a connection ID, never a token;
cache only bounded ID/label results in memory with a short TTL.

Action IDs are stable API identifiers such as `slack.send_message`, never UI
labels. Reject duplicate registrations at startup. Descriptors must never
contain tokens or tenant-specific secret defaults.

**Verify**: Rust tests cover duplicate IDs, descriptor serialization, input
validation, unknown fields, missing scope, and output-size rejection.

### Step 2: Build the Rust action executor registry

Define an object-safe async executor boundary returning a boxed, sendable future
(equivalent to `Pin<Box<dyn Future<Output = Result<ActionResult, ActionError>> +
Send + '_>>`). Use the existing Tauri/Tokio runtime and bridge to the current
blocking runner without creating a nested runtime. The executor receives
validated input, a connection record, and a token-access capability. Do not pass
raw credentials through the generic runner or return them in errors. The
framework should handle:

- connection/provider match and healthy status;
- required-scope checks;
- timeout and cancellation;
- stable error codes (`connection_required`, `scope_missing`, `rate_limited`,
  `provider_unauthorized`, `provider_unavailable`, `invalid_input`);
- one on-demand refresh-and-retry through Plan 008's shared refresh service;
- redacted logging and provider request/correlation IDs.

Add a test-only fake action executor. Avoid a global mutable map that is hard
to isolate in tests; construct and inject the registry at app startup.

**Verify**: unit tests cover success, cancellation, timeout, refresh retry,
provider mismatch, redacted errors, and no double execution after timeout.

### Step 3: Delegate one runner node type

Add a single `"appAction"` arm in `runner/mod.rs`. It interpolates supported
string fields using the existing workflow context rules, passes the result to
the action registry, and stores only normalized output. Provider modules must
not add their own match arms.

Enforce the descriptor result limits before persistence. Truncate only derived
UI previews with the explicit marker; reject an oversized provider result.
Never persist request headers or credential data. Include provider/action/
connection identifiers in safe run metadata so the Activity UI can explain a
failure without exposing inputs such as email body.

**Verify**: runner tests execute the fake action, consume its output in a later
node, cancel it, and prove a fixture secret is absent from run events/results.

### Step 4: Add the generic frontend node and settings form

Extend `WorkflowNodeData`, model conversion, node registry, add-step groups,
and settings dispatch with only `AppActionNodeData`. Add an **Apps** palette
group and an App Action card. The settings form flow is:

1. provider;
2. action;
3. compatible connection;
4. descriptor-driven inputs.

Dynamic selectors call `list_app_action_resources` with connection ID and query
text; they do not receive a token. Register the command with the integration
commands and verify it from a packaged app with
`src-tauri/capabilities/default.json`. Preserve unknown input keys when
opening/saving a newer workflow with an older app version, but display a
compatibility warning. Validate required values in React for feedback and again
in Rust for trust.

**Verify**: frontend tests cover descriptor rendering, provider/action reset,
connection filtering, selector loading/failure, workflow round trip, unknown
descriptor handling, and no secret field kind.

### Step 5: Add extension documentation

Document how a provider registers actions, scopes, selectors, tests, errors,
and redaction. Include a checklist requiring provider APIs to stay in Rust and
forbid client secrets/tokens in descriptors or React state.

## Test plan

- [x] `bun test` — 50 passed.
- [x] `bun run build:frontend`.
- [x] `cargo test --manifest-path src-tauri/Cargo.toml --locked` — 80 passed.
- [x] `cargo check --manifest-path src-tauri/Cargo.toml --locked`.
- [x] Save/reopen a workflow containing the fake `appAction` node in a
  serialization round-trip test, including an unknown newer field.
- [x] Execute the fake action, feed normalized output to interpolation, exercise
  cancellation/timeout/refresh, and reject current or pre-refresh token
  fixtures before persistence.
- [x] Build a packaged macOS debug app with
  `bun run tauri build --debug --bundles app`.
- [x] Invoke descriptor and resource commands from the packaged main window and
  inspect persisted run events/SQLite manually; no real provider action is
  registered in Plan 009 by design. Confirmed by the maintainer 2026-08-19.
- Plan 008's signed macOS/Windows/Linux credential-store smoke remains on 008.

## Done criteria

- [x] One generic node supports all registered app actions.
- [x] New providers register descriptors/executors without editing runner match.
- [x] Rust revalidates every action input and connection scope.
- [x] Tokens never enter node data, frontend state, run events, or output.
- [x] Unknown/newer descriptors fail safely without corrupting workflows.
- [x] Existing nodes and all automated build/test gates pass.

## STOP conditions

- Plan 008 does not provide a backend-only credential access boundary.
- A proposed field requires provider-supplied executable frontend code.
- Existing interpolation can expose the entire connection/token object.
- The node would need to persist a token or authorization header to function.

## Maintenance notes

- Version descriptors additively; action IDs and field keys are persisted data.
- Keep provider-specific rate limits and pagination in provider modules.
- A future plugin SDK can adapt to this registry, but dynamic untrusted plugins
  need a separate sandbox/signing design.
