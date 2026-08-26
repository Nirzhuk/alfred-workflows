# Plan 032: Define the native harness compatibility contract

> **Executor instructions**: This plan freezes the provider-neutral runtime
> contract before provider implementations diverge. It is a contract plan, not
> permission to add provider-specific behavior everywhere. Stop when a provider
> cannot map safely to the contract; record a capability gap instead of adding
> an unbounded escape hatch.

## Status

- **Priority**: P0
- **Effort**: L
- **Risk**: HIGH
- **Depends on**: Plans 030–031
- **Category**: agent runtime / compatibility
- **Planned at**: 2026-08-24
- **Implementation**: FOUNDATION DONE; capability contract v3 tool-owner
  cutover implemented 2026-08-26 with focused verification pending

## Goal

Define the smallest common Alfred harness contract that can host native
providers while keeping the existing CLI harness untouched.

Native providers may expose different models, tool policies, sessions, usage
windows, and auth methods. The contract must represent capability differences
honestly instead of pretending all providers are interchangeable.

## Architecture contract

### Runtime request

A native turn request must carry:

- provider and account reference;
- harness and runtime version;
- prompt/context blocks;
- model identifier validated against that provider;
- working directory and allowed workspace roots;
- permission profile;
- tool capability set;
- session/thread identifier when supported;
- cancellation handle;
- bounded output/event policy.

No request contains a raw access token. Runtime code resolves the opaque account
reference through the account service.

### Normalized events

Define bounded, provider-neutral event kinds:

- `session_started`
- `turn_started`
- `assistant_delta`
- `tool_started`
- `tool_progress`
- `tool_completed`
- `approval_requested`
- `approval_resolved`
- `warning`
- `turn_completed`
- `turn_failed`
- `turn_cancelled`

Provider-specific payloads may be retained only inside bounded metadata with a
schema version and explicit redaction. Chain-of-thought/reasoning text is never
persisted or emitted as ordinary assistant output.

### Capability model

Native providers declare capabilities rather than being forced into fake parity:

```text
supports_oauth
supports_api_key
supports_sessions
supports_resume
supports_model_list
supports_usage
supports_tool_calls
supports_approval_events
supports_native_filesystem
supports_native_shell
supports_mcp
supports_subagents
```

Every runtime descriptor also declares exactly one execution owner, separate
from capability flags. The descriptor carries an exact provider/product pair,
and the resolved account must match both before the runtime receives it:

```text
alfred_executed
runtime_executed_with_host_approval
no_tools
```

`alfred_executed` means model/runtime requests are converted to typed Alfred
tool calls and Alfred performs execution. `runtime_executed_with_host_approval`
means the managed runtime performs execution only after Alfred observes and
approves the bounded request. `no_tools` means tool calls and approval events
are absent. The capability contract version is 3; invalid owner/capability
combinations fail registration.

A missing capability must produce a visible, actionable state. The runner must
not silently emulate a dangerous capability with shell execution.

## Scope

**In scope**:

- Native request/event/capability types.
- Event redaction and size limits.
- Tool and approval boundary.
- Provider contract test fixtures.
- Native-vs-CLI run-history metadata.
- Versioning and compatibility policy.

**Out of scope**:

- A provider implementation.
- A generic “run arbitrary provider JSON” escape hatch.
- Unbounded event persistence.
- Cross-provider prompt translation that changes user intent.
- Cloud execution.

## Implementation steps

### Step 1: Define runtime interfaces

Create a provider-neutral native runtime trait or equivalent registry. It must
support account validation, model discovery, turn execution, cancellation, and
usage snapshot where declared.

Keep the current synchronous CLI adapter contract separate if changing it to an
async native interface would create a broad unrelated diff. The harness router
may bridge the two deliberately.

**Verify**: a fake native runtime can be registered, invoked, cancelled, and
unregistered without a provider module or subprocess.

### Step 2: Define event normalization

Create conversion helpers from provider events to normalized events. Bound:

- event count per turn;
- text bytes per event;
- metadata depth and serialized size;
- tool output bytes;
- error text length.

Redact tokens, authorization headers, cookies, private keys, raw prompt secrets,
and provider credential paths before persistence or emission.

**Verify**: fixtures for oversized payloads, malformed events, reasoning
content, nested tool results, provider errors, and secret-looking fields.

### Step 3: Define tools and approval

Register Alfred-owned tools separately from runtime-executed tools. A runtime
must use the descriptor owner above; boolean capability combinations are not a
substitute for ownership. Runtime execution is allowed only when Alfred can
observe, approve, cancel, and bound the operation before execution.

For Alfred-owned tools, define a stable request/result shape for:

- file read/write/edit;
- directory listing;
- shell/process execution;
- patch application;
- MCP/tool delegation where explicitly supported.

The native harness must never inherit `bypassPermissions`, `--full-auto`,
`--allow-all`, or equivalent CLI flags without an explicit Alfred permission
profile.

**Verify**: approval required, approval denied, cancellation during a tool,
workspace escape, output limit, and command timeout fixtures.

### Step 4: Define sessions and context policy

Support ephemeral turns first. Add resume/fork/compaction only when a provider
reports a stable session capability. Persist only Alfred-safe thread metadata
and bounded run outputs.

Skills must be resolved by Alfred before a native call. Native mode must not
assume that a provider CLI will interpret `/skill-name` commands.

**Verify**: context ordering, bounded context, missing skill, session-unavailable
provider, cancellation, and resume identity fixtures.

### Step 5: Define conformance gates

Every native provider must pass the same focused contract suite:

1. auth/account state;
2. model validation;
3. one streamed turn;
4. tool/approval behavior or explicit unsupported capability;
5. cancellation;
6. timeout and retry classification;
7. usage snapshot or honest unavailable state;
8. redacted error/event output;
9. disconnect/revoke behavior;
10. no CLI binary dependency in Alfred mode.

## Subagent-ready ownership slices

- **Runtime contract**: types, registry, fake runtime.
- **Event safety**: normalization, bounds, redaction.
- **Tool boundary**: permissions, approvals, cancellation, process limits.
- **Session/context**: skills, context budgets, ephemeral/resume semantics.
- **Conformance suite**: provider-neutral fixtures and acceptance helpers.

Provider agents consume this contract; they must not redefine it independently.

## STOP conditions

- A provider requires unbounded raw event passthrough.
- A provider cannot expose whether a tool was approved or executed.
- Native mode would silently execute commands outside the Alfred permission
  profile.
- A provider capability can only be implemented by scraping CLI output.
- The contract requires provider-specific fields in the core runner for one
  provider only.

## Verification

```bash
bun test
bun run build:frontend
cargo test --locked --manifest-path src-tauri/Cargo.toml agents runner
cargo check --locked --manifest-path src-tauri/Cargo.toml
```

Add a contract-test report listing each provider capability as `supported`,
`unsupported`, or `blocked`, with evidence and no optimistic defaults.

## Done criteria

- [x] Native runtime and normalized event contracts are versioned.
- [x] Provider capabilities are explicit and bounded.
- [x] Tool/approval/cancellation ownership is documented.
- [x] Alfred resolves skills/context in native mode.
- [ ] Re-run the fake native runtime suite against capability contract v3.
- [x] Provider plans can proceed without editing core semantics.
