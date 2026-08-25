# Plan 030: Make CLI and Alfred harnesses first-class

> **Executor instructions**: This plan changes the workflow contract without
> removing or demoting any current provider CLI. Existing graphs must continue
> to run unchanged. Stop at every STOP condition; do not silently reinterpret a
> saved agent node. When complete, update this plan's row in `plans/README.md`.
>
> **Drift check (run first)**: re-read `src-tauri/src/agents/mod.rs`,
> `src-tauri/src/runner/mod.rs`, `src-tauri/src/commands/mod.rs`,
> `src/features/workflow/types.ts`, and the agent-node settings component. The
> current implementation is CLI-first, but the exact node field names may have
> moved. Preserve the current serialized provider IDs and make the new field
> additive.

## Status

- **Priority**: P0
- **Effort**: L
- **Risk**: HIGH
- **Depends on**: none
- **Category**: agent architecture / compatibility
- **Planned at**: 2026-08-24
- **Implementation**: TODO

## Goal

Add an explicit, persisted harness choice:

- `cli`: Alfred delegates execution to the user's installed provider CLI and
  keeps the existing behavior.
- `alfred`: Alfred owns the account, runtime selection, event normalization, and
  native provider execution.

`cli` is not a legacy mode. It remains a supported first-class harness for
users who want the vendor's own local harness, configuration, plugins, and
sessions.

## Current state

- `AgentProvider` and `AgentAdapter` live in `src-tauri/src/agents/mod.rs`.
- `adapter_for()` selects one subprocess adapter per provider.
- `runner::execute_run()` builds context, invokes the adapter, streams activity,
  persists run steps, and handles cancellation.
- Existing agent nodes persist provider/model/skill data in workflow graph JSON.
- A missing new field must therefore mean `cli`, preserving every existing
  workflow and fixture.

## Architecture contract

Keep these concepts separate:

1. **Provider**: Claude Code, Cursor, Codex, OpenCode, GitHub Copilot, Gemini,
   or Grok.
2. **Harness**: `cli` or `alfred`.
3. **Account**: provider identity and native credential, only for `alfred`.
4. **Runtime**: the implementation that executes a turn and emits normalized
   events.

Do not encode `cli` or `alfred` into provider IDs. Do not rename existing
`claude_code`, `cursor`, `codex`, `opencode`, `github_copilot`, `gemini`, or
`grok` values.

Recommended wire shape:

```json
{
  "type": "agent",
  "data": {
    "provider": "codex",
    "harness": "cli",
    "model": "gpt-5.6-luna"
  }
}
```

`harness` is optional on input and defaults to `cli`. New writes should emit an
explicit value once the UI has selected one.

Never place OAuth access tokens, refresh tokens, provider credentials, or
credential-store payloads in this graph object.

## Scope

**In scope**:

- Rust/frontend harness enum and serialization.
- Additive agent-node field with old-graph defaulting.
- A runner-level harness router that preserves all CLI adapters.
- Harness-aware model/account capability DTOs.
- UI choice between the two harnesses without hiding CLI providers.
- Focused migration, serialization, and routing tests.
- Documentation of the two-harness product contract.

**Out of scope**:

- Native provider OAuth or API calls.
- Removing, renaming, or weakening any CLI adapter.
- Importing credentials from CLI files/keychains.
- A generic provider plugin system.
- A new cloud service or Alfred account.
- Provider-specific UI beyond the harness choice and capability states.

## Implementation steps

### Step 1: Define the stable harness contract

Add a Rust enum and matching TypeScript union with explicit serde casing.
Provide a single parser that rejects unknown persisted values rather than
silently selecting a provider or granting native access.

Add a small `AgentExecutionTarget`/equivalent value object containing provider,
harness, optional account reference, model, working directory, and request
metadata. Keep account references opaque.

**Verify**: round-trip `cli` and `alfred`; missing field resolves to `cli`;
unknown values produce a stable validation error; serialized DTOs contain no
credential material.

### Step 2: Route execution without touching CLI adapters

Move the existing `adapter_for(provider)` selection behind a harness router.
The `cli` branch must call the current adapter implementations unchanged.
The `alfred` branch must return an explicit `native_runtime_unavailable` error
until a provider plan registers a runtime; it must not fall back to a CLI
silently.

Include the selected harness in safe run-step metadata and activity labels, but
never include tokens or raw provider credential errors.

**Verify**: every current provider still reaches its existing adapter in `cli`
mode; `alfred` mode fails clearly when no native runtime is registered; a
native failure cannot accidentally execute a CLI node.

### Step 3: Extend workflow types and editor

Add the optional field to the agent-node data type and persistence helpers.
Update the agent settings UI to show:

- provider selector;
- harness selector;
- model selector scoped to provider+harness;
- native account requirement when `alfred` is selected;
- honest unavailable state when no native runtime exists.

Switching harnesses must preserve prompt, skills, working directory, and model
when compatible. Clear only fields that are invalid for the selected harness.

**Verify**: old graphs render as CLI; switching modes is deterministic; an
unavailable native provider cannot be saved as if it were connected; saved graph
JSON remains bounded and secret-free.

### Step 4: Update commands and model discovery boundaries

Add harness to provider/model DTOs. Existing CLI model discovery remains the
source for `cli`. Native providers must register a separate discovery function
and must not call `find_bin()` as a hidden prerequisite.

Add capability flags such as `supportsOAuth`, `supportsApiKey`, `supportsUsage`,
and `requiresAccount`, but keep them descriptive. Capability flags must not
claim a provider feature before its plan passes the provider contract gate.

**Verify**: CLI model discovery behavior is unchanged; native-unavailable
providers return explicit capability state; model IDs cannot cross provider or
harness boundaries without validation.

### Step 5: Document compatibility rules

Add a section to `docs/` or the relevant agent documentation covering:

- CLI is first-class, not deprecated;
- Alfred harness is additive;
- old graphs default to CLI;
- native account credentials are separate from CLI credentials;
- provider-specific native support may be narrower than CLI support.

Do not promise “OAuth works for every provider” in user-facing copy.

## Subagent-ready ownership slices

- **Contract slice**: Rust/TypeScript enums, serialization, DTO tests.
- **Runner slice**: harness router and safe run metadata; no provider code.
- **UI slice**: agent settings and old-graph compatibility.
- **Verification slice**: focused fixtures for routing, migration, and redaction.
- **Documentation slice**: two-harness contract and user-facing capability copy.

Slices may run in parallel after the contract names are frozen. No slice may
edit the same runner or workflow type block without coordinating with the
integration owner.

## STOP conditions

- Existing saved graphs require a destructive migration.
- Any implementation path reads or imports a CLI credential store.
- `alfred` silently executes a CLI fallback.
- Provider IDs or current CLI behavior must be renamed to make the field fit.
- The UI would imply native support for a provider without a completed provider
  plan.

## Verification

Focused commands:

```bash
bun test tests/workflow-list.test.tsx
bun run build:frontend
cargo test --locked --manifest-path src-tauri/Cargo.toml agents runner
cargo check --locked --manifest-path src-tauri/Cargo.toml
```

Before completion, run the full repository gate required by the current
`plans/README.md` and update the index only after all focused and full gates
pass.

## Done criteria

- [ ] `cli` and `alfred` are explicit first-class harness values.
- [ ] Existing graphs without `harness` still use the current CLI path.
- [ ] No CLI adapter was renamed, removed, or hidden.
- [ ] Native-unavailable errors are explicit and never fall through to CLI.
- [ ] Workflow JSON and run events contain no credentials.
- [ ] Focused and full verification gates pass.
- [ ] `plans/README.md` reflects the actual status.
