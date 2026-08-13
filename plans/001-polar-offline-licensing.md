# Plan 001: Add Polar licensing with an offline entitlement lease

> **Executor instructions**: Follow this plan step by step. Run every
> verification command and confirm the expected result before moving to the
> next step. If anything in the STOP conditions occurs, stop and report; do
> not improvise. When done, update this plan's row in `plans/README.md`.
>
> **Drift check (run first)**: this workspace had no Git metadata when the plan
> was written. Run `git rev-parse --short HEAD`. If it still fails, compare the
> hashes and current-state excerpts below with the live files. If Git now
> exists, record the current SHA in this plan and inspect all in-scope paths for
> changes before proceeding. Any semantic mismatch is a STOP condition.

## Status

- **Priority**: P1
- **Effort**: L
- **Risk**: MED
- **Depends on**: none
- **Category**: direction, security, architecture
- **Planned at**: unversioned workspace snapshot, 2026-08-09

## Why this matters

Agentflow needs one authoritative license state before feature limits can be
added safely. The application is local-first and may run local models without
internet access, so launch and workflow execution must never depend on a live
Polar request. This plan adds direct Polar activation plus a cached 30-day Pro
lease, without an Agentflow backend or a secret API token in the desktop app.

## Current state

Relevant files and their roles:

- `src-tauri/src/lib.rs` wires managed state and Tauri commands. There is no
  licensing module or background license task.
- `src-tauri/src/commands/mod.rs` exposes workflow, schedule, trigger, and
  memory commands returning `Result<_, String>`.
- `src-tauri/src/db/mod.rs` opens one SQLite database, executes
  `db/schema.sql` on every launch, and then applies additive migrations.
- `src-tauri/src/db/schema.sql` contains workflows, runs, schedules, triggers,
  memories, and memory links, but no license state.
- `src/features/workflow/api.ts` is the frontend's thin `invoke` wrapper.
- `src-tauri/Cargo.toml` has no HTTP client dependency.

Observed command registration (`src-tauri/src/lib.rs:96`):

```rust
.invoke_handler(tauri::generate_handler![
    commands::list_workflows,
    commands::get_workflow,
    commands::create_workflow,
    // ...
    commands::clear_memories,
])
```

Observed database initialization (`src-tauri/src/db/mod.rs:42`):

```rust
let conn = Connection::open(&path)?;
conn.execute_batch("PRAGMA foreign_keys = ON;")?;
conn.execute_batch(include_str!("schema.sql"))?;
migrate::apply_migrations(&conn)?;
```

Selected snapshot hashes:

```text
9267f5e755d3c8bba4fc10de4340d312320748d90a6e51c6e8d764f7ab1827a9  src-tauri/Cargo.toml
eac9f21568481783df5117a56d270971c0fbeaf3ae80d315cf3d3e9903a08532  src-tauri/src/lib.rs
92668620509571b5fa19d952162f0c54bd8c58c0a1abbd64f717d2c10a0e0444  src-tauri/src/commands/mod.rs
ac98398e96451f639a19abdeb6255dfc635b1f45be4e498a18c664c25febb4a3  src-tauri/src/db/mod.rs
0d944a32aeafec17cc33188915f20298874bfd13542c7673f6bae7e48da0d103  src-tauri/src/db/schema.sql
0342ac06bf1f81ad43145715c1f86c5db30d93cc978fecfd6184f8b641d05ed7  src/features/workflow/api.ts
```

Conventions to match:

- Rust response structs derive `Serialize`/`Deserialize` and use
  `#[serde(rename_all = "camelCase")]`; see `src-tauri/src/db/schedules.rs`.
- Database methods live in a focused file under `src-tauri/src/db/` and are
  re-exported by `db/mod.rs`; see `db/memories.rs`.
- Frontend API functions are small typed wrappers around `invoke`; see
  `src/features/workflow/api.ts`.
- Errors shown to the user must not contain license keys, full Polar payloads,
  customer email addresses, or billing data.

## Fixed product and technical policy

- Initial activation is online.
- A successful validation is considered fresh for 7 days.
- If a refresh fails because of timeout, DNS, connection, HTTP 429, or HTTP
  5xx, retain Pro until 30 days after the last successful validation.
- Do not perform network I/O in `get_license_status`.
- An explicit `revoked`, `disabled`, invalid-key, or expired response removes
  effective Pro access. Expiration cached from Polar is also honored offline.
- Store the license key locally because Polar revalidation requires it, but
  never return the full key to the React frontend after activation. Return a
  masked form only.
- Polar organization and benefit IDs are public configuration. Read them from
  compile-time environment variables; never add an organization access token.
- Use only the public customer-portal endpoints documented as safe for desktop
  clients:
  - <https://polar.sh/docs/api-reference/customer-portal/license-keys/activate>
  - <https://polar.sh/docs/api-reference/customer-portal/license-keys/validate>
  - <https://polar.sh/docs/api-reference/customer-portal/license-keys/deactivate>

## Commands you will need

| Purpose | Command | Expected on success |
|---------|---------|---------------------|
| Frontend build | `bun run build:frontend` | exit 0; TypeScript and Vite complete |
| Rust check | `cargo check --manifest-path src-tauri/Cargo.toml` | exit 0 |
| Rust tests | `cargo test --manifest-path src-tauri/Cargo.toml` | all tests pass |
| License tests | `cargo test --manifest-path src-tauri/Cargo.toml licensing::` | all licensing tests pass |

Do not use the currently failing repository-wide format check as a reason to
reformat unrelated Rust files.

## Scope

**In scope** (the only files to modify or create):

- `src-tauri/Cargo.toml`
- `src-tauri/Cargo.lock`
- `src-tauri/src/lib.rs`
- `src-tauri/src/commands/mod.rs`
- `src-tauri/src/db/mod.rs`
- `src-tauri/src/db/schema.sql`
- `src-tauri/src/db/license.rs` (new)
- `src-tauri/src/licensing/mod.rs` (new)
- `src-tauri/src/licensing/polar.rs` (new)
- `src-tauri/src/licensing/state.rs` (new)
- `src/features/licensing/types.ts` (new)
- `src/features/licensing/api.ts` (new)
- `plans/README.md` and this plan's status only

**Out of scope**:

- Feature limits and entitlement gates; Plan 002 owns them.
- Settings, activation forms, upgrade dialogs, banners, or CSS; Plan 003 owns
  user-facing work.
- Stripe, Paddle, Lemon Squeezy, a custom server, or webhook receiver.
- A custom customer account system.
- Stronghold/keychain integration or signed offline license files.
- Automatic updates, analytics, pricing, and team seats.

## Git workflow

- The workspace is currently unversioned. Do not initialize Git unless the
  operator asks.
- If Git exists at execution time, use branch
  `advisor/001-polar-offline-licensing` and follow the repository's then-current
  commit convention.
- Do not push or open a PR unless instructed.

## Steps

### Step 1: Add persistent license state

Create `src-tauri/src/db/license.rs` and re-export it from `db/mod.rs`. Add a
single-row `license_state` table to `db/schema.sql`. `schema.sql` is executed on
every launch, so `CREATE TABLE IF NOT EXISTS` handles existing databases; do
not add a duplicate migration.

The record must contain only what offline evaluation and revalidation need:

- full license key (backend only);
- Polar activation ID;
- Polar benefit ID;
- last explicit Polar status (`granted`, `revoked`, or `disabled`);
- optional `expires_at`;
- `last_validated_at` and `last_attempted_at`;
- sanitized last network/error category, never a raw response body;
- non-identifying device label generated once, such as
  `Agentflow macOS 7f3a91c2`;
- `updated_at`.

Implement database methods to load, upsert, update validation fields, and
clear the single record. Keep the full key out of `Debug` output by either not
deriving `Debug` for the stored record or by implementing a redacted formatter.

**Verify**:

```bash
cargo check --manifest-path src-tauri/Cargo.toml
```

Expected: exit 0 and no schema/module errors.

### Step 2: Implement pure offline entitlement evaluation

Create `src-tauri/src/licensing/state.rs`. Model the frontend-safe snapshot
with at least:

- `tier`: `free | pro`;
- `mode`: `free | online | offlineGrace | needsRefresh | revoked | expired`;
- `hasProAccess`: boolean used by every later gate;
- masked key;
- `lastValidatedAt`, `refreshDueAt`, `offlineGraceEndsAt`, and `expiresAt`;
- `refreshRecommended`: boolean;
- a short safe message/category for the UI.

Implement entitlement evaluation as a pure function that accepts the stored
record and an explicit `now`. Keep time injectable so tests do not sleep or
depend on the real clock. Use 7 and 30 day constants in one place.

Rules:

1. No record means Free.
2. Explicit revoked/disabled means Free immediately.
3. Cached expiration at or before `now` means expired Free.
4. Granted and validated within 7 days means online Pro.
5. Granted, older than 7 days but within 30 days means offline-grace Pro and
   recommends refresh.
6. Older than 30 days means `needsRefresh` without Pro access.
7. A network error changes the safe message and attempt timestamp, not the last
   successful validation timestamp or explicit Polar status.

**Verify**:

```bash
cargo test --manifest-path src-tauri/Cargo.toml licensing::state
```

Expected: new state-transition tests pass.

### Step 3: Add the public Polar client

Add `reqwest` with JSON and rustls TLS support and no native OpenSSL dependency.
Use one reusable client with an 8-second total timeout. Add compile-time public
configuration:

- `AGENTFLOW_POLAR_ORGANIZATION_ID`
- `AGENTFLOW_POLAR_BENEFIT_ID`

Missing configuration must return a clear `licensing_not_configured` error; it
must not panic or prevent the rest of Agentflow from running.

In `licensing/polar.rs`, implement activate, validate, and deactivate against
`https://api.polar.sh/v1/customer-portal/license-keys/*`. Do not send an
Authorization header. Parse only fields needed for licensing and ignore the
customer object so customer PII is neither persisted nor logged. Validate that
the returned organization and benefit IDs match Agentflow's configured IDs
before granting Pro.

Classify results:

- successful `granted` response: persist status, activation, expiration, and
  successful validation time;
- explicit revoked/disabled/expired or definitive invalid-key response:
  persist the explicit invalid state;
- timeout, connection, 429, or 5xx: retain the previous valid state and record
  only a sanitized transient-error category;
- malformed success response or mismatched organization/benefit: reject and do
  not grant Pro.

Never log request bodies, keys, response bodies, or customer data.

**Verify**:

```bash
cargo test --manifest-path src-tauri/Cargo.toml licensing::polar
```

Expected: response parsing and error-classification tests pass without making
real network requests.

### Step 4: Expose local-first Tauri commands

Add and register these commands:

- `get_license_status`: local SQLite evaluation only;
- `activate_license(key)`: activate and persist, returning a safe snapshot;
- `refresh_license`: validate the stored key/activation;
- `deactivate_license`: call Polar first, then clear locally only after success.

All network commands must be async. `get_license_status` must remain fast and
offline. Do not add automatic refresh to Tauri startup in this plan; Plan 003
will schedule opportunistic refresh after the app is already usable.

Add matching TypeScript types and `invoke` wrappers under
`src/features/licensing/`. Keep them separate from workflow types/API.

**Verify**:

```bash
bun run build:frontend
cargo check --manifest-path src-tauri/Cargo.toml
```

Expected: both exit 0.

### Step 5: Complete unit coverage and regression gates

Add deterministic Rust tests covering:

- no stored license;
- freshly granted license;
- day 7 boundary;
- day 30 boundary;
- cached expiration;
- revoked and disabled statuses;
- transient network error preserving the last success;
- definitive invalid-key response;
- organization/benefit mismatch;
- masked key output and absence of full key in serialized snapshots/errors;
- activation and deactivation persistence transitions.

No test may call Polar or depend on internet availability.

**Verify**:

```bash
cargo test --manifest-path src-tauri/Cargo.toml
bun run build:frontend
```

Expected: all Rust tests pass and the frontend build exits 0.

## Test plan

- Put pure time/status tests in `src-tauri/src/licensing/state.rs` under
  `#[cfg(test)]`.
- Put JSON fixture and HTTP classification tests in
  `src-tauri/src/licensing/polar.rs`; test parsing functions directly rather
  than running an HTTP server.
- Put persistence tests beside `db/license.rs`, using a test-only in-memory
  connection helper if necessary. If adding that helper requires changing
  files outside scope, stop and request that `src-tauri/src/db/mod.rs` remain
  the only helper location.
- Model test structure after existing module-local tests in
  `src-tauri/src/triggers/http.rs` and `src-tauri/src/skills/mod.rs`.

## Done criteria

- [ ] `cargo test --manifest-path src-tauri/Cargo.toml` exits 0.
- [ ] `bun run build:frontend` exits 0.
- [ ] `get_license_status` contains no network call.
- [ ] No Polar organization access token, Stripe key, webhook secret, or other
      privileged credential exists in source or frontend bundles.
- [ ] Serialized license snapshots contain no full license key or customer PII.
- [ ] A transient validation failure preserves Pro during the 30-day window.
- [ ] A cached expiration or explicit revoked/disabled state removes Pro.
- [ ] No files outside the in-scope list are modified.
- [ ] `plans/README.md` marks Plan 001 `DONE`.

## STOP conditions

Stop and report rather than improvising if:

- Polar's current public client endpoints require an organization access token
  or stop being documented as safe for desktop clients.
- Polar does not return enough status/expiration information to implement the
  stated state machine.
- The implementation would require placing a privileged token in the app.
- Existing source differs semantically from the current-state excerpts or the
  module layout changes during execution.
- Database encryption or OS keychain storage becomes a release requirement;
  that is a separate design decision.
- A verification command fails twice after a reasonable scoped correction.

## Maintenance notes

- Reviewers should focus on key/PII redaction, transient-versus-definitive
  error classification, and exact day-boundary tests.
- Keep Polar DTOs private to the client module. Other code should consume the
  provider-neutral `LicenseSnapshot` only.
- If a signed offline license format is added later, replace the stored lease
  evaluator behind the same snapshot interface so Plan 002 gates do not need
  to change.

