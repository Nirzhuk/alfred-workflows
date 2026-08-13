# Plan 002: Enforce fair Free and Pro entitlements

> **Executor instructions**: Complete Plan 001 first. Follow every step and
> verification gate below. Stop on any STOP condition rather than adding a
> UI-only workaround. When done, update this plan's row in
> `plans/README.md`.
>
> **Drift check (run first)**: this plan was written against an unversioned
> workspace. Run `git rev-parse --short HEAD`. If it fails, compare the hashes
> and excerpts below to the live files and inspect Plan 001's resulting
> licensing interface. If Git now exists, record HEAD and inspect every
> in-scope path. Semantic drift is a STOP condition.

## Status

- **Priority**: P1
- **Effort**: L
- **Risk**: HIGH
- **Depends on**: `plans/001-polar-offline-licensing.md`
- **Category**: direction, architecture, correctness
- **Planned at**: unversioned workspace snapshot, 2026-08-09

## Why this matters

Freemium limits must be enforced at trusted Tauri command and runtime
boundaries, not only by disabled React buttons. At the same time, a downgrade
must never delete local customer data. This plan centralizes the Free policy,
allows three runnable workflows and 25 owned memories, and allows one live
automation while keeping manual runs and core local features unlimited.

## Current state

- `commands::create_workflow` directly calls `db.create_workflow` with no
  entitlement check (`src-tauri/src/commands/mod.rs:22`).
- `commands::run_workflow` starts any selected workflow
  (`src-tauri/src/commands/mod.rs:69`).
- `commands::create_memory` directly inserts any memory
  (`src-tauri/src/commands/mod.rs:217`).
- Schedule and trigger upserts accept `enabled: true` without a shared global
  automation count (`commands/mod.rs:97` and `:131`).
- `scheduler::tick` consumes every enabled schedule, while trigger reload/fire
  paths consume enabled triggers independently. A UI gate alone would allow
  downgraded automations to continue running in the tray.
- Memories are owned rows in `memories`; cross-workflow uses are separate rows
  in `memory_links`. Therefore the 25-memory limit must count `memories`, not
  the current workflow's combined owned-and-linked list.

Observed workflow creation (`src-tauri/src/db/workflows.rs:94`):

```rust
pub fn create_workflow(&self, input: CreateWorkflowInput) -> Result<Workflow, DbError> {
    let id = Uuid::new_v4().to_string();
    // ... INSERT INTO workflows ...
}
```

Observed memory insertion (`src-tauri/src/db/memories.rs:232`):

```rust
conn.execute(
    "INSERT INTO memories
     (id, workflow_id, run_id, node_id, kind, source, title, body,
      artifact_path, pinned, created_at, updated_at)
     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?11)",
    params![/* ... */],
)?;
```

Selected pre-Plan-001 snapshot hashes:

```text
737dc0527f1fea123a4b93d609c3c1a78836503531abe9386a57f81d20f435fd  src-tauri/src/db/workflows.rs
02bff4e4c284b8a5b3c17e241152131f31236c4d3cbf26b00db20b6ffe124993  src-tauri/src/db/memories.rs
54c26180b44bd470a99406dfad3d1ca74823cb4de555f64cdd27c62833586950  src-tauri/src/db/schedules.rs
10966520e2492776e1c25c044a50337c591b87d082f35f40a620904530504361  src-tauri/src/db/triggers.rs
```

Plan 001 is expected to change `commands/mod.rs`, `lib.rs`, `db/mod.rs`, and
`licensing/mod.rs`; reconcile those changes rather than restoring these hashes.

## Fixed entitlement policy

```text
FREE_WORKFLOW_LIMIT = 3
FREE_MEMORY_LIMIT = 25
FREE_ACTIVE_AUTOMATION_LIMIT = 1
```

- Pro, including valid offline-grace Pro, bypasses these limits.
- Manual runs are unlimited, but when a downgraded user owns more than three
  workflows, only three selected Free slots may run.
- All workflows remain visible, editable, saveable, deletable, and available
  for future export. Over-limit workflows are not deleted.
- Existing memories over 25 remain readable, editable, linkable, pinnable, and
  deletable. Only creation of another owned memory is blocked.
- A linked memory consumes no additional memory slot.
- Disabled schedules/triggers consume no automation slot.
- On downgrade with multiple enabled automations, execute only one deterministic
  oldest enabled automation across both schedules and triggers. Leave the rest
  stored and enabled-but-plan-paused; do not delete or silently rewrite them.
- Trigger test runs are manual diagnostics and remain allowed even when the
  trigger is plan-paused.

## Commands you will need

| Purpose | Command | Expected on success |
|---------|---------|---------------------|
| Rust check | `cargo check --manifest-path src-tauri/Cargo.toml` | exit 0 |
| Policy tests | `cargo test --manifest-path src-tauri/Cargo.toml licensing::policy` | all policy tests pass |
| Full Rust tests | `cargo test --manifest-path src-tauri/Cargo.toml` | all tests pass |
| Frontend build | `bun run build:frontend` | exit 0 |

## Scope

**In scope**:

- `src-tauri/src/licensing/mod.rs`
- `src-tauri/src/licensing/policy.rs` (new)
- `src-tauri/src/db/mod.rs`
- `src-tauri/src/db/schema.sql`
- `src-tauri/src/db/entitlements.rs` (new)
- `src-tauri/src/db/workflows.rs`
- `src-tauri/src/db/memories.rs`
- `src-tauri/src/db/schedules.rs`
- `src-tauri/src/db/triggers.rs`
- `src-tauri/src/commands/mod.rs`
- `src-tauri/src/lib.rs`
- `src-tauri/src/runner/mod.rs`
- `src-tauri/src/scheduler/mod.rs`
- `src-tauri/src/triggers/mod.rs`
- `src-tauri/src/triggers/file.rs`
- `src-tauri/src/triggers/http.rs`
- `src/features/licensing/types.ts`
- `src/features/licensing/api.ts`
- `plans/README.md` and this plan's status only

**Out of scope**:

- React upgrade dialogs, badges, usage meters, and settings; Plan 003 owns them.
- Limiting providers, models, skills, nodes, manual-run count, or local history.
- Deleting or archiving customer data automatically.
- Team seats, cloud sync, analytics, and account authentication.
- Changing the 7/30-day offline policy from Plan 001.

## Git workflow

- Do not initialize Git in the current unversioned workspace.
- If Git exists at execution time, use branch
  `advisor/002-freemium-entitlement-enforcement`.
- Do not push or open a PR unless instructed.

## Steps

### Step 1: Centralize limits and structured entitlement errors

Create `licensing/policy.rs` with the three constants above and pure decisions
that consume Plan 001's provider-neutral `LicenseSnapshot`. Do not scatter
numeric literals through commands or UI responses.

Define a serializable entitlement error containing:

- stable `code`: `workflow_limit`, `workflow_not_in_free_slots`,
  `memory_limit`, or `automation_limit`;
- human-readable message;
- `limit` and `used`;
- `upgradeRequired: true`.

Return this structured error from gated Tauri commands. Database and validation
failures remain operational errors and must not be mislabeled as paywalls.

**Verify**:

```bash
cargo test --manifest-path src-tauri/Cargo.toml licensing::policy
```

Expected: constant and decision-table tests pass.

### Step 2: Add usage queries and persistent Free workflow slots

Add a `free_workflow_slots` table with `workflow_id` as a foreign key and a
selection timestamp. Use `CREATE TABLE IF NOT EXISTS` in `schema.sql`; do not
delete slots when switching to Pro. Cascade slot deletion when its workflow is
deleted.

Create `db/entitlements.rs` with methods to:

- count all saved workflows;
- count all owned memory rows globally;
- count enabled schedules plus enabled triggers globally;
- ensure up to three Free workflow slots exist, filling missing slots from the
  most recently updated workflows deterministically;
- atomically replace the selected Free workflow IDs, validating that every ID
  exists and no more than three are supplied;
- determine whether a workflow is runnable under Free;
- return a frontend-safe `EntitlementUsage` containing counts, limits,
  `freeWorkflowIds`, and `planPausedAutomationIds`;
- choose the one runnable Free automation from a union of enabled schedules and
  triggers ordered by `created_at`, with ID as a tie-breaker.

Do not count `memory_links`. Do not mutate enabled flags when determining the
effective automation.

**Verify**:

```bash
cargo test --manifest-path src-tauri/Cargo.toml db::entitlements
```

Expected: in-memory database tests cover counts, slot selection, links, and the
shared automation ordering.

### Step 3: Gate workflow creation and execution

At `commands::create_workflow`:

- evaluate the local license snapshot only; no network request;
- if Free and the total saved workflow count is already 3, return
  `workflow_limit`;
- after successful Free creation, place the workflow in a Free slot.

At `commands::run_workflow`:

- Pro may run any saved workflow;
- Free with three or fewer workflows may run any of them;
- Free with more than three (a downgrade case) may run only selected Free
  workflow slots;
- manual run count remains unlimited.

Add commands to get entitlement usage and atomically select up to three Free
workflow IDs. Selection changes must never alter workflow data.

**Verify**:

```bash
cargo test --manifest-path src-tauri/Cargo.toml licensing::policy workflow
```

Expected: the fourth Free creation is rejected, Pro bypasses the cap, and a
downgraded over-limit workflow cannot run until selected.

### Step 4: Gate creation of the 26th owned memory

At the backend `create_memory` boundary, block a new owned memory when a Free
user already owns 25. Keep update, delete, clear, link, unlink, and pin paths
available.

Preserve legacy data migration: an import with an explicit legacy ID that does
not already exist may be allowed to migrate even above the limit, because
silently losing pre-existing local data is worse than a one-time over-limit
state. Document and test this narrow exception. Do not exempt ordinary manual
or run-output creation.

**Verify**:

```bash
cargo test --manifest-path src-tauri/Cargo.toml licensing::policy memory
```

Expected: memory 25 succeeds, memory 26 fails for Free, Pro succeeds, linking
does not change usage, and legacy migration does not lose existing data.

### Step 5: Enforce one live automation across schedules and triggers

When enabling a new or currently disabled schedule/trigger under Free, reject
the operation if another enabled automation already occupies the one slot.
Editing an already enabled automation remains allowed. Saving disabled
automation configurations is allowed because they do not run.

Also enforce effective access in runtime paths so downgrade cannot be bypassed:

- `scheduler::tick` must skip plan-paused schedules;
- file watcher reload must bind only effectively allowed triggers;
- webhook fire must reject a plan-paused live trigger;
- trigger test runs must use an explicit test/manual mode that bypasses the
  live-automation slot without enabling the trigger;
- after license activation, refresh, deactivation, or Free slot changes,
  refresh trigger bindings and tray state.

If more than one automation remains enabled after downgrade, the oldest one
selected by Step 2 runs; the others remain stored and surface as plan-paused in
the usage response.

**Verify**:

```bash
cargo test --manifest-path src-tauri/Cargo.toml licensing::policy automation
```

Expected: schedule + trigger share one Free slot, only the deterministic winner
runs after downgrade, and manual trigger testing remains available.

### Step 6: Expose typed usage to the frontend

Extend `src/features/licensing/types.ts` and `api.ts` with:

- `EntitlementUsage`;
- limit error shape and a safe type guard/parser;
- `getEntitlementUsage()`;
- `setFreeWorkflowSlots(ids)`.

Do not implement presentation logic here. Unknown errors must remain ordinary
operational errors rather than being converted to upgrade prompts.

**Verify**:

```bash
bun run build:frontend
cargo test --manifest-path src-tauri/Cargo.toml
```

Expected: both commands exit 0.

## Test plan

- Add pure policy tests beside `licensing/policy.rs` for Free, online Pro,
  offline-grace Pro, and needs-refresh Free.
- Add in-memory SQLite tests beside `db/entitlements.rs`. If Plan 001 did not
  provide an in-memory `Db` constructor, add one under `#[cfg(test)]` in
  `db/mod.rs`; do not create a production alternate database path.
- Cover exact boundaries: workflows 3/4, memories 25/26, automations 1/2.
- Cover cross-type automation ordering: schedule versus file trigger and
  schedule versus webhook.
- Cover downgrade without mutation: enabled rows and all customer data remain
  in SQLite even when execution is plan-paused.
- Cover the narrow legacy-memory migration exception.
- Model module-local test layout after `triggers/http.rs`.

## Done criteria

- [ ] `cargo test --manifest-path src-tauri/Cargo.toml` exits 0.
- [ ] `bun run build:frontend` exits 0.
- [ ] Free creation stops at 3 workflows and 25 owned memories.
- [ ] Free has one active automation shared across all automation types.
- [ ] Manual runs remain unlimited for the three selected Free workflows.
- [ ] Provider, model, skill, node, and run-history paths contain no new gate.
- [ ] Every backend gate uses the same offline-aware local license snapshot.
- [ ] Downgrade does not delete or rewrite workflows, memories, schedules, or
      triggers.
- [ ] UI-only bypasses cannot create a fourth workflow, a 26th ordinary memory,
      or a second live Free automation.
- [ ] No files outside scope are modified.
- [ ] `plans/README.md` marks Plan 002 `DONE`.

## STOP conditions

Stop and report if:

- Plan 001 does not expose a synchronous/local effective entitlement without
  network I/O.
- Enforcing a limit requires deleting, truncating, or rewriting customer data.
- Trigger and schedule runtimes cannot share one deterministic policy without
  a larger runner redesign.
- Existing memory import behavior cannot distinguish legacy restoration from
  normal creation.
- Source has drifted such that the named command/runtime boundaries no longer
  own these operations.
- A verification command fails twice after a reasonable scoped correction.

## Maintenance notes

- Reviewers should attempt Tauri-command bypasses, not only click through the
  UI. Limits are correct only when backend commands reject invalid operations.
- Any future automation source must participate in the shared active-automation
  query and runtime policy.
- Any future bulk import/restore feature needs an explicit data-restoration
  policy; do not casually reuse the legacy migration exemption.
- Keep constants provider-neutral so changing Polar later does not alter the
  Free/Pro product definition.

