# Extending Connected Apps with workflow actions

Alfred exposes every provider operation through one persisted React Flow node,
`appAction`. Provider modules register declarative descriptors and Rust
executors; they do not add provider-specific frontend code or runner match
arms.

## Registering an action

At desktop startup, register an `ActionDescriptor`, `ActionLimits`, and an
`ActionExecutor` with `IntegrationsState.actions`. Registration rejects
duplicate action IDs and unsafe descriptors.

- Use a stable action ID namespaced to the provider, such as
  `slack.send_message`. Labels may change; IDs and field keys must not.
- Declare the provider ID, user-facing label and description, required scopes,
  result schema version, and fields.
- V1 field kinds are `text`, `textarea`, `boolean`, `enum`, and
  `resource_selector`.
- A resource selector names an executor-owned option source. Its browser-facing
  command receives a connection ID and returns bounded `{id, label}` entries;
  it never receives a credential. The node persists the immutable ID plus a
  UI-only `<field>__display` snapshot so renamed or temporarily unavailable
  resources remain understandable. Rust validates but does not authorize by
  the snapshot; providers always recheck the ID at execution time.
- Mark interpolation only on string fields that intentionally support
  `{{context}}`, `{{output}}`, and `{{cwd}}`.
- Set provider-specific output limits only when they are tighter than the
  framework defaults: 64 KiB serialized and JSON depth 8.
- Set `outputIsUntrusted` for knowledge, mail, message, or other externally
  authored text that will become downstream agent context. The runner then
  labels it as external data and explicitly denies instruction/authorization
  semantics.

Descriptors are serialized to React, so they must never contain access tokens,
refresh tokens, authorization headers, client secrets, tenant-specific secret
defaults, or executable provider code. Secret input fields are not supported.

## Executor boundary

`ActionExecutor` is an object-safe async Rust trait. It receives:

- backend-validated input;
- the matching healthy connection metadata;
- a backend-only `TokenAccessCapability`; and
- cancellation state.

Use `TokenAccessCapability::with_credential` only long enough to build the
backend provider request. Provider APIs and token-bearing HTTP requests stay in
Rust. Never put a credential in `ActionResult`, an `ActionError`, a log line,
provider request ID, artifact metadata, or a workflow event. The framework
checks results and selector pages against the credential values before they can
be returned or persisted.

Executors must not detach request tasks. The registry owns timeout and
cancellation; dropping the executor future must prevent later result handling
or a second execution. When an executor returns `provider_unauthorized`, the
registry uses the shared Connected Apps refresh service and retries exactly
once.

Return only stable `ActionErrorCode` values. Do not wrap or store raw provider
responses. Provider correlation IDs may be attached only when they use the
bounded safe identifier format.

## Testing checklist

Every provider action should cover:

- descriptor registration and duplicate rejection;
- required and unknown field validation;
- connection/provider and required-scope validation;
- successful execution with a fake credential store;
- timeout and cancellation without detached completion;
- one unauthorized refresh-and-retry;
- rate-limit and unavailable error mapping;
- 64 KiB/depth enforcement, including any tighter provider limit;
- resource pagination, query bounds, item bounds, caching, and failure state;
- serialized command, run-step, and event output containing no credential
  fixture; and
- a workflow round trip that preserves unknown newer input keys.

Provider-specific actions should live in their provider module. The only core
registration change should be the provider's startup registration call; the
generic runner and React node remain unchanged.

Knowledge providers also follow the extraction, citation, and retention rules
in [`knowledge-connectors.md`](knowledge-connectors.md).
