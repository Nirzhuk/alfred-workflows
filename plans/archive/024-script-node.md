# Plan 024: Script steps and agent script hints

> **Executor instructions**: Add one new workflow node type (`script`) and one
> new option on the Input node. Reuse the existing agent process machinery
> (`run_cmd_with_stdin`) rather than `run_shell_command`. Do not add a script
> library, a SQLite table, template substitution into script bodies, a new
> color token, or a per-node timeout field. Do not migrate or delete existing
> `shell` nodes.
>
> **Drift check (run first)**: `src-tauri/src/runner/mod.rs`,
> `src/App.css`, and several `src/features/integrations/*` files carry a large
> staged Connected Apps batch on `main`. Re-read the `"shell"` runner arm, the
> `"prompt" | "input"` runner arm, `format_attachments_context`,
> `run_cmd_inner` in `src-tauri/src/agents/process.rs`, and the
> `.wf-node-shell` block in `src/App.css` before editing. Preserve every
> overlapping change; reference symbols, not the line numbers in this plan.

## Status

- **Priority**: P2
- **Effort**: M (~3.5 h)
- **Risk**: LOW
- **Depends on**: nothing
- **Does not depend on**: Connected Apps plans 008–023, release-money track
- **Category**: workflow
- **Planned at**: 2026-08-17
- **Implementation status**: DONE (2026-08-17). All five phases shipped.
  `bun test` 97 pass, `bun run build:frontend` clean, `cargo test` 154 pass
  (6 new script tests), `cargo fmt --check` and `git diff --check` clean.
- **Archived at**: 2026-08-19. Live smoke is no longer blocking.

## Deviations from the plan as written

- `src/platform.ts` already exports `detectDesktopPlatform()`, so
  `defaultInterpreter()` uses it instead of the sketched `navigator` sniff.
  There is no `@/` path alias in this repo — imports are relative.
- `CmdOutput` had no exit code, only `success: bool`. Added `code: Option<i32>`
  and changed `wait_child` to return `(bool, Option<i32>)` so `(exit N)` is
  real rather than inferred.
- Shebang handling was widened from inline-only to any script whose first two
  bytes are `#!`. A user's own file is run directly only when it is already
  executable; Alfred chmods only the temp file it created itself.
- `ScriptRefFields` was extracted so the Script node and the Input node option
  render identical source/path/body/interpreter controls.
- The canvas chip reuses `wf-input-attach-chip`, so no new CSS beyond adding
  `.wf-node-script` to the existing `.wf-node-shell` rule.

## Product decisions

Every decision below was resolved in design review. Do not re-open them.

- **Script source is the user's choice.** A script is either a **file** on disk
  or an **inline** body saved in the node. One shared `ScriptRef` shape covers
  both, and is reused by the Script node and by the Input node's option.
- **No script library.** The body lives in node data, saved with the graph,
  exactly like the Shell node's `command` today. Cross-node reuse is what file
  mode is for. No `scripts` table, no CRUD commands, no library UI.
- **The Script node has a target handle and a source handle**, like every other
  step. `SimpleStepNode` already renders both.
- **Upstream context reaches the script two ways**: piped to stdin, and
  exported as `ALFRED_OUTPUT`, `ALFRED_CONTEXT`, `ALFRED_CWD`. Both are
  injection-safe.
- **No template substitution into script bodies.** `apply_template` is
  deliberately not used here. Agent output containing quotes, `$`, or backticks
  would become shell injection inside the user's own script.
- **Interpreter**: on macOS/Linux an inline body starting with `#!` is honored
  directly. Otherwise a single free-text "Run with" field is used, default
  `bash`. No preset dropdown, no separate extension field.
- **Windows has no shebang.** Any `#!` line is ignored there and the "Run with"
  value is always used, defaulting to `pwsh`.
- **Execution reuses the agent process machinery**: 15-minute timeout, honors
  the run's cancel token, streams stdout lines into the run console. No
  per-node timeout field, no continue-on-error checkbox.
- **Script node: non-zero exit fails the step** and stops the run, matching the
  Shell node. The Script node is a gate.
- **Input node option: non-zero exit does NOT fail the run.** Output and exit
  code go into the prompt and the agent runs anyway. This asymmetry is
  intentional — a failing script is usually the exact information the agent
  needs ("tests fail, fix them").
- **The Input node option defaults to instruct-only.** Alfred does not execute
  anything unless the user checks "Run it before the agent".
- **Instruct mode embeds an inline body** into the prompt as a fenced code
  block; a file-source script contributes its **path only**, and the agent
  reads the file itself. No temp file is created for instruct-only mode.
- **The instruction sentence is user-editable**, defaulting to
  `Use this script for this task:`. It carries the framing, so the block gets
  no `##` heading. Executed output does get a `## Script output` heading,
  matching `## Shell output` / `## App action result`.
- **Shell is superseded but not removed.** It leaves the Add-step palette; the
  `"shell"` runner arm, `ShellNode`, `ShellSettings`, and `.wf-node-shell` all
  stay so existing saved graphs keep working untouched.
- **No new color token.** `.wf-node-script` reuses `var(--shell)` — same visual
  family, so `docs/design-system.md` needs no change.

## Product outcome

A user drops a **Script** step onto the canvas, picks a file or types an inline
body, and wires it between steps: upstream output arrives on stdin and in
`$ALFRED_OUTPUT`, stdout becomes the step's output and (optionally) appends to
context, and a non-zero exit stops the run.

Separately, on any Input step the user can attach a script and an editable
sentence, so the agent's prompt ends with:

```
Use this script for this task:
`./scripts/seed-fixtures.sh`
```

The agent decides when to run it. If the user also checks "Run it before the
agent", Alfred runs it first and appends the result under `## Script output`,
without failing the run when it exits non-zero.

## Scope

**In scope**:

- `ScriptRef` type shared by the Script node and the Input node option.
- A `script` node type: React Flow node, settings panel, palette entry,
  `titleForNodeType` case, CSS class.
- A `"script"` arm in the Rust runner with stdin, env vars, shebang handling,
  temp-file materialization and cleanup, timeout, cancel, and line streaming.
- An `envs` parameter threaded through `run_cmd_inner` plus one new public
  wrapper.
- An optional `script` field on `PromptNodeData`, rendered in the Input
  settings modal, summarized as a chip on the canvas node, honoring `blocked`.
- Instruction-block composition in the `"prompt" | "input"` runner arm.
- Removing Shell from the Add-step palette.
- Rust unit tests for the pure helpers, one TS test file, `bun run check`.

**Out of scope** (do not build):

- A script library, `scripts` SQLite table, or cross-workflow script reuse.
- Template substitution (`{{output}}`) inside script bodies.
- A per-node timeout field or continue-on-error checkbox.
- Migrating, rewriting, or deleting existing `shell` nodes.
- A new `--script` CSS color token or `docs/design-system.md` change.
- A user-facing `docs/` page (no other node type has one).
- An ADR (this reuses existing runner patterns; it sets no new architecture).
- Entitlement gating — no entitlement code exists in the repo yet.

---

## Phase 1 — Types (~30 min)

**File**: `src/features/workflow/types.ts`

1. Add the shared reference shape and the node data:

```ts
/** Where a script's text comes from. Shared by the Script node and the
 *  Input node's "use this script" option. */
export type ScriptRef = {
  source: "file" | "inline";
  /** File mode. Absolute, or relative to the workflow working directory. */
  path: string;
  /** Inline mode. On macOS/Linux a leading `#!` line is honored. */
  body: string;
  /** `bash`, `python3`, `node`, `pwsh`, … Ignored when a shebang applies. */
  interpreter: string;
};

/** Run a script and pass its stdout downstream. Supersedes `shell`. */
export type ScriptNodeData = ScriptRef & {
  kind: "script";
  label: string;
  /** Append stdout/stderr into context_prompt after the run. */
  appendOutput: boolean;
};

/** Input-node option: tell the agent about a script, optionally run it first. */
export type InputScript = ScriptRef & {
  /** Editable sentence placed above the path or body in the prompt. */
  message: string;
  /** Run it before the agent and append `## Script output`. Default false. */
  run: boolean;
};
```

2. Export the default interpreter and factories:

```ts
export const DEFAULT_SCRIPT_MESSAGE = "Use this script for this task:";

/** `pwsh` on Windows — there is no shebang there. */
export function defaultInterpreter(): string {
  return typeof navigator !== "undefined" &&
    navigator.userAgent.includes("Windows")
    ? "pwsh"
    : "bash";
}

export function defaultScriptNodeData(label = "Script"): ScriptNodeData {
  return {
    kind: "script",
    label,
    source: "inline",
    path: "",
    body: "",
    interpreter: defaultInterpreter(),
    appendOutput: true,
  };
}

export function defaultInputScript(): InputScript {
  return {
    source: "file",
    path: "",
    body: "",
    interpreter: defaultInterpreter(),
    message: DEFAULT_SCRIPT_MESSAGE,
    run: false,
  };
}
```

> Prefer `src/platform.ts` over a `navigator` sniff if it already exposes an OS
> flag — check it first. `.claude/rules/design-system.md` forbids
> component-local user-agent detection; this is a data default in a shared
> module, but reuse the shared helper if one exists.

3. Add `script?: InputScript` to `PromptNodeData`, with a doc comment noting
   it is optional and absent on legacy graphs.
4. Add `ScriptNodeData` to the `WorkflowNodeData` union.
5. Add `isScriptNodeData` using the existing `hasKind` helper.
6. Add `case "script": return "Script";` to `titleForNodeType`.

**Verify**: `bun run build:frontend` compiles.

**STOP** if `PromptNodeData` is consumed anywhere that spreads it into a
non-optional shape — fix those call sites before continuing.

---

## Phase 2 — Runner (~1 h)

### 2a. Env vars through the process helper

**File**: `src-tauri/src/agents/process.rs`

1. Add a final `envs: &[(&str, String)]` parameter to `run_cmd_inner` and apply
   it with `command.envs(...)` after `configure_agent_environment`.
2. Pass `&[]` from the existing `run_cmd` and `run_cmd_with_stdin` wrappers so
   no current caller changes.
3. Add one new public wrapper:

```rust
/// Variant used by Script steps: stdin payload plus explicit env vars.
pub fn run_cmd_with_stdin_env(
    bin: &Path,
    args: &[String],
    cwd: Option<&Path>,
    timeout: Duration,
    control: Option<&RunControl>,
    on_line: Option<&dyn Fn(&str)>,
    stdin_payload: &str,
    envs: &[(&str, String)],
) -> Result<CmdOutput, String>
```

### 2b. Script helpers

**File**: `src-tauri/src/runner/mod.rs`

Add three pure helpers (these are what the tests target) plus one impure
executor.

```rust
/// Temp-file extension for an interpreter. Windows dispatches on extension,
/// and PowerShell refuses a file that is not `.ps1`.
fn script_extension(interpreter: &str) -> &'static str

/// Absolute path for a file-source script. Relative paths resolve against the
/// workflow working directory; absolute paths pass through.
fn resolve_script_path(path: &str, cwd: Option<&str>) -> Result<PathBuf, String>

/// The Input node's instruction block: the user's message, then a fenced body
/// (inline) or a backticked path (file). No `##` heading.
fn format_script_instruction(script: &Value) -> String
```

`script_extension` mapping — match on the interpreter's file stem, lowercased:

| Interpreter stem | Extension |
|------------------|-----------|
| `bash`, `sh`, `zsh`, `fish` | `.sh` |
| `python`, `python3`, `uv` | `.py` |
| `node`, `bun`, `deno` | `.js` |
| `pwsh`, `powershell` | `.ps1` |
| `cmd` | `.cmd` |
| anything else | `.sh` |

`resolve_script_path` returns `Err("Script path is relative but this workflow
has no working directory")` for a relative path with no cwd — mirror the
wording style of the existing `gitStatus` arm.

### 2c. The executor

```rust
fn run_script(
    script: &Value,
    context: &str,
    last_output: &str,
    cwd: Option<&str>,
    control: &RunControl,
    on_line: Option<&dyn Fn(&str)>,
) -> Result<(String, i32), String>
```

Behavior:

1. **File source** — `resolve_script_path`, then error
   `Script not found: {path}` if it does not exist.
2. **Inline source** — write the body to
   `std::env::temp_dir()/alfred-script-{run_id}-{node_id}{ext}`. Hold it in a
   `Drop` guard that removes the file, so early `?` returns and cancellation
   still clean up.
3. **Interpreter resolution**:
   - Unix, inline body starting with `#!`: `chmod 0o755`
     (`std::os::unix::fs::PermissionsExt`) and execute the file directly.
   - Unix otherwise: `find_bin(interpreter)`, then `<bin> <file>`.
   - Windows: ignore any `#!`. `find_bin("pwsh")`, else
     `find_bin("powershell")`, else fall back to `cmd` — and when falling back
     to `cmd`, re-derive the extension as `.cmd` so the file is dispatchable.
   - PowerShell invocation must be
     `pwsh -NoProfile -ExecutionPolicy Bypass -File <file>`; a bare
     `pwsh <file>` is unreliable and an unsigned `.ps1` is otherwise blocked by
     execution policy.
   - `find_bin` miss → `Err("Interpreter not found on PATH: {interpreter}")`.
4. Call `run_cmd_with_stdin_env` with a `Duration::from_secs(60 * 15)` timeout,
   `Some(control)`, the `on_line` streamer, `last_output` as the stdin payload,
   and env `[("ALFRED_OUTPUT", last_output), ("ALFRED_CONTEXT", context),
   ("ALFRED_CWD", cwd_or_empty)]`.
5. Merge stdout and stderr the way `run_shell_command` already does and return
   `(text, exit_code)`.

### 2d. The `"script"` arm

Model it on the existing `"shell"` arm, with these differences:

- Empty inline body **and** empty path → `Err("Script node has no script")`.
- Stream lines via the same `step_log` emit pattern the agent arms use, so the
  run console shows progress instead of going silent for minutes.
- On success append under `## Script output` when `appendOutput` is set; set
  `last_output` to the text either way.
- Non-zero exit → `Err(format!("Script exited with status {code}\n{text}"))`.
- A cancelled run must surface the existing `"__cancelled__"` sentinel, matching
  how the `appAction` arm handles cancellation.

### 2e. The Input arm

In the `"prompt" | "input"` arm, after `format_attachments_context`:

1. Read `data.get("script")`. Absent or `source`-less → behave exactly as today.
2. Append `format_script_instruction(script)` after the attachments block.
3. If `script.run` is true, call `run_script`. On any outcome — including a
   non-zero exit or a spawn error — append

   ````
   ## Script output

   ```
   {text}
   (exit {code})
   ```
   ````

   and continue. Emit a `step_log` noting the exit code when it is non-zero.
   **Never** fail the step because of the Input node's script.

**Verify**: `cargo test --locked --manifest-path src-tauri/Cargo.toml` and
`cargo fmt --check`.

**STOP** if `RunControl` is not reachable from the Input arm the way it is from
the agent arms — report that instead of dropping cancellation support.

---

## Phase 3 — Script node UI (~45 min)

1. **`src/features/workflow/components/utility-nodes/utility-nodes.tsx`** — add
   `ScriptNode` beside `ShellNode`, reusing `SimpleStepNode` (which already
   renders both handles and the output preview):

```tsx
export function ScriptNode({
  id,
  data,
}: NodeProps<Node<ScriptNodeData, "script">>) {
  const body =
    data.source === "file"
      ? previewLine(data.path, "No script file")
      : previewLine(data.body, "Empty script");
  return (
    <SimpleStepNode
      id={id}
      className="wf-node-script"
      title={data.label || "Script"}
      body={body}
      meta={data.appendOutput ? "Append stdout to context" : "Run only"}
    />
  );
}
```

2. **`src/features/workflow/components/node-types/index.ts`** — import it and
   add `script: ScriptNode`. Leave `shell: ShellNode` in place.
3. **`src/App.css`** — beside `.wf-node-shell`:

```css
.wf-node-script {
  background: var(--shell);
  min-width: 180px;
}
```

4. **`src/features/workflow/components/node-settings-modal/utility-settings.tsx`**
   — add `ScriptSettings`: label field, a **Source** radio pair (File / Inline),
   then either a path input with a `Browse…` button (reuse
   `pickFileAttachments` from `../../attachments`) or a monospace body
   textarea; a **Run with** text input with `defaultInterpreter()` as its
   placeholder; and the **Append stdout to context** checkbox. Add a `hint`
   noting that stdin and `$ALFRED_OUTPUT` both carry the upstream output, and
   that a `#!` line overrides "Run with" (not on Windows).
5. **`node-settings-modal.tsx`** — wire the `script` case to `ScriptSettings`.
6. **`src/features/workflow/add-step-items.ts`** — in the `agent` group,
   replace the Shell entry with:

```ts
{ kind: "step", label: "Script", type: "script", data: defaultScriptNodeData() },
```

   Remove the now-unused `defaultShellNodeData` import if nothing else in the
   file uses it. Do **not** delete `defaultShellNodeData` from `types.ts`.

**Verify**: `bun run build:frontend`; open a workflow, add a Script step,
confirm both handles connect and that a saved graph with a `shell` node still
renders.

---

## Phase 4 — Input node script option (~1 h)

1. **`node-settings-modal.tsx`**, Input section — after the attachments block,
   add a **Script** group:
   - A three-way source control: `None` / `File` / `Inline`. `None` clears
     `script` to `undefined`; the others seed it from `defaultInputScript()`.
   - File mode: path input + `Browse…` (reuse `pickFileAttachments`).
   - Inline mode: monospace body textarea + the **Run with** input.
   - A **Message** textarea, prefilled with `DEFAULT_SCRIPT_MESSAGE`.
   - A **Run it before the agent** checkbox, default off, with a `hint`
     explaining that a failing script does not stop the run — its output is
     handed to the agent.
   - Every control must be disabled when `blocked` is set, matching how the
     prompt textarea and attachment buttons already behave.
2. **`prompt-node/prompt-node.tsx`** — when `data.script` is set, render one
   read-only chip below the attachment list showing the file basename or
   `Inline script`, plus a `Run` marker when `script.run` is true. Reuse the
   attachment-chip styling; add no new canvas controls, and do not change the
   node's resize bounds.

**Verify**: `bun run build:frontend`; set a script on an Input node, confirm the
chip appears, confirm every field is inert when the node is blocked, and confirm
the prompt sent to the agent ends with the instruction block.

---

## Phase 5 — Tests and gates (~30 min)

1. **`src-tauri/src/runner/mod.rs`**, in the existing `#[cfg(test)]` module:
   - `script_extension_maps_interpreters` — covers `bash`, `python3`, `node`,
     `pwsh`, and an unknown interpreter.
   - `resolve_script_path_uses_cwd` — relative path joins the cwd.
   - `resolve_script_path_errs_without_cwd` — relative path, no cwd → `Err`.
   - `resolve_script_path_passes_absolute` — absolute path unchanged.
   - `script_block_inlines_body` — inline source produces the message plus a
     fenced body.
   - `script_block_references_path` — file source produces the message plus a
     backticked path and **no** body.
2. **`tests/script-node.test.ts`** (new, matching the existing top-level test
   files):
   - `defaultScriptNodeData()` shape: `kind`, `source: "inline"`,
     `appendOutput: true`.
   - `defaultInputScript()` shape: `source: "file"`, `run: false`,
     `message === DEFAULT_SCRIPT_MESSAGE`.
   - `titleForNodeType("script") === "Script"`.
   - A legacy graph node with no `script` field still satisfies
     `isPromptNodeData`.
3. Run `bun run check` (frontend tests + build + `cargo test`), then
   `cargo fmt --check` and `git diff --check`.
4. Update this plan's **Implementation status** and the `plans/README.md` row.

**Do not** add a test that spawns a real process. It was considered and
rejected: it costs a CI spawn and needs a separate Windows variant.

---

## Verification checklist

- [ ] `bun run check` passes.
- [ ] `cargo fmt --check` and `git diff --check` pass.
- [ ] A workflow with an existing `shell` node still loads, renders, and runs.
- [ ] Shell no longer appears in any Add-step menu.
- [ ] Script node: inline `echo "$ALFRED_OUTPUT"` returns the upstream output.
- [ ] Script node: `exit 1` fails the step and stops the run.
- [ ] Script node: cancelling a `sleep 60` script stops it and reports cancelled.
- [ ] Input node, instruct-only: the prompt gains the block, nothing executes.
- [ ] Input node, run mode, `exit 1`: the agent still runs and sees the output.
- [ ] Both themes reviewed; no layout shift on the Input node when a script chip
      is present.
