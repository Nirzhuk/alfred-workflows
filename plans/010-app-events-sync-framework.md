# Plan 010: Add app events, polling, and subscription lifecycle

> **Executor instructions**: Implement after Plan 008. Plan 009 is recommended
> but not technically required. Verify each step and update the index only when
> complete.
>
> **Drift check (run first)**: confirm `app_connections` and `TokenStore` from
> Plan 008 exist. Then hash `src-tauri/src/db/triggers.rs
> src-tauri/src/triggers/mod.rs src-tauri/src/triggers/http.rs
> src-tauri/src/runner/mod.rs src/features/workflow/types.ts
> src/features/workflow/components/triggers-modal/triggers-modal.tsx`.
> Baseline hashes at Git commit `36835c9` on 2026-08-13 begin `4264fbdb`,
> `ff71bfd6`, `a628ee3f`, `d19cbc76`, `42d759c4`, and `ad478c1c`.

## Status

- **Priority**: P0
- **Effort**: XL
- **Risk**: HIGH
- **Depends on**: Plan 008
- **Category**: architecture / data handling
- **Planned at**: 2026-08-11; reconciled at `36835c9`, 2026-08-13

## Why this matters

Useful integrations react to mentions, new mail, issue changes, and incidents.
Those sources vary—polling, WebSocket, Pub/Sub, and webhooks—but workflow runs
need one normalized event contract, one deduplication policy, and one lifecycle
while the desktop app is open.

## Current state

- Frontend `TriggerSource` is the closed union `"file" | "webhook"` at
  `src/features/workflow/types.ts:457`; Rust/SQLite already stores source as a
  string, which is easier to extend.
- `TriggerRuntime` owns file watchers and a loopback HTTP server.
- `RunTrigger::Event(String)` is provider-neutral, but enqueueing stores the
  full event string in `runs.payload_json`. Prompt injection truncates at 8,000
  characters; database persistence does not minimize sensitive payloads.
- Schedules/triggers only work while Alfred is running, including tray mode.
  This local-runtime rule remains true until Plan 011.

## Event contract

Normalize every provider event before it reaches the runner:

```text
schemaVersion, providerId, eventType, connectionId, externalEventId,
occurredAt, subject, actor, resourceUrl, preview, attributes
```

`attributes` is an allow-listed small map. Raw webhook bodies, email bodies,
attachments, Slack thread history, tokens, signatures, and provider headers are
not workflow payloads. The default preview limit is 1,000 Unicode characters;
the whole normalized event must remain below a documented byte limit.

## Commands you will need

- `bun test`
- `bun run build:frontend`
- `cargo test --manifest-path src-tauri/Cargo.toml`
- `cargo check --manifest-path src-tauri/Cargo.toml`
- `rg -n "app_trigger_state|app_event_receipts|NormalizedAppEvent" src src-tauri tests`

## Scope

**In scope**:

- Generic app trigger descriptors/configuration.
- Normalization, dedupe, checkpoints/cursors, polling/socket workers, retry,
  renewal scheduling, status, and local lifecycle.
- Trigger UI driven by provider event descriptors.
- Safe event persistence and tests.

**Out of scope**:

- Public internet webhook endpoints and execution while the app is offline
  (Plan 011).
- Real provider events (provider plans consume this framework).
- Full-text mail/document/message indexing.
- Reusing loopback webhook bearer secrets as OAuth credentials.

## Git workflow

Use the current repository history and branch conventions. Preserve prerequisite
and unrelated changes, do not commit/push without user direction, and avoid
provider-specific event code in the core framework change. Update the index only
after all gates.

## Implementation steps

### Step 1: Define app trigger and normalized event models

Add an `app` trigger source whose config stores only `providerId`, `eventType`,
`connectionId`, filters, and descriptor version. Define Rust and TypeScript
event descriptors: label, required scopes, supported delivery modes, filter
fields, and whether resource content is fetched on demand.

Normalize and validate events in Rust. Require an external event ID when the
provider offers one; otherwise derive a deterministic hash from stable fields,
never from a secret. Reject over-size/deep payloads and invalid timestamps.

**Verify**: serialization fixtures round-trip Rust ↔ TypeScript shape and
oversized/raw-secret fields are rejected or removed.

### Step 2: Add durable cursor, dedupe, and delivery state

Add additive tables such as:

- `app_trigger_state(trigger_id, cursor, subscription_id, expires_at,
  last_polled_at, last_success_at, last_error_code, overrun_count, updated_at)`;
- `app_event_receipts(trigger_id, external_event_id, received_at,
  disposition, run_id, reason_code,
  PRIMARY KEY(trigger_id, external_event_id))`;
- `app_event_queue(id, trigger_id, external_event_id, normalized_event_json,
  enqueued_at, started_at, UNIQUE(trigger_id, external_event_id))` for accepted
  events waiting on the workflow's existing single-active-run slot.

Keep receipt retention bounded (for example 30 days or a per-trigger cap).
Insert receipt and enqueue exactly once in a transaction or recoverable state
machine. Advance a polling cursor only after all accepted events are durably
recorded. A crash between receipt and run creation must be repairable without
double-running the workflow.

Use explicit receipt dispositions such as `queued`, `enqueued`,
`dropped_overrun`, and `rejected_invalid`. A run that later fails is not
automatically re-enqueued from its receipt; an explicit user “run again” action
is a separate product feature. Receipt pruning must not remove a pending queue
item or the receipt needed to deduplicate it.

**Verify**: migration tests plus unit tests for duplicate delivery, out-of-order
events, crash recovery, cursor rollback, one-shot failed runs, terminal receipt
dispositions, and receipt pruning.

### Step 3: Implement a provider-neutral sync runtime

Create a runtime separate from the existing file/webhook runtime, sharing app
lifecycle and cancellation. Provider event adapters expose poll, connect
(optional socket), renew (optional), and disconnect. The runtime must:

- start only enabled triggers with healthy connections;
- consolidate triggers sharing a connection/provider where safe;
- apply jittered exponential backoff and honor `Retry-After`;
- persist checkpoints and renewal deadlines;
- pause on revoked/expired credentials and surface a stable status;
- stop cleanly on app exit and reload on trigger edits;
- limit concurrency so polling never starves workflow execution;
- drain `app_event_queue` when the existing per-workflow run slot is free rather
  than calling `start_run` and losing an event while that workflow is active;
- enforce a configurable per-trigger pending cap. Pull-based providers stop
  accepting events and do not advance beyond the unaccepted cursor. For
  non-replayable socket/push delivery, reject the newest event with a durable
  `dropped_overrun` receipt and increment the visible overrun counter. Never
  delete an older event that was already accepted.

It must not promise background delivery when the process is not running.

**Verify**: fake-clock/fake-provider tests cover poll intervals, backoff,
renewal, disable/re-enable, connection expiry, queue draining, pull backpressure,
non-replayable overrun, restart recovery, and graceful shutdown.

### Step 4: Minimize event data before workflow enqueue

Change app-trigger enqueueing so `runs.payload_json` receives only the
normalized event. Keep existing manual/file/webhook behavior compatible. Add a
safe prompt renderer that labels external content as untrusted data and does
not interpret event text as workflow instructions.

Expose provider resource IDs so a later app action may explicitly fetch detail
using the connected account, instead of embedding entire bodies in the event.
Apply output/preview limits before both SQLite and `run://event` emission.

**Verify**: database tests assert a raw fixture body, authorization header, and
signature are absent while normalized subject/preview/resource ID remain.

### Step 5: Add descriptor-driven trigger UI and health

Extend the triggers modal with **Connected app**. Select provider/event,
connection, and descriptor filter fields. Explain “Runs while Alfred is
open” beside local delivery modes. Display last success, next poll/renewal,
paused credential state, and a reconnect action linking to Connected Apps.

Do not ask users to paste webhook URLs or secrets into these provider forms.

**Verify**: frontend tests cover descriptor switching, filter validation,
connection filtering, local-runtime disclosure, and status rendering.

### Step 6: Add observability without sensitive content

Use stable operational counters/status (poll succeeded, events accepted,
duplicates ignored, renewal due, rate limited) and correlation IDs. Do not log
subjects, message previews, email addresses, tokens, signatures, or raw bodies
by default. Add a user-visible “last error code” with a provider-safe message.

## Test plan

- `bun test`
- `bun run build:frontend`
- `cargo test --manifest-path src-tauri/Cargo.toml`
- Fake-provider end-to-end: event → receipt → one run → restart → no duplicate.
- Inspect SQLite and logs for the fixture token/raw body.
- Manual app-close test confirms UI does not claim events are received offline.

## Done criteria

- [ ] Providers can register events without modifying core trigger UI/runtime.
- [ ] Events are normalized, bounded, deduplicated, and recoverable.
- [ ] Poll/socket/subscription lifecycle pauses and resumes safely.
- [ ] The durable queue respects the existing one-active-run-per-workflow rule.
- [ ] Event bursts are bounded without silently deleting accepted events.
- [ ] Failed runs are one-shot and are never silently re-enqueued from receipts.
- [ ] Only allow-listed event data reaches runs and prompts.
- [ ] UI clearly distinguishes local-only delivery from cloud delivery.
- [ ] Existing schedule/file/webhook behavior remains green.

## STOP conditions

- Plan 008 token access can leak through event adapter errors or logs.
- Exactly-once state cannot be made recoverable with the current run schema;
  write a dedicated state-machine migration proposal before proceeding.
- Product requires delivery while the desktop is closed; implement Plan 011,
  not a hidden always-on OS daemon in this plan.
- A provider needs public callbacks before any local poll/socket mode exists;
  block that provider on Plan 011.

## Maintenance notes

- Provider subscription expiry rules change; adapters own renewal cadence.
- Version normalized event schemas additively and preserve old workflow runs.
- Dedupe retention and event preview limits should become documented settings
  only if real usage shows the defaults are insufficient.
