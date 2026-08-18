# 018 — Stream safe agent activity in real time

**Implementation status:** DONE — 2026-08-11. Automated verification is
checked below; paid-provider live runs and manual feel checks remain unchecked.

- **Commit:** unavailable — this workspace has no usable `HEAD`; use the drift
  hashes below
- **Severity:** HIGH
- **Category:** Missed opportunities
- **Estimated scope:** 11 files, ~350–500 lines

## Problem

The UI promises a “Live log,” but the two paths most visible in the supplied
running-state screenshot are not live: Claude Code requests one final JSON
object, and Codex deliberately disables the line callback for its JSONL mode.
Animating a node over a silent stream would communicate activity without giving
the user anything useful to inspect.

The fix must expose provider-neutral activity—status, tool use, commands, file
work, and assistant messages—without displaying raw chain-of-thought. The
stream is an observability surface, not a reasoning transcript.

## Where

| File | Lines | What's there |
| --- | --- | --- |
| `src-tauri/src/agents/claude_code.rs` | 34–54, 75–116 | Requests single-result JSON and receives no useful line until completion |
| `src-tauri/src/agents/codex.rs` | 32–70, 128–175 | Requests JSONL but sets `line_handler` to `None`; parses only after the process exits |
| `src-tauri/src/agents/cursor.rs` | 33–66 | Prefers text output instead of the installed CLI's structured live stream |
| `src-tauri/src/agents/opencode.rs` | 38–66, 109–184 | Already streams text events; this is the closest existing exemplar |
| `src-tauri/src/agents/mod.rs` | 124–152 | The shared hook transports only an untyped `&str` line |
| `src-tauri/src/runner/mod.rs` | 52–67, 796–845 | `RunEvent` has no structured activity; agent lines become generic `step_log` events |
| `src-tauri/src/runner/mod.rs` | 626–645 | Every node is artificially delayed 350ms so the UI can show it |
| `src/features/workflow/types.ts` | 404–435 | Frontend events/logs mirror only generic kind/message/output fields |
| `src/features/workflow/store.ts` | 1180–1189, 1268–1273, 1330 | Every line appends a log and may concatenate into step output |

### Current code

```rust
// src-tauri/src/agents/claude_code.rs:34
let args = vec![
    "-p".into(),
    prompt,
    "--model".into(),
    model.clone(),
    "--output-format".into(),
    "json".into(),
    "--permission-mode".into(),
    "bypassPermissions".into(),
    "--max-turns".into(),
    "40".into(),
];
```

```rust
// src-tauri/src/agents/codex.rs:62
let structured = args.iter().any(|arg| arg == "--json");
let line_handler = if structured { None } else { hooks.on_line };
```

```rust
// src-tauri/src/runner/mod.rs:643
// Brief pause so the UI can show the active node.
thread::sleep(Duration::from_millis(350));
```

```ts
// src/features/workflow/store.ts:1268
if (event.kind === "step_log" && event.nodeId && event.output) {
  const previous = stepOutputs[event.nodeId] ?? "";
  stepOutputs[event.nodeId] = previous
    ? `${previous}\n${event.output}`
    : event.output;
}
```

## Drift gate

Before implementing, run:

```bash
sha256sum src-tauri/src/agents/mod.rs \
  src-tauri/src/agents/process.rs \
  src-tauri/src/agents/claude_code.rs \
  src-tauri/src/agents/codex.rs \
  src-tauri/src/agents/cursor.rs \
  src-tauri/src/agents/opencode.rs \
  src-tauri/src/runner/mod.rs \
  src/features/workflow/types.ts \
  src/features/workflow/store.ts tests/store.test.ts
```

Reviewed-at hashes:

```text
18db9525d53097b62daadc9aef682ec53b6d7cc495448608fe0e24ba448adf32  src-tauri/src/agents/mod.rs
5230f8a3e88627e505fdd644cecf15ae5a0b07eac94320863a33341ebef75f80  src-tauri/src/agents/process.rs
784a8efd2c68bd392d4c9409cce331511e402aa9da302827b5aad9e68b9b369b  src-tauri/src/agents/claude_code.rs
ee71b78abaa82a08a5cdb9e2a9b0389a048b06f2fdd658264d6806a24b21b47b  src-tauri/src/agents/codex.rs
164981cadb62a330cfcacc855a5610c67e5afea5bc8100864d19f4f666e4c7d0  src-tauri/src/agents/cursor.rs
3e5e7249c1b06aba5b9c37a2cc61a97bf6f6d4438e8fa34e31e65edf55f600c7  src-tauri/src/agents/opencode.rs
a8e5f81a98e287c44ce880890bde7406c586017e58eb8cd64dfb802cde5422bf  src-tauri/src/runner/mod.rs
7fabeafd5ff790bb4e0a9c0ba2bb53b88a9c3927d188003fde9363c93f48f58c  src/features/workflow/types.ts
3f131b29b097ab000ae4b6c6edeaa9c84cf9ecc57465eaf081969174b71706c3  src/features/workflow/store.ts
ae7929f74ff0ce791d077d769ea8d50d9edfd66eb971e6b7aa3dce08006ae847  tests/store.test.ts
```

If an adapter's argument list, `AgentRunHooks`, `RunEvent`, or
`handleRunEvent` has changed, reconcile that seam before editing. Preserve the
concurrent provider-usage and auth-toast work; do not overwrite it.

## Target

### Provider-neutral event contract

Add `src-tauri/src/agents/activity.rs` and export these serialized types from
`agents/mod.rs`:

```rust
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AgentActivityKind {
    Status,
    Assistant,
    Tool,
    Command,
    File,
    Error,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AgentActivityState {
    Started,
    Completed,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AgentActivity {
    pub id: String,
    pub kind: AgentActivityKind,
    pub state: AgentActivityState,
    pub label: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}
```

Contract rules are exact:

- `id` is stable across started/completed events. Use the provider's tool/item
  ID; for provider phases without one, use `<provider>:<phase>`.
- `label` is one line, whitespace-collapsed, and capped at 160 Unicode scalar
  values.
- `detail` is capped at 32 KiB after redaction/truncation.
- Never place the initial prompt, provider thinking/reasoning text, credentials,
  environment dumps, or a complete tool input JSON object in either field.
- A reasoning/thinking event becomes `{ kind: Status, label: "Thinking" }`
  with `detail: None`. Do not forward its text.
- Unknown provider events are ignored. Never dump raw JSON into the UI.

Replace the untyped line hook with:

```rust
pub struct AgentRunHooks<'a> {
    pub control: Option<&'a RunControl>,
    pub on_activity: Option<&'a dyn Fn(&AgentActivity)>,
}
```

Keep `run_cmd`'s internal line callback: adapters still need each raw JSONL line
to parse their own protocol. Only the adapter-to-runner hook becomes typed.

Add optional `activity: Option<AgentActivity>` to `RunEvent`. Emit normalized
events with `kind: "agent_activity"`, `status: "running"`, `message` equal to
`activity.label`, and `output: None`. The nested `activity.detail` is the only
console detail field. This keeps activity out of `stepOutputs`.

Mirror the enums/shape in `types.ts` and add `activity?: AgentActivity | null`
to `RunEvent` and `RunLogLine`.

### Exact provider mappings

| Provider event | Normalized activity |
| --- | --- |
| Session/system init | `status/completed`, label `<Provider> session started` |
| Thinking/reasoning block | `status/started`, label `Thinking`, no detail |
| Assistant text block/message | `assistant/completed`, label `Agent response`, text in detail |
| Tool call start | `tool/started`, label from the tool name, stable call ID |
| Tool call completion/result | same ID, `tool/completed`, concise result in detail |
| Shell/command item | `command/started|completed`, command summary in detail |
| File change item | `file/completed`, label `Changed <relative path>` |
| Provider error event | `error/completed`, safe provider error text in detail |

Provider-specific requirements:

1. **Claude Code:** change the first attempt to `--output-format stream-json
   --verbose`; retain `-p`, model, permission mode, and max turns. Do not add
   `--include-partial-messages`, `--forward-subagent-text`, or any thinking
   flag. Parse `system`, `assistant`, `user` tool-result, and final `result`
   records. Accumulate final assistant text and extract the same duration,
   turns, cost, and usage metadata currently taken from the final JSON object.
   If an older CLI rejects `stream-json`, retry the current single-JSON command
   and emit one `Waiting for final response` status rather than failing solely
   because streaming is unavailable.
2. **Codex:** keep `exec --json`, but pass every JSONL line through a parser and
   still retain it in `CmdOutput.stdout` for final result/usage aggregation.
   Map `thread.started`, `turn.started`, `item.started`, `item.completed`,
   `turn.completed`, `turn.failed`, and `error`. For item types, expose
   `command_execution`, `file_change`, `mcp_tool_call`, `web_search`, and
   `agent_message`; convert `reasoning` only to the generic `Thinking` status.
   Preserve the current non-JSON fallback attempts and their text line hook.
3. **Cursor:** make the first attempt `-p --force --model <model>
   --output-format stream-json <prompt>`. Parse `system/init`, `tool_call`
   started/completed, `assistant`, and terminal `result` events. The provider's
   documented print stream suppresses thinking; do not attempt to reconstruct
   it. Keep the current text attempts as compatibility fallbacks.
4. **OpenCode:** keep `--format json` and do not add `--thinking`. Replace
   `event_text` with a parser that emits text and tool lifecycle events the
   installed JSON schema exposes, while preserving the existing final response
   and token/cost aggregation.
5. **Custom agents:** retain their existing human-readable line behavior. Wrap
   each line as `assistant/completed` with a deterministic sequence ID; do not
   reinterpret arbitrary custom output as provider JSON.

Official protocol references to pin in code comments/tests:

- Anthropic CLI/SDK stream shape:
  `https://docs.anthropic.com/en/docs/claude-code/cli-usage`
- Cursor stream event schema:
  `https://docs.cursor.com/en/cli/reference/output-format`
- OpenCode CLI flags:
  `https://dev.opencode.ai/docs/cli/`
- Codex's installed `codex exec --help` is the local authority for `--json`;
  retain the already implemented event parser as the schema baseline.

### Store behavior

For `agent_activity`:

- Never append `activity.detail` into `stepOutputs`.
- Upsert a `RunLogLine` by `runId + nodeId + activity.id`. A completion replaces
  the matching started row instead of adding a duplicate.
- Append events with a new activity ID in arrival order.
- Keep ordinary `started`, `step_started`, `step_log`, completion, and failure
  behavior unchanged.
- Use a module constant `MAX_RUN_LOG_LINES = 1_000`. When a run exceeds it,
  retain the latest 1,000 rows. Do not cap `stepOutputs` or final output.
- Remove the unconditional 350ms sleep after `step_started`. A real activity
  event, not artificial runtime latency, is now responsible for visibility.

**Why these values:** 160 characters keeps labels scannable; 32 KiB retains
useful terminal output without letting one event dominate memory; 1,000 logical
events is ample for inspection because tool start/completion pairs are upserted;
zero artificial delay keeps every workflow step connected to actual work.

## Conventions to follow

- `src-tauri/src/agents/opencode.rs` already parses JSONL before calling the
  shared hook and separately aggregates final output/stats. Preserve that
  separation.
- `src-tauri/src/agents/codex.rs::parse_json_output` is the existing Codex final
  output/usage parser. Extend it; do not replace it with displayed console text.
- `src-tauri/src/runner/mod.rs::emit` remains the only Tauri event emission
  seam.
- `src/features/workflow/store.ts::handleRunEvent` remains the only frontend
  event reducer. Do not create a parallel activity store.

## Steps

1. Add the provider-neutral activity types, normalization/truncation helpers,
   and unit tests in `agents/activity.rs`.
2. Replace `AgentRunHooks.on_line` with `on_activity` and adapt every built-in
   and custom-agent call site without changing cancellation or timeout logic.
3. Add streaming parsers and fixture tests for Claude, Codex, Cursor, and
   OpenCode using the exact mapping table above.
4. Extend `RunEvent`; have the runner emit `agent_activity` records for the
   active node and remove the 350ms artificial pause.
5. Mirror the event contract in TypeScript. Upsert lifecycle rows, exclude
   activity details from step output, and cap the run log at 1,000 rows.
6. Extend `tests/store.test.ts` for activity start/completion upsert, distinct
   activity ordering, output isolation, cap behavior, and normal completion.

## Out of scope

- Raw chain-of-thought, reasoning tokens, or hidden provider prompts.
- Token-by-token assistant text deltas.
- Attaching a new OS terminal to an already-running child process.
- Persisting live events to SQLite or restoring them after an app reload.
- Changing permissions, approval modes, timeouts, or cancellation semantics.
- Do not introduce a new animation or parsing library.
- Do not change unrelated motion timings.

## Verification

**Build**

- [x] `cargo test --manifest-path src-tauri/Cargo.toml agents::activity`
- [x] Focused parser tests pass for all four providers.
- [x] `cargo test --manifest-path src-tauri/Cargo.toml`
- [x] `bun test tests/store.test.ts`
- [x] `bun run build:frontend`

**Behavior**

- [ ] A Claude Code run shows session/tool/assistant activity before the final
  result with the installed CLI, and still succeeds through the JSON fallback.
- [ ] A Codex `--json` run emits tool/file/message activity while retaining the
  same final response and usage numbers.
- [ ] Cursor and OpenCode stream their supported tool/text events.
- [x] Thinking/reasoning events show only `Thinking`; their content never
  appears in Tauri payloads, Zustand state, logs, or the UI.
- [x] No activity detail is duplicated into node output.
- [ ] Cancellation still kills the same child process and stops new activity.

**Feel**

- [ ] Record a run and scrub it: activity changes must correspond to real CLI
  events, never to a timer pretending progress.
- [ ] A five-step workflow starts each step immediately; there is no visible
  350ms dead pause between nodes.
- [ ] Long tool output remains inspectable without making the canvas hitch.

## Notes

Provider schemas can add fields. Parsers must ignore unknown event types and
fixture tests must assert that unknown fields do not fail a run. If a provider
version has no live protocol, show the honest `Waiting for final response`
status; do not manufacture a progress percentage or cycling fake phases.
