# Plan 028: Curate memory through reviewable post-run suggestions

> **Executor instructions**: Execute only after Plans 025–027 are DONE. The
> reviewer processes untrusted model output and proposes durable state changes;
> validate every boundary, never persist raw provider errors, and never bypass
> user approval. Run every verification command and update the status in
> `plans/README.md` when complete.
>
> **Drift check (run first)**:
> `git diff --stat 6af3e7e..HEAD -- src-tauri/src/db/schema.sql src-tauri/src/db/migrate.rs src-tauri/src/db/memories.rs src-tauri/src/db/history.rs src-tauri/src/db/memory_retrieval.rs src-tauri/src/db/memory_curation.rs src-tauri/src/agents src-tauri/src/runner/mod.rs src-tauri/src/commands/mod.rs src-tauri/src/lib.rs src/features/workflow/types.ts src/features/workflow/api.ts src/features/workflow/store.ts src/features/workflow/components/memories-inspector src/features/workflow/components/history-page src/features/settings src/App.css tests specs.md docs/install.md`
>
> Reconcile against the live scoped-memory, retrieval, and history contracts.
> Preserve the unrelated Connected Apps work that was already dirty when this
> plan was authored.

## Status

- **Priority**: P1
- **Effort**: XL (7–10 days)
- **Risk**: HIGH — introduces hidden-cost/background model execution and
  model-authored proposals for durable state
- **Depends on**: `plans/025-searchable-run-history.md`,
  `plans/026-scoped-atomic-memory.md`,
  `plans/027-automatic-memory-retrieval.md`
- **Category**: direction / security
- **Planned at**: commit `62ff2bf`, 2026-08-18, with a dirty worktree
- **Reconciled at**: approved Plan 027 commit `6af3e7e`, 2026-08-18.
  The live code provides lexical scoped-memory visibility, lifecycle and expiry
  filtering, bounded pinned context, chunked deterministic FTS retrieval,
  per-agent retrieval traces, exact History navigation, and a persisted
  workflow recall toggle. Curation must reuse those contracts and preserve
  Plans 025–027 prompt trust framing and byte limits.

## Why this matters

Automatic recall is only as good as what gets saved. Asking users or agents to
manually recognize every durable preference, decision, correction, and lesson
creates gaps; writing model conclusions directly into memory creates silent
falsehoods. This plan adds a consent-aware middle path: after an eligible run,
an explicitly enabled local agent CLI proposes a small set of bounded memory
changes, Alfred validates/deduplicates them, and the user approves, edits, or
rejects each suggestion before canonical memory changes.

## Product decisions

- Background review is **off by default** globally and per workflow.
- Enabling it requires choosing a supported Alfred agent provider and
  acknowledging that one additional model invocation may occur after each
  completed run. No review runs for failed/cancelled runs.
- The first release is `candidate_only`. There is no auto-approve mode and no
  hidden direct write, even for high-confidence output.
- The reviewer uses the provider selected in settings. It does not silently
  move a run transcript to a different provider. The UI explains this boundary.
- Custom-agent-only workflows are reviewable only when a supported reviewer
  provider is configured. Alfred never executes the custom command as a
  background reviewer.
- The review is asynchronous and starts after the run has been marked completed
  and the completion event emitted. Review failure never changes run status or
  final output.
- Review input is a bounded digest built from canonical `run_steps`: at most
  32 KiB total, newest relevant steps retained, and no transient console
  activity. It is framed as untrusted data.
- The reviewer sees at most 12 relevant existing memories from Plan 027 so it
  can propose create/supersede/retract operations. It never sees OS credential
  values or connected-app credential metadata.
- Reviewer output is strict JSON, maximum five suggestions. Reject the entire
  response on schema violation; never “repair” arbitrary prose with a second
  model call.
- Candidate operations are `create`, `supersede`, and `retract`. Supersede and
  retract require an active target memory id in the same visible scope.
- Approval is transactional and revalidates the candidate against current
  canonical memory. A stale/conflicting candidate becomes `blocked`, not
  silently adapted.
- Rejection retains metadata/rationale but not extra raw model output. Users can
  physically delete candidate history later through the data settings.
- Candidate bodies are bounded, sanitized, screened for secret-like material,
  control/invisible characters, and obvious instruction/authorization language
  before storage. This is defense in depth; all eventual prompt injection still
  uses Plans 026–027's untrusted reference framing.
- The reviewer never creates or modifies skills. Procedural learning is a
  separate future plan after factual memory quality is measured.

## Current state

- `src-tauri/src/runner/mod.rs:1640-1688` marks a run completed and emits its
  final output synchronously. There is no post-run job lifecycle.
- `src-tauri/src/agents/mod.rs:63-166` already provides provider-neutral
  `AgentRequest`, `AgentAdapter`, `adapter_for`, model resolution, and run hooks.
  Reuse that adapter boundary; do not invoke provider binaries directly.
- `src-tauri/src/agents/activity.rs` bounds/redacts safe live activity but its
  redaction helpers are private. Do not assume live-console redaction makes
  stored run prompts safe.
- Plan 025 provides exact run detail and FTS search.
- Plan 026 provides centralized memory validation and lifecycle operations.
- Plan 027 provides relevant-memory retrieval and auditable use traces.
- The existing Memories inspector is the canonical management surface; add a
  Suggestions mode there instead of a second modal.
- Settings currently persist UI preferences in feature stores/localStorage,
  but the reviewer runs in Rust after the frontend may be hidden. Persist
  reviewer settings in SQLite, not only localStorage.

## Commands you will need

| Purpose | Command | Expected on success |
|---|---|---|
| Curation tests | `cargo test --locked --manifest-path src-tauri/Cargo.toml db::memory_curation` | all pass |
| Runner review tests | `cargo test --locked --manifest-path src-tauri/Cargo.toml runner::tests::background_memory_review` | all pass |
| Memory regression tests | `cargo test --locked --manifest-path src-tauri/Cargo.toml db::memories` | all pass |
| Frontend tests | `bun test tests/memory-curation.test.ts` | all pass |
| Frontend build | `bun run build:frontend` | exit 0 |
| Full gate | `bun run check` | exit 0 on a normal host |
| Diff hygiene | `git diff --check` | no output, exit 0 |

Plan 025's baseline caveats remain applicable. No new failures are acceptable,
and the complete gate must pass in normal CI before DONE.

## Scope

**In scope**:

- `src-tauri/src/db/schema.sql`
- `src-tauri/src/db/migrate.rs`
- `src-tauri/src/db/memory_curation.rs` (new)
- `src-tauri/src/db/mod.rs`
- Plans 025–027 DB modules where shared read/validation APIs must be exposed
- `src-tauri/src/agents/mod.rs` only for a testable reviewer adapter boundary
- `src-tauri/src/runner/mod.rs`
- `src-tauri/src/commands/mod.rs`
- `src-tauri/src/lib.rs`
- `src/features/workflow/types.ts`
- `src/features/workflow/api.ts`
- `src/features/workflow/store.ts`
- `src/features/workflow/components/memories-inspector/`
- Plan 025's `src/features/workflow/components/history-page/`
- `src/features/settings/components/settings-sidebar/settings-sections.ts`
- `src/features/settings/components/settings-sidebar/settings-sidebar.tsx`
- `src/features/settings/components/settings-page/settings-page.tsx`
- `src/features/settings/memory-review.ts` (new)
- `src/App.css`
- `tests/memory-curation.test.ts` (new)
- `specs.md`, `docs/install.md`

**Out of scope**:

- Auto-approval or direct background writes to canonical memory
- Skill creation/editing, self-modifying prompts, or arbitrary files
- External memory providers, embeddings, knowledge graphs, or cloud review
- Reviewing failed/cancelled runs or live partial runs
- Running more than one review per run
- Replaying raw live activity/tool streams
- Retrying provider calls automatically; the user may manually retry once
- Team approval queues, notifications to messaging platforms, or remote review
- A general background-job framework unrelated to memory review

## Git workflow

- Branch: `advisor/028-safe-memory-curation`
- Suggested commits: `Persist memory review jobs and candidates`, `Run bounded
  post-run review`, `Add memory suggestion approval UI`.
- Do not push or open a pull request unless instructed.

## Steps

### Step 1: Persist explicit reviewer settings, jobs, and candidates

Add these tables with fresh/migrated tests:

```sql
CREATE TABLE IF NOT EXISTS memory_review_settings (
  id INTEGER PRIMARY KEY CHECK (id = 1),
  enabled INTEGER NOT NULL DEFAULT 0,
  provider TEXT,
  model TEXT,
  max_candidates INTEGER NOT NULL DEFAULT 5 CHECK (max_candidates BETWEEN 1 AND 5),
  updated_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS workflow_memory_review (
  workflow_id TEXT PRIMARY KEY REFERENCES workflows(id) ON DELETE CASCADE,
  enabled INTEGER NOT NULL DEFAULT 0,
  updated_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS memory_reviews (
  run_id TEXT PRIMARY KEY REFERENCES runs(id) ON DELETE CASCADE,
  workflow_id TEXT NOT NULL REFERENCES workflows(id) ON DELETE CASCADE,
  status TEXT NOT NULL CHECK (
    status IN ('pending','running','completed','failed','skipped')
  ),
  provider TEXT NOT NULL,
  model TEXT,
  error_code TEXT,
  candidate_count INTEGER NOT NULL DEFAULT 0,
  started_at TEXT,
  finished_at TEXT,
  created_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS memory_candidates (
  id TEXT PRIMARY KEY NOT NULL,
  review_run_id TEXT NOT NULL REFERENCES memory_reviews(run_id) ON DELETE CASCADE,
  workflow_id TEXT NOT NULL REFERENCES workflows(id) ON DELETE CASCADE,
  source_node_id TEXT,
  operation TEXT NOT NULL CHECK (operation IN ('create','supersede','retract')),
  target_memory_id TEXT REFERENCES memories(id) ON DELETE SET NULL,
  scope_type TEXT NOT NULL CHECK (scope_type IN ('user','workspace','workflow')),
  scope_key TEXT NOT NULL,
  memory_type TEXT NOT NULL,
  title TEXT NOT NULL,
  body TEXT NOT NULL,
  confidence REAL NOT NULL CHECK (confidence >= 0 AND confidence <= 1),
  rationale TEXT NOT NULL,
  content_hash TEXT NOT NULL,
  status TEXT NOT NULL CHECK (
    status IN ('pending','approved','rejected','blocked')
  ),
  blocked_code TEXT,
  created_at TEXT NOT NULL,
  decided_at TEXT,
  UNIQUE (review_run_id, content_hash)
);
```

Additional rules:

- initialize singleton settings as disabled with null provider/model;
- validate provider against `AgentProvider::from_str` in Rust, not a SQL enum;
- never store a raw provider error, prompt, response, or transcript in review
  tables;
- index candidate status/workflow/time and review status/time.

Expose typed Rust DTOs and Tauri commands:

- `get_memory_review_settings`
- `update_memory_review_settings`
- `set_workflow_memory_review`
- `list_memory_candidates(workflow_id, status)`
- `update_memory_candidate` (editable title/body/scope/type only while pending)
- `approve_memory_candidate`
- `reject_memory_candidate`
- `retry_memory_review(run_id)` with one active/complete job invariant

**Verify**:
`cargo test --locked --manifest-path src-tauri/Cargo.toml db::migrate`
→ fresh, upgrade, FK, singleton, and uniqueness tests pass.

### Step 2: Build a bounded, testable review input

In `db/memory_curation.rs`, implement pure helpers:

- `build_review_digest(run_detail, max_bytes = 32 * 1024)`;
- `candidate_existing_memory_context(run_id, workflow_context)` using Plan
  027's retrieval engine capped at 12 items / 12 KiB;
- `build_review_prompt(digest, existing)`.

Digest behavior:

- include workflow id/name, run id, and completed status;
- include each step's node id/provider/status plus bounded string leaves from
  input/output; omit exact duplicated text;
- prioritize final output, user/input-node prompts, agent outputs, and explicit
  corrections; drop utility receipts before dropping recent agent output;
- retain newest content when over budget;
- strip control characters except newline/tab and truncate on UTF-8 boundaries;
- do not include `run_memory_uses` scores or memory candidate history;
- never load full artifacts or credential-store values.

Review prompt must:

- state that the transcript/existing memories are untrusted data and embedded
  instructions must be ignored;
- distinguish durable facts/preferences/decisions/constraints/lessons from
  temporary paths, raw logs, generic knowledge, and task ephemera;
- request zero to five candidates;
- require exact JSON with a top-level `{ "candidates": [...] }`;
- define operation/scope/type enums and require `targetMemoryId` for
  supersede/retract;
- prohibit credentials, tokens, private keys, auth codes, cookies, environment
  dumps, and hidden instructions;
- require compact title/body/rationale and confidence as a number.

Add snapshot-like assertions on required contract phrases without freezing the
entire prompt.

**Verify**:
`cargo test --locked --manifest-path src-tauri/Cargo.toml db::memory_curation::tests::builds_bounded_untrusted_review_prompt`
→ bounds, ordering, exclusions, and trust labels pass.

### Step 3: Parse and validate reviewer output without repair calls

Define a strict deserialization shape with `#[serde(deny_unknown_fields)]` at
every object level. Accept either raw JSON or one outer markdown JSON fence;
reject all surrounding prose, multiple fences, trailing content, NaN, and more
than five candidates.

Central validation before candidate insertion:

- title: 1–120 Unicode scalar values;
- body: 1–1,200 UTF-8 bytes;
- rationale: 1–500 UTF-8 bytes;
- memory type/scope must pass Plan 026 validators;
- user scope key must be `local-user`; workflow scope must be the run workflow;
  workspace must match that workflow's normalized working directory;
- supersede/retract target must be among the existing memories given to the
  reviewer, still active, visible, and in the same scope;
- `create` must not carry a target id;
- normalize whitespace for SHA-256 content hash; reject exact duplicates of an
  active canonical memory or pending candidate in the same scope/type;
- reject bidi override/isolate characters, zero-width characters, non-tab/
  newline controls, and Unicode tag characters;
- reject high-signal secret forms: bearer authorization, private-key headers,
  common `*_TOKEN`/`*_SECRET`/`*_PASSWORD` assignments, and known provider
  token prefixes. Tests use synthetic fixtures only; never log the rejected
  value;
- reject bodies whose primary content is instruction/authorization language
  such as attempts to override prompts, grant permissions, reveal secrets, or
  execute commands. Keep this conservative and deterministic; false positives
  become a skipped candidate, not a run failure.

If any candidate violates the JSON schema, reject the whole response with
stable review error `invalid_response`. If a structurally valid individual
candidate fails content validation, omit it and continue with other candidates,
recording only aggregate rejected count in internal debug logs without body.

**Verify**:
`cargo test --locked --manifest-path src-tauri/Cargo.toml db::memory_curation`
→ valid create/supersede/retract plus malformed JSON, fence, duplicate, stale
target, control/invisible, secret-like, injection-like, and size cases pass.

### Step 4: Run one asynchronous review after an eligible completed run

Add a testable `MemoryReviewRunner` boundary rather than hard-coding global
state into `execute_run`:

```rust
pub trait ReviewAgent: Send + Sync {
    fn run_review(
        &self,
        provider: AgentProvider,
        request: AgentRequest,
    ) -> Result<AgentResponse, AgentError>;
}
```

The production implementation delegates to `adapter_for(provider).run` with
no live activity hook and no workflow cancellation token. Tests use a fake.

After `set_run_status(..., "completed")` and after emitting the completed
event:

1. read global + workflow settings;
2. if disabled/missing provider, create no job and return;
3. insert `memory_reviews` as pending using `INSERT ... ON CONFLICT DO NOTHING`;
4. spawn one blocking background task through Tauri's runtime;
5. atomically claim pending → running; if claim count is zero, exit;
6. build digest/existing context, invoke selected provider once, parse/validate,
   insert candidates, and mark completed in one final transaction;
7. on auth/missing binary/timeout/invalid response, mark failed with a stable
   code (`auth_required`, `provider_unavailable`, `timeout`,
   `invalid_response`, `internal`) and no raw error text;
8. emit `memory://candidates-changed` with only workflow id and pending count.

No automatic retries. `retry_memory_review` may reset a failed job to pending
only after settings are valid and must preserve one invocation at a time.

Do not hold the SQLite mutex while the provider process runs. Read inputs,
release DB, invoke, then reacquire for validated inserts.

**Verify**:
`cargo test --locked --manifest-path src-tauri/Cargo.toml runner::tests::background_memory_review`
→ fake-agent tests cover disabled behavior, exact-once claim, non-blocking run
completion, success, each stable failure, retry, and no DB lock during call.

### Step 5: Apply approvals transactionally through the canonical memory API

In `approve_memory_candidate`:

1. start an immediate transaction and reload the pending candidate;
2. revalidate scope, target, duplicate hash, lifecycle, and expiry against live
   canonical memory;
3. for `create`, call/refactor Plan 026's canonical insert logic with
   `source='review'`, source run/node, confidence, default salience 50, active,
   and unpinned;
4. for `supersede`, create the new memory with `supersedes_id=target`, then mark
   the target `superseded`, unpinned, and updated in the same transaction;
5. for `retract`, mark target `retracted`, unpinned, and updated; no empty
   replacement memory is created;
6. maintain Plan 025 FTS in the same transaction;
7. mark candidate approved with `decided_at` and emit memory/candidate changed
   events after commit.

If the target changed, disappeared, changed scope, or a duplicate now exists,
mark the candidate `blocked` with a stable `blocked_code` and make no canonical
change. Rejection only changes candidate status/time.

**Verify**:
`cargo test --locked --manifest-path src-tauri/Cargo.toml db::memory_curation::tests::approval_is_atomic`
→ create/supersede/retract, stale conflict, duplicate race, FTS, pin clearing,
and rollback cases pass.

### Step 6: Add explicit settings and acknowledgement

Add a **Memory review** settings section under Local:

- global enabled switch, default off;
- supported provider select populated from Alfred's existing providers;
- optional model field/select using existing model discovery;
- explanation: one additional model call after eligible runs, selected CLI may
  receive a bounded digest of persisted run text, suggestions always require
  approval, and failures do not affect workflows;
- enabling requires a confirmation checkbox/inline acknowledgement before the
  save button is enabled;
- no credential fields; provider CLIs remain responsible for authentication;
- show stable last failure guidance without raw provider errors.

Persist through Tauri/SQLite. `src/features/settings/memory-review.ts` may hold a
small Zustand store patterned after notifications/integrations, but must not be
the source of truth.

In the Memories inspector, add a per-workflow **Suggest memories after runs**
switch that is disabled until global settings are enabled/configured.

**Verify**:
`bun test tests/memory-curation.test.ts && bun run build:frontend`
→ off defaults, acknowledgement, provider/model payload, workflow gate, and
safe error mapping pass.

### Step 7: Add a Suggestions queue to the Memories inspector

Extend the existing inspector with All memories / Suggestions modes and a
pending-count badge.

For each suggestion show:

- operation, proposed scope/type, title/body, confidence, compact rationale,
  source run link, target memory link when applicable;
- editable title/body/scope/type while pending;
- Approve, Reject, and (for failed reviews) Retry review actions;
- a blocked explanation that never leaks raw provider/DB errors.

Requirements:

- approving/rejecting removes the row from Pending immediately only after the
  backend succeeds;
- confirmation is required for user-scope approval and retract operations;
- link source run to History and target/result memory to the inspector detail;
- listen to `memory://candidates-changed` and refresh only the affected active
  workflow;
- preserve keyboard focus after list updates and expose status changes through
  a polite live region;
- use existing modal/select/button tokens and responsive patterns.

Extend History run detail with review status, reviewer provider/model, candidate
count, and links to pending/decided suggestions. Do not show the review prompt
or raw response.

**Verify**:
`bun test tests/memory-curation.test.ts && bun run build:frontend`
→ queue state transitions, editing, confirmations, events, links, and safe
failure copy pass.

### Step 8: Document cost, consent, privacy, and recovery

Update `specs.md` and `docs/install.md`:

- default off and explicit provider/acknowledgement;
- one possible extra invocation per completed eligible run;
- bounded digest and same local CLI boundary;
- candidate-only behavior and approval operations;
- no skill changes or direct writes;
- stable failure/retry behavior;
- how to disable global/per-workflow review and clear candidate history.

**Verify**:
`rg -n "candidate-only|additional model|32 KiB|approval|Memory review" specs.md docs/install.md`
→ consent and cost contract is explicit.

## Test plan

### Database and validation

- Fresh/migrated tables, constraints, cascades, singleton defaults.
- Exact-once review claim and one review per run.
- Strict JSON/fence parser and deny-unknown-fields behavior.
- Every enum/size/scope/target constraint.
- Duplicate hashing by normalized body + scope + type.
- Synthetic secret/invisible/injection fixture rejection without value logging.
- Transactional approve create/supersede/retract and stale blocking.
- FTS and retrieval visibility after approval.

### Background execution

- Global/workflow disabled paths make zero adapter calls.
- Completion event is emitted before slow fake review returns.
- DB mutex is free while fake review blocks.
- Selected provider/model are passed exactly once.
- Failed/cancelled runs do not review.
- Auth, timeout, unavailable, malformed output, and internal errors persist only
  stable codes.
- Manual retry cannot overlap or create a second review row.

### Frontend

- Safe off defaults and acknowledgement requirement.
- Provider/model/global/workflow settings round-trip.
- Candidate list/edit/approve/reject/blocked transitions.
- Confirmation for user-scope and retract.
- Event-driven refresh scoped to the active workflow.
- History/memory navigation and accessible status announcements.
- Raw provider errors/prompts/responses never render.

## Done criteria

- [ ] Review is globally and per-workflow off by default.
- [ ] Enabling requires provider selection and explicit acknowledgement.
- [ ] At most one bounded post-run model call occurs per eligible run.
- [ ] Completed run status/output never depends on review success.
- [ ] Strict parsing and deterministic validation bound every candidate.
- [ ] Canonical memory is never changed before explicit approval.
- [ ] Approval atomically handles create/supersede/retract and stale conflicts.
- [ ] No raw transcript, prompt, response, provider error, or secret is stored in
  review metadata tables.
- [ ] Suggestions are inspectable/editable/rejectable with source provenance.
- [ ] Focused Rust/frontend tests and frontend build pass.
- [ ] `bun run check` passes on a normal host.
- [ ] `git diff --check` passes and no out-of-scope files changed.
- [ ] Plan 028 is marked DONE in `plans/README.md`.

## STOP conditions

Stop and report if:

- Plans 025–027 are incomplete or their contracts differ materially;
- implementing review requires storing provider credentials or bypassing
  `AgentAdapter`;
- the selected CLI cannot produce machine-readable output without exposing
  raw reasoning/tool inputs in persisted data;
- review cannot be scheduled after completion without blocking final output;
- exact-once job claiming cannot be guaranteed with SQLite transactions;
- candidate validation would require a second “repair” model call;
- a proposed UX enables automatic approval/direct writes;
- a shared provider error path would store raw CLI output;
- an out-of-scope skill/self-modification system becomes necessary;
- a verification fails twice after a reasonable correction.

## Maintenance notes

- Keep `candidate_only` as the only mode until measured false-positive,
  approval, rejection, contradiction, and stale rates justify any change.
- Store a future `review_prompt_version` before changing extraction semantics so
  quality comparisons remain meaningful.
- A future cheaper-review-model option must repeat the consent flow because it
  may move run text to a different provider.
- Candidate and memory deletion/export need a dedicated privacy plan; do not
  overload rejection with physical deletion.
- Procedural skill learning should be planned separately with diff review and a
  stricter approval boundary than these compact factual candidates.
