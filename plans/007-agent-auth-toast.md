# Plan 007: Surface agent authentication failures with an actionable toast

> **Executor instructions**: Follow the steps in order and run each stated
> verification. Preserve concurrent user work, especially the recently added
> agent model/usage functionality. If a STOP condition occurs, report it rather
> than widening scope. After all implementation gates pass, change only Plan
> 007's row in `plans/README.md` from `TODO` to `DONE`.
>
> **Reviewed after model/usage changes**: 2026-08-11. The original plan was
> re-checked against the live workspace after `AgentUsageSnapshot`, usage
> loading, menu handling, and model-related UI changed. The implementation
> below deliberately does not reuse or modify that state.

## Drift gate

This workspace has no usable commit/`HEAD`. Before implementation, run:

```bash
sha256sum src-tauri/src/agents/mod.rs src-tauri/src/runner/mod.rs \
  src/features/workflow/types.ts src/features/workflow/store.ts \
  src/App.tsx tests/store.test.ts
```

Reviewed-at hashes:

```text
18db9525d53097b62daadc9aef682ec53b6d7cc495448608fe0e24ba448adf32  src-tauri/src/agents/mod.rs
a3d8f4cbf5bc44188ab56855e3d0bd4126fc12b2f1c5f636a524ca75dfaec17b  src-tauri/src/runner/mod.rs
382e1de8c5a3520d4e5be7308cc2338d592aa8f1374ba045d872195145c84565  src/features/workflow/types.ts
64cbeabc97e101bb22136f94b7eaa9451ce1c7f5395caf3768242a114af4ccc1  src/features/workflow/store.ts
586b9682aeb7bf9a145c472b12ee494fda88d99f35681d0ab48423593ce2a716  src/App.tsx
cd9b565e7b677e383ae0e062b375e099b3f40ba1ba221030b8d386e2a1689f62  tests/store.test.ts
```

If a hash differs, inspect the named integration seam before editing. Continue
when the drift is unrelated and can be preserved. Stop only if the built-in
agent error arm, `RunEvent`, `handleRunEvent`, or the single app-root mount point
has changed enough that the steps below are no longer valid.

`App.css` is intentionally excluded from the hash gate because concurrent
agent-usage styling is active there. Add the toast rules without rewriting or
reformatting existing usage styles.

## Status

- **Priority**: P1
- **Effort**: M
- **Risk**: MED
- **Depends on**: none
- **Category**: dx
- **Planned at**: uncommitted workspace with no `HEAD`, 2026-08-11

## Outcome

When a built-in agent CLI fails with a recognized authentication error, the
existing run still fails normally and retains the full error in run history.
In addition, the app shows one persistent in-app toast naming the provider and
showing the terminal login command. The user can copy the command or dismiss
the toast.

The app does not read, store, refresh, or submit provider credentials, and it
does not launch an interactive login process. If an error is not confidently
recognized as authentication-related, the existing generic failure flow is
left unchanged.

## Current state and protected work

- `src-tauri/src/agents/mod.rs` exposes four built-in `AgentProvider` values and
  free-form `AgentError::Message` failures. It now also exports
  `list_provider_usage` and `AgentUsageSnapshot`; preserve those exports.
- `src-tauri/src/agents/usage.rs` and the corresponding command/API/store/UI
  additions are separate provider-usage work. `AgentUsageSnapshot.connected`
  is not an auth signal: the probes mix installation, timeout, and provider
  behavior, and OpenCode currently treats an installed binary as connected.
- `src-tauri/src/runner/mod.rs` knows the provider only inside the `"agent"`
  match arm. The outer generic error branch receives only a `String`, emits
  `step_failed`, then emits the run-level `failed` event.
- `src/features/workflow/store.ts` records `step_failed`; its run-level
  `failed` branch also queues the existing OS notification. Run-event errors
  are not written to the workflow toolbar's `.error` field.
- `src/App.tsx` contains current native-menu and selection handling and returns
  one `<WorkflowCanvas />`. Preserve those handlers and mount the toast next to
  that canvas.
- Bun tests directly exercise Zustand stores and have no DOM/component test
  environment. Keep automated toast tests store-level; verify clipboard UI
  manually.

## Provider contract

The toast command is backend-owned so the frontend does not duplicate provider
knowledge:

| Provider | Label | Login command |
| --- | --- | --- |
| `claude_code` | Claude Code | `claude auth login` |
| `cursor` | Cursor | `cursor-agent login`, or `agent login` when only the legacy binary is installed |
| `codex` | Codex | `codex login` |
| `opencode` | OpenCode | `opencode auth login` |

Normalize an error by lowercasing it and joining whitespace before matching.
Use exactly these conservative positive rules:

| Provider | Recognized normalized signatures |
| --- | --- |
| Claude Code | contains `failed to authenticate`; or contains `oauth session expired`; or contains `oauth` plus `could not be refreshed`; or contains `oauth`, `refresh token`, and either `expired` or `invalid` |
| Cursor | contains `press any key to sign in`, `not authenticated`, `not logged in`, or `authentication required` |
| Codex | contains `not logged in`, `login required`, or `authentication required`; or contains both `401` and `unauthorized`; or contains `api key` plus one of `missing`, `invalid`, or `required` |
| OpenCode | the same high-confidence rules as Codex |

Do not classify CLI-not-found, timeout, network, rate-limit, unavailable-model,
permission-denied, unknown-option, or cancellation errors unless one of the
provider-specific positive rules above is also present. Custom-agent nodes
never call this classifier.

## Scope

Implementation may modify only:

- `src-tauri/src/agents/auth.rs` — new structured hint, command resolver,
  classifier, and Rust unit tests.
- `src-tauri/src/agents/mod.rs` — declare/re-export the auth module while
  preserving all model and usage exports.
- `src-tauri/src/runner/mod.rs` — propagate an optional hint on `step_failed`.
- `src/features/workflow/types.ts` — mirror the event payload.
- `src/features/workflow/store.ts` — translate the event into toast state.
- `src/components/toast/toast-store.ts` — new auth-toast Zustand store.
- `src/components/toast/toast.tsx` — new accessible viewport/item UI.
- `src/components/toast/index.ts` — exports.
- `src/App.tsx` — mount the viewport once without replacing current handlers.
- `src/App.css` — toast styling.
- `tests/toast.test.ts` — new toast-store tests.
- `tests/store.test.ts` — event-to-toast integration tests.
- `plans/README.md` — only the Plan 007 status cell, and only after completion.

Do not modify provider credential storage, `src-tauri/src/agents/usage.rs`, the
usage command/API/UI, database schemas, existing OS notifications, activity
panel behavior, provider adapters, package dependencies, Tauri plugins, or
custom-agent behavior. Do not start a login subprocess.

## Steps

### 1. Add a provider-neutral auth classifier

Create `src-tauri/src/agents/auth.rs` with:

- `AgentAuthRequired`, deriving `Debug`, `Clone`, `Serialize`, `Deserialize`,
  `PartialEq`, and `Eq`, and using `#[serde(rename_all = "camelCase")]`.
- Fields `provider: AgentProvider`, `label: String`, and
  `login_command: String`. Do not include the raw CLI error.
- `auth_required(provider: AgentProvider, message: &str) ->
  Option<AgentAuthRequired>` implementing exactly the normalization and
  matching table above.
- One label/command resolver. For Cursor, use
  `super::process::find_bin("cursor-agent")`; return `cursor-agent login` when
  found and `agent login` otherwise. This mirrors the current adapter's binary
  resolution. Keep the boolean choice in a small pure helper so both Cursor
  branches are deterministic in unit tests.

In `agents/mod.rs`, add the module and re-export only the values the runner
needs. Do not move the classifier into this hot module and do not remove or
reorder the new `usage` exports unnecessarily.

Unit tests in `auth.rs` must cover every positive rule family, mixed
case/whitespace, both Cursor command aliases, and false positives for normal
failure, network, rate-limit, unavailable model, permission denied,
CLI-not-found, and cancellation text.

Verify:

```bash
cargo test --manifest-path src-tauri/Cargo.toml agents::auth::tests
```

Expected: the focused classifier tests pass. The full Rust suite is a later
gate; do not claim this filtered command ran unrelated tests.

### 2. Preserve provider context and extend `RunEvent`

In `runner/mod.rs`, add:

```rust
#[serde(default, skip_serializing_if = "Option::is_none")]
pub auth_required: Option<AgentAuthRequired>,
```

to `RunEvent`. Because the struct already uses camel-case serialization, the
wire field is `authRequired`. Add `auth_required: None` to every existing event
literal.

The provider is out of scope in the outer `Err(err)` branch, so propagate it
without changing the result type used by every node arm:

1. Immediately before `let step_result = match node_type.as_str()`, declare
   `let mut step_auth_required: Option<AgentAuthRequired> = None;`.
2. In the built-in `"agent"` arm's `Err(e)` case, while `provider` is still in
   scope, stringify once, assign the classifier result to the sidecar, and
   return the same string error:

   ```rust
   Err(e) => {
       let message = e.to_string();
       step_auth_required = auth_required(provider, &message);
       Err(message)
   }
   ```

3. In the outer generic error branch, attach
   `step_auth_required.take()` only to the `step_failed` event. Leave the
   subsequent run-level `failed` event at `None`.

This keeps custom agents and all non-agent nodes generic, avoids reparsing node
data, and emits exactly one auth-bearing event.

In `types.ts`, add `AgentAuthRequired` with `provider: AgentProviderId`,
`label: string`, and `loginCommand: string`; add
`authRequired?: AgentAuthRequired` to `RunEvent`. Only this new field is
optional because it is omitted from ordinary serialized events; do not loosen
the existing required `message`, `at`, `runId`, or `workflowId` fields.

Verify:

```bash
cargo test --manifest-path src-tauri/Cargo.toml
bun run build:frontend
```

### 3. Create a persistent, deduplicated auth-toast store and UI

Create `toast-store.ts` as a small Zustand store independent of the OS
notification store. Its public behavior is exact:

- `showAgentAuthToast(authRequired, workflowName?)` creates ID
  `agent-auth:<provider>`.
- A duplicate provider replaces that record in place with the latest label,
  command, and workflow name; it never adds another record.
- Different built-in providers may coexist, so the natural maximum is four.
- Auth toasts do not auto-expire. They remain until explicit dismissal because
  the user must leave the app, authenticate in a terminal/browser, and return.
- `dismissToast(id)` removes the record.
- The store never receives or retains the raw CLI error.

Create `toast.tsx` and `index.ts`. Mount an `aria-live="assertive"` viewport and
render each card with `role="alert"`, provider title, concise instructions, the
login command, `Copy command`, and an accessible dismiss button. Do not move
focus when a toast appears.

Use `navigator.clipboard.writeText`. If it is missing or rejects, keep the
command visible and show `Select and copy manually.` Do not log the error and
do not add a clipboard plugin. Give the command container the existing
`.user-select-text` class so `App.tsx` does not suppress selection. Automated
tests do not need to render this component because no DOM harness exists;
TypeScript build plus the manual smoke test covers it.

Add `tests/toast.test.ts` using the current direct-Zustand Bun style. Reset the
store between tests and assert creation, same-provider replacement without
growth, coexistence across providers, and dismissal. There are no timer tests.

Verify:

```bash
bun test tests/toast.test.ts
bun run build:frontend
```

### 4. Connect only `step_failed` auth events to the toast

In `handleRunEvent`, import the toast store directly. When
`event.kind === "step_failed"` and `event.authRequired` exists, resolve the
workflow name from the current workflow list with `Workflow` as fallback and
call `showAgentAuthToast`. Perform this once outside the workflow store's
`set(...)` callback; the toast store handles provider deduplication.

Do not wire the toast from the final `failed` branch. That branch must continue
to update run status and queue the existing OS notification unchanged. Keeping
the auth trigger on `step_failed` also makes the Bun integration test
independent of `notifications.ts`, whose browser fallback expects `document`.

Extend `tests/store.test.ts` with:

- a `step_failed` event containing `authRequired` creates exactly one expected
  auth toast and still records the failed step/log;
- a generic `step_failed` event creates no toast;
- repeating the same provider does not grow the toast list.

Reset both Zustand stores between tests. Do not invoke a final `failed` event
merely to test the toast, and do not mock the OS notification subsystem.

Verify:

```bash
bun test tests/store.test.ts tests/toast.test.ts
```

### 5. Mount and style the viewport

In `App.tsx`, preserve all current effects/menu callbacks and change only the
root return so `<WorkflowCanvas />` and one `<ToastViewport />` are siblings.

In `App.css`, use existing color/shadow variables. Position the viewport at the
top-right below native title-bar controls, make its width responsive, and use
an explicit `z-index: 130` so it stays above current menus/modals and the
workflow drag ghost. Ensure buttons have visible `:focus-visible` states and
the command is selectable. Static appearance/disappearance is acceptable; no
animation or new dependency is required.

Verify:

```bash
bun run build:frontend
```

Manual Tauri smoke test when a logged-out built-in CLI is safely available:

1. Run `bun run dev` and execute a workflow with that provider.
2. Confirm one toast appears, the activity panel still contains the full error,
   and the run finishes as failed.
3. Confirm copy works; if clipboard access is unavailable, the manual-copy
   message appears and the command remains selectable.
4. Open Settings and return to the canvas; the root-level toast remains.
5. Dismiss it, then retry after authenticating.

Do not log out a real user or alter provider credentials solely for this smoke
test. If no provider is already logged out, record the manual smoke as not
runnable; the classifier and store tests remain the required gates.

### 6. Run final gates and update the plan index

Run:

```bash
bun test
bun run build:frontend
cargo test --manifest-path src-tauri/Cargo.toml
rustfmt --edition 2021 --check --config skip_children=true \
  src-tauri/src/agents/auth.rs src-tauri/src/agents/mod.rs \
  src-tauri/src/runner/mod.rs
rg -n '[ \t]+$' src-tauri/src/agents/auth.rs \
  src-tauri/src/agents/mod.rs src-tauri/src/runner/mod.rs \
  src/features/workflow/types.ts src/features/workflow/store.ts \
  src/components/toast/toast-store.ts src/components/toast/toast.tsx \
  src/components/toast/index.ts src/App.tsx src/App.css \
  tests/toast.test.ts tests/store.test.ts
git status --short
```

Expected: tests/build/formatting pass, and `rg` prints no trailing-whitespace
matches. `git status` is context only: because this workspace has no `HEAD` and
already contains staged/untracked user work, it cannot prove feature scope.
Review the implementation against the explicit Scope list and the initial
hashes instead. Do not use a successful `git diff --check` as evidence that it
checked untracked toast files.

After the gates pass, change only Plan 007's status cell in `plans/README.md` to
`DONE`. Do not commit or push unless separately requested.

## Done criteria

- [ ] Recognized built-in auth failures produce a structured provider, label,
  and login command; conservative negative cases remain generic.
- [ ] Auth metadata is omitted from ordinary events and appears only on the
  relevant built-in agent `step_failed` event.
- [ ] One persistent, accessible toast appears per provider with copy and
  dismiss actions; duplicate failures replace rather than stack.
- [ ] Full CLI errors remain in existing run logs/history and never enter toast
  state.
- [ ] Existing run failure state and OS notifications remain unchanged.
- [ ] Current model discovery, provider usage, menu, and selection behavior are
  preserved.
- [ ] `bun test`, `bun run build:frontend`, the full Rust suite, and scoped
  rustfmt all exit 0.
- [ ] The scoped trailing-whitespace check prints no matches.
- [ ] Only Plan 007's index status is changed after implementation succeeds.

## STOP conditions

Stop and report instead of improvising if:

- Relevant drift invalidates the agent-side sidecar, `RunEvent`, workflow event
  handler, or root mount strategy described above.
- Implementing this requires reading/storing credentials, starting a login
  process, adding a Tauri/plugin/package dependency, or changing a database or
  external API contract.
- A required change falls outside the Scope list or conflicts with concurrent
  model/usage work and cannot be preserved cleanly.
- A required test/build command still fails after one reasonable scoped fix and
  the failure is caused by code outside this feature.

## Maintenance notes

- Provider auth wording is an external CLI contract. Add newly observed exact
  signatures to `auth.rs` with a positive and false-positive test; do not broaden
  to generic words such as `authentication`, `permission`, or `token` alone.
- Keep provider labels, commands, and matching rules in the backend. The
  renderer should consume structured data and never parse raw CLI output.
- Direct in-app reauthentication is a separate security/process feature. This
  toast intentionally tells the user what to run in their terminal.
