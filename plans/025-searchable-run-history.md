# Plan 025: Make run history and saved memories searchable

> **Executor instructions**: Follow this plan step by step. Run every
> verification command and confirm the expected result before moving to the
> next step. If anything in the "STOP conditions" section occurs, stop and
> report; do not improvise. When done, update this plan's row in
> `plans/README.md`.
>
> **Drift check (run first)**:
> `git diff --stat 62ff2bf..HEAD -- src-tauri/Cargo.toml src-tauri/src/db/schema.sql src-tauri/src/db/migrate.rs src-tauri/src/db/mod.rs src-tauri/src/db/history.rs src-tauri/src/runner/mod.rs src-tauri/src/commands/mod.rs src-tauri/src/lib.rs src/features/workflow/types.ts src/features/workflow/api.ts src/features/workflow/components/sidebar-nav/sidebar-nav.tsx src/features/workflow/components/workflow-canvas/workflow-canvas.tsx src/features/workflow/components/history-page src/App.css tests`
>
> The plan was written while the repository had an unrelated, uncommitted
> Connected Apps batch. `src-tauri/src/runner/mod.rs`, `src-tauri/src/lib.rs`,
> `src/App.css`, `plans/README.md`, and several integration tests were already
> modified. Preserve those changes. Re-read every overlapping symbol before
> editing.

## Status

- **Priority**: P0
- **Effort**: L (3–5 days)
- **Risk**: MED — additive indexing is low-risk, but migration/backfill and a
  new history UI touch persisted user data and the main navigation
- **Depends on**: none
- **Category**: direction / data
- **Planned at**: commit `62ff2bf`, 2026-08-18, with a dirty worktree

## Why this matters

Alfred persists runs and step payloads, but neither users nor agents can find
an old decision or output without reopening the live run that produced it.
This plan makes the existing SQLite data useful as episodic memory: exact
history remains the source of truth, while FTS5 supplies fast, local,
deterministic search without an LLM call or network service. It also creates
the lexical retrieval primitive required by Plans 027 and 028.

## Product decisions

- Raw `runs`, `run_steps`, and `memories` rows remain canonical. The FTS tables
  are disposable indexes and must be rebuildable.
- Search is local-only. No query, prompt, output, or snippet leaves the machine.
- This plan indexes text already stored in SQLite. It does not ingest live
  console activity, filesystem contents, connected-app credentials, or OS
  credential-store data.
- Search results are bounded text previews rendered as text, never HTML.
- Empty-query history browsing and non-empty-query search are separate API
  paths. Do not simulate browsing with an FTS wildcard.
- FTS syntax is not exposed to users. Convert plain text into a safely quoted
  prefix query so punctuation cannot produce `MATCH` parse errors.
- Large memory artifacts index the existing SQLite preview, not the full
  spilled file. Loading full artifact text remains an explicit memory action.
- Deleting a workflow/run/memory must remove its search documents. Index
  repair must be idempotent and safe to run at startup.

## Current state

- `src-tauri/src/db/schema.sql:34-59` defines `runs` and `run_steps`.
  `run_steps.input_json` and `output_json` hold the durable step payloads, but
  there is only an index on `run_id`.
- `src-tauri/src/db/schema.sql:87-108` defines workflow-owned `memories` and
  cross-workflow `memory_links`; there is no text index.
- `src-tauri/src/runner/mod.rs:473-519` serializes each completed step into
  `run_steps` through `insert_step`:

  ```rust
  let input_json = serde_json::to_string(input).unwrap_or_else(|_| "{}".into());
  let output_json = serde_json::to_string(output).unwrap_or_else(|_| "{}".into());
  conn.execute("INSERT INTO run_steps ...", rusqlite::params![...])?;
  ```

- `src-tauri/src/runner/mod.rs:51-57` exposes only the pending-run summary:

  ```rust
  pub struct RunSummary {
      pub id: String,
      pub workflow_id: String,
      pub trigger: String,
      pub status: String,
      pub created_at: String,
  }
  ```

- `src-tauri/src/commands/mod.rs:120-150` can start/cancel/list active runs,
  but has no persisted-history command.
- `src/features/workflow/components/sidebar-nav/sidebar-nav.tsx:3` has only
  `canvas | schedules | settings`. Follow its `NavButton` pattern for the new
  History destination.
- `src/features/workflow/components/workflow-canvas/workflow-canvas.tsx:210`
  owns the selected `SidebarView` and renders `SchedulesPage`/`SettingsPage`.
  Follow that routing pattern; do not introduce React Router.
- Rust DB tests are colocated under `#[cfg(test)]`; frontend behavior tests use
  Bun's `describe`, `test`, and `expect` from `bun:test`.

## Commands you will need

| Purpose | Command | Expected on success |
|---|---|---|
| Frontend tests | `bun test` | all tests pass |
| Frontend type/build | `bun run build:frontend` | exit 0 |
| Focused Rust DB tests | `cargo test --locked --manifest-path src-tauri/Cargo.toml db::history` | all history tests pass |
| Migration tests | `cargo test --locked --manifest-path src-tauri/Cargo.toml db::migrate` | all migration tests pass |
| Full gate | `bun run check` | exit 0 in a normal development/CI host |
| Diff hygiene | `git diff --check` | no output, exit 0 |

Baseline note from 2026-08-18: `bun run build:frontend` passed. The dirty
worktree's `bun test` had two unrelated Connected Apps copy-assertion failures.
Sandboxed Rust tests that bind loopback fixture servers failed with
`PermissionDenied`, while 133 non-network tests passed. Do not weaken or skip
those tests; use the focused commands above locally and require the full gate
on a normal host before marking DONE.

## Scope

**In scope**:

- `src-tauri/Cargo.toml` only if the bundled SQLite build does not expose FTS5
- `src-tauri/src/db/schema.sql`
- `src-tauri/src/db/migrate.rs`
- `src-tauri/src/db/mod.rs`
- `src-tauri/src/db/history.rs` (new)
- `src-tauri/src/runner/mod.rs`
- `src-tauri/src/commands/mod.rs`
- `src-tauri/src/lib.rs`
- `src/features/workflow/types.ts`
- `src/features/workflow/api.ts`
- `src/features/workflow/components/history-page/` (new)
- `src/features/workflow/components/sidebar-nav/sidebar-nav.tsx`
- `src/features/workflow/components/workflow-canvas/workflow-canvas.tsx`
- `src/App.css`
- `tests/history-search.test.ts` (new, for pure frontend contracts)
- `specs.md` and `docs/install.md` for the user-visible local-history behavior

**Out of scope**:

- Embeddings, vector databases, reranking models, remote search services
- Automatic prompt injection of search hits (Plan 027)
- Memory scopes or semantic memory kinds (Plan 026)
- Background extraction or summarization (Plan 028)
- Deleting, pruning, exporting, or syncing run history
- Indexing raw credential-store values, environment dumps, filesystem files,
  or transient `run://event` console rows
- Adding a general command palette or React Router

## Git workflow

- Branch: `advisor/025-searchable-run-history`
- Keep commits logical and imperative, for example `Index persisted run history`
  and `Add local history search`.
- Do not push or open a pull request unless instructed.

## Steps

### Step 1: Prove FTS5 availability and define the index schema

1. Add a focused migration test that opens the same in-memory schema as `Db`
   and executes:

   ```sql
   SELECT sqlite_compileoption_used('ENABLE_FTS5');
   ```

   Assert the result is `1`, then create and query a small temporary FTS5 table.
2. If the assertion fails, inspect `rusqlite`'s enabled bundled features and
   make the smallest `Cargo.toml` feature change that enables FTS5. Do not add
   a dynamic SQLite extension or runtime download.
3. Add these disposable indexes in `schema.sql` and equivalent idempotent
   creation in `migrate.rs`:

   ```sql
   CREATE VIRTUAL TABLE IF NOT EXISTS memory_fts USING fts5(
     memory_id UNINDEXED,
     workflow_id UNINDEXED,
     title,
     body,
     tokenize = 'unicode61 remove_diacritics 2'
   );

   CREATE VIRTUAL TABLE IF NOT EXISTS run_step_fts USING fts5(
     step_id UNINDEXED,
     run_id UNINDEXED,
     workflow_id UNINDEXED,
     node_id UNINDEXED,
     input_text,
     output_text,
     error_text,
     tokenize = 'unicode61 remove_diacritics 2'
   );
   ```

   Use standalone/contentless indexes rather than FTS external-content mode:
   memory artifacts may have a preview instead of their full body, and
   `run_steps` stores JSON rather than ready-to-index columns.

**Verify**:
`cargo test --locked --manifest-path src-tauri/Cargo.toml db::migrate::tests::initializes_search_indexes`
→ one passing test.

### Step 2: Add bounded text extraction and transactional index maintenance

Create `src-tauri/src/db/history.rs` and export it from `db/mod.rs`.

1. Implement `searchable_json_text(value: &Value, max_bytes: usize) -> String`.
   Walk JSON recursively, collect string/number/boolean leaves, omit object
   keys, preserve array order, remove control characters except newline/tab,
   and truncate on a UTF-8 boundary. Use 32 KiB per input/output side.
2. Implement internal helpers that use an existing `Connection`/transaction:

   - `index_run_step(conn, step_id, run_id, workflow_id, node_id, input,
     output, error)`
   - `index_memory(conn, memory_id)`; load the current SQLite title/body preview
   - `delete_run_step_index(conn, step_id)`
   - `delete_memory_index(conn, memory_id)`

3. Change `runner::insert_step` so the `run_steps` insert and FTS insert happen
   in one transaction. Query the run's `workflow_id` inside that transaction.
   A failed index write must roll back the step insert; do not silently create
   drift.
4. Update `create_memory`, `update_memory`, `delete_memory`, `clear_memories`,
   workflow deletion, and run deletion paths to maintain their indexes in the
   same transaction. Where a bulk delete occurs, delete by `workflow_id` or
   `run_id` before deleting canonical rows.
5. Add `rebuild_search_indexes(conn)` in `migrate.rs`. It must:

   - clear both FTS tables;
   - iterate canonical memories and run steps;
   - parse JSON with a safe empty-object fallback;
   - repopulate indexes;
   - be idempotent;
   - run only when a schema-version marker indicates the initial backfill is
     needed, not on every app startup.

   Add a tiny `schema_meta(key PRIMARY KEY, value)` table if no migration
   version table exists. Use a key such as `search_fts_backfill_v1`.

**Verify**:
`cargo test --locked --manifest-path src-tauri/Cargo.toml db::history`
→ tests prove insert/update/delete synchronization, UTF-8-safe bounds, and an
idempotent legacy-row backfill.

### Step 3: Add plain-text search and exact run-detail APIs

In `db/history.rs`, define camelCase-serialized DTOs:

- `RunHistoryItem`: run id, workflow id/name, trigger, status, error, start,
  finish, created time, step count, and a bounded final-output preview.
- `RunHistoryDetail`: one `RunHistoryItem` plus ordered `RunHistoryStep` rows
  containing node/provider/skill/status, parsed input/output `Value`, error,
  and timestamps.
- `HistorySearchInput`: `query`, optional `workflow_id`, optional `limit`.
- `HistorySearchHit`: `kind` (`run_step` or `memory`), stable source id, run and
  workflow identifiers/names where applicable, title, snippet, timestamp, and
  numeric rank.

Implement:

- `list_run_history(workflow_id: Option<&str>, limit, offset)`; clamp limit to
  1–100 and order newest first.
- `get_run_history(run_id)`; return `None` for an unknown id.
- `search_history(input)`; reject an empty query and clamp limit to 1–50.

Implement `plain_text_fts_query` by splitting on Unicode whitespace, dropping
empty tokens, doubling embedded `"`, limiting to 12 terms/64 characters per
term, and joining safe quoted prefix terms with `AND`. Never pass raw user text
to `MATCH`.

Search both FTS tables, use `bm25(...)`, generate `snippet(...)` with explicit
`[`/`]` markers, union in Rust, sort lowest BM25 first, and cap after the union.
Do not expose raw JSON or an unbounded body in a result card.

Expose Tauri commands:

- `list_run_history(workflow_id, limit, offset)`
- `get_run_history(run_id)`
- `search_history(input)`

Register them in `src-tauri/src/lib.rs`, then mirror the DTOs and invoke
wrappers in `types.ts` and `api.ts`.

**Verify**:
`cargo test --locked --manifest-path src-tauri/Cargo.toml db::history`
→ tests cover punctuation-only input, quotes/operators, workflow filtering,
memory/run unions, ranking, limits, missing runs, and Unicode snippets.

### Step 4: Build a first-class History page

1. Add `history` to `SidebarView`, a History icon/button to `SidebarNav`, and a
   `HistoryPage` branch in `WorkflowCanvas`. Keep the existing canvas,
   schedules, settings, Activity rail, and Memories modal behavior unchanged.
2. Create `src/features/workflow/components/history-page/` with colocated
   `history-page.tsx`, `index.ts`, and pure `history-format.ts` helpers.
3. The page must provide:

   - a search field with a 200–300 ms debounce;
   - scope switch: **Current workflow** when one is selected, or **All
     workflows**;
   - blank-query newest-run browsing with explicit `Load more` pagination;
   - non-empty search results grouped visually as Run step or Memory;
   - loading, empty, and stable safe-error states;
   - click-through from a run hit to exact run detail;
   - ordered step detail with collapsed input/output `<details>` blocks;
   - plain `<pre>`/text rendering only; never `dangerouslySetInnerHTML`.

4. Cancel stale effects with a boolean/Abort-style generation guard, matching
   the component-local async patterns in `MemoriesInspector`.
5. Add semantic CSS tokens only if an existing token cannot express the state.
   Follow `docs/design-system.md`, the existing settings/schedules page spacing,
   accessible labels, focus-visible behavior, and compact scroll ownership.

**Verify**:
`bun test tests/history-search.test.ts && bun run build:frontend`
→ tests and build pass.

### Step 5: Document the local-history contract

Update `specs.md` and `docs/install.md`:

- runs and saved memories are searchable locally;
- the index is derived and rebuildable;
- no embedding or remote service is used;
- deleting canonical data removes it from search;
- history may contain prompts/tool results and should be treated as private
  local data.

**Verify**: `rg -n "FTS5|searchable run history|derived index" specs.md docs/install.md`
→ the intended contract appears in both docs.

## Test plan

### Rust

Add colocated tests in `db/history.rs` and `db/migrate.rs` for:

- FTS5 availability with the shipped SQLite build;
- fresh schema and legacy backfill;
- JSON text extraction bounds/control-character handling;
- memory and run-step insert/update/delete synchronization;
- workflow/run bulk deletion cleanup;
- safe plain-text query construction for quotes, punctuation, Unicode, and
  apparent FTS operators;
- workflow-filtered and all-workflow search;
- stable ordering/limits and bounded snippets;
- exact run details in step creation order.

### Frontend

Add `tests/history-search.test.ts` for pure formatting/query-state helpers and a
colocated component test if the existing Bun/React test harness can render the
page without introducing a new dependency. Cover:

- blank query selects browsing mode;
- current/all-workflow request mapping;
- run/memory result labels;
- stale response suppression;
- snippets render as literal text.

## Done criteria

- [ ] Fresh and migrated databases contain both FTS5 indexes.
- [ ] Indexes rebuild from canonical rows and stay synchronized transactionally.
- [ ] Plain-text searches cannot produce an FTS syntax error.
- [ ] History browsing and search work across current/all workflows.
- [ ] Exact run detail shows ordered persisted steps without HTML execution.
- [ ] `cargo test --locked --manifest-path src-tauri/Cargo.toml db::history` passes.
- [ ] `cargo test --locked --manifest-path src-tauri/Cargo.toml db::migrate` passes.
- [ ] `bun test tests/history-search.test.ts` passes.
- [ ] `bun run build:frontend` passes.
- [ ] `bun run check` passes on a normal host; no new failures are accepted.
- [ ] `git diff --check` passes.
- [ ] No files outside the in-scope list are modified by this plan.
- [ ] Plan 025 is marked DONE in `plans/README.md`.

## STOP conditions

Stop and report if:

- the shipped bundled SQLite cannot enable FTS5 without replacing the SQLite
  runtime or loading an unsigned dynamic extension;
- preserving FTS consistency appears to require indexing secrets from the OS
  credential store or transient environment variables;
- another in-progress change has introduced a canonical run-history API or
  search index that overlaps this design;
- migrating/backfilling requires rewriting or deleting canonical run/memory
  rows;
- the History page requires a new routing framework;
- a step verification fails twice after a reasonable correction;
- an out-of-scope file is required for product behavior rather than imports or
  exports explicitly listed above.

## Maintenance notes

- FTS rows are cache, not user data. Future export/import or deletion work must
  operate on canonical tables and then rebuild/clean the index.
- Any future `run_steps` write path must use the shared indexing helper.
- Plan 026 rebuilds the memories table to add scope metadata; it must rebuild
  `memory_fts` after that migration.
- Plan 027 consumes `search_history`/FTS primitives but applies its own bounded
  retrieval ranking. Do not couple user-facing search ranking to prompt recall.
- Semantic embeddings remain deliberately deferred until retrieval quality is
  measured and a local/privacy-safe provider decision is recorded.
