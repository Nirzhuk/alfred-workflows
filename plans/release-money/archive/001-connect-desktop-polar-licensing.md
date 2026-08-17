# Plan 001: Connect Alfred directly to Polar licensing

> **Executor instructions**: This is the first executable coding task. Build
> against injected public configuration, official response fixtures, and a
> local mock server; no Polar account or dashboard access is required. Use only
> Polar's public customer-portal license-key contract—no API token, webhook, or
> Alfred backend is permitted. Follow every verification gate, stop on any
> STOP condition, and update this plan's row in
> `plans/release-money/README.md` when done.
>
> **Drift check (run first)**:
> `git diff --stat ecb94d6..HEAD -- src src-tauri package.json bun.lock`
> Re-read the live command registry, database migrations, credential store,
> capabilities, and settings composition if any in-scope path changed.

## Status

- **Priority**: P0
- **Effort**: L
- **Risk**: HIGH (license-key custody and offline state)
- **Depends on**: none
- **Category**: security, architecture, migration
- **Planned at**: commit `ecb94d6`, 2026-08-15

## Why this matters

Alfred needs a small, local-first adapter around Polar rather than a second
commerce system. The app must keep the customer's full license key out of
SQLite, React, URLs, and logs; validate without delaying startup; preserve a
bounded offline window on transient failure; and apply confirmed revocation
immediately. This state is customer guidance, not a security boundary around
GPL local features or Polar-hosted downloads.

## Current state

- `src-tauri/src/lib.rs` registers commands and managed state; there is no
  licensing module.
- `src-tauri/src/db/mod.rs` opens `app.db`, loads `db/schema.sql`, and runs
  additive migrations.
- `src-tauri/src/integrations/token_store.rs` is the credential-store exemplar:
  `keyring`, opaque references, redacted `Debug`, zeroization, and an in-memory
  test fake. Licensing must use a separate service name,
  `com.alfred.licensing`.
- `src-tauri/Cargo.toml` already includes `reqwest`, `serde`, `keyring`,
  `rusqlite`, `chrono`, `uuid`, and `zeroize`.
- Plan 003 will supply Alfred's real public organization ID and three benefit
  IDs. This plan defines an injectable configuration seam and uses UUID-shaped
  test fixtures; no real identifier or Polar account is needed to finish it.
- Polar's public endpoint response can contain customer data. Deserialize only
  the minimal license fields and never log or forward the raw response.

## Fixed state model

Product and effective state remain separate:

- `product`: `none | desktopAnnual | desktopLifetime | companySeat`;
- `state`: `unlicensed | active | offlineGrace | needsOnline | expired |
  revoked | disabled | deviceLimit | secureStorageUnavailable |
  notConfigured`.

Safe fields exposed outside Rust:

- masked key;
- product and state;
- Polar `benefit_id` only if UI logic needs it, otherwise keep it in Rust;
- activation label/current-device fact;
- `expires_at` when Polar supplies it;
- last successful validation, next refresh time, and offline deadline;
- sanitized stable error code.

Rules:

1. `get_license_status` reads local state and performs no network request.
2. Validate opportunistically when the last success is at least 7 days old.
3. Timeout, DNS/connectivity, HTTP 429, and Polar 5xx are transient. The last
   granted state may remain `offlineGrace` through day 30.
4. A successful Polar response with `revoked` or `disabled`, an explicit
   invalid key, or a passed `expires_at` overrides grace immediately.
5. After day 30 without validation, state is `needsOnline`. No local workflow,
   memory, schedule, trigger, or stored data is disabled or deleted.
6. The full key and activation ID are one zeroizing credential envelope in the
   OS credential store. SQLite stores only safe snapshot fields and an opaque
   credential reference.
7. Benefit IDs are allow-listed from injected configuration. An otherwise
   valid key for an unknown Alfred benefit is rejected as
   `unsupported_product`.

## Commands you will need

| Purpose | Command | Expected on success |
| --- | --- | --- |
| Full check | `bun run check` | frontend tests/build and Rust tests pass |
| License tests | `cargo test --locked --manifest-path src-tauri/Cargo.toml licensing::` | all license tests pass |
| Frontend tests | `bun test` | all frontend tests pass |
| Secret scan | `rg -n '(POLAR_ACCESS_TOKEN|polar.*token|licenseKey|license_key)' src src-tauri/src --glob '!**/*.test.*'` | no Polar credential; license matches are ephemeral input/redacted storage only |

## Scope

**In scope**:

- `src-tauri/Cargo.toml`, `src-tauri/Cargo.lock` only if dependency changes are required;
- `src-tauri/src/lib.rs`, `src-tauri/src/commands/mod.rs`;
- `src-tauri/src/db/mod.rs`, `src-tauri/src/db/schema.sql`,
  `src-tauri/src/db/migrate.rs`, and `src-tauri/src/db/license.rs` (new);
- `src-tauri/src/licensing/**` (new);
- a typed frontend command wrapper under `src/features/licensing/**` (new);
- focused Rust/frontend tests;
- this plan and the release-money index status.

**Out of scope**:

- settings presentation (Plan 002);
- Tauri automatic updater plugins or updater signing;
- custom checkout, webhooks, accounts, email, Company portal, or server;
- Polar organization access tokens or privileged APIs;
- gating any local feature or authorizing Polar-hosted downloads client-side;
- deleting the abandoned Stripe implementation.

## Git workflow

- Branch: `codex/001-polar-desktop-licensing`.
- Commit logical slices with imperative messages such as
  `Add secure Polar license state` and `Validate Polar licenses directly`.
- Do not push or open a PR unless instructed.

## Steps

### Step 1: Add safe persistent state and a licensing credential store

Add an additive single-row license snapshot migration. Persist the product,
status, masked key, safe Polar benefit ID, activation label, `expires_at`, last
successful validation, refresh due, offline deadline, sanitized error code,
and opaque credential reference.

Create a dedicated zeroizing envelope containing exactly the full license key
and activation ID. Follow the `TokenStore` pattern, but use the licensing
service name. Never fall back to plaintext if the OS store is locked.

**Verify**:
`cargo test --locked --manifest-path src-tauri/Cargo.toml licensing::storage`
passes round-trip, overwrite, migration, missing/locked store, redacted debug,
and delete tests.

### Step 2: Add injectable Polar configuration

Create a typed configuration for:

- production and sandbox API bases;
- public organization ID;
- three allow-listed benefit IDs.

Define the seam now with injected UUID-shaped test fixtures. Plan 003 will bind
the approved public values through compile-time/release variables or a reviewed
public configuration file. A source build with no Alfred Polar configuration
returns `notConfigured` and remains fully usable locally.
Reject non-HTTPS production API bases and any endpoint outside the explicit
Polar host allow-list.

**Verify**: configuration tests cover valid production/sandbox fixtures,
missing configuration, duplicate benefit IDs, unknown host, and non-HTTPS URL.

### Step 3: Implement the minimal public Polar client

Implement only:

- `POST /v1/customer-portal/license-keys/activate`;
- `POST /v1/customer-portal/license-keys/validate`;
- `POST /v1/customer-portal/license-keys/deactivate`.

Use bounded connect/total timeouts, JSON size limits, and typed safe errors. Do
not send `Authorization`; do not add an SDK that expects a privileged access
token. Deserialize only license ID, organization ID, benefit ID, status,
activation ID/label, activation limit, and expiration. Ignore and never log
customer/email/address fields or raw bodies.

**Verify**: local mock tests assert exact methods/paths/bodies, absence of
`Authorization`, response size limits, unknown fields, malformed JSON,
timeouts, 403/404/422/429, and 5xx classification.

### Step 4: Implement activation, validation, and deactivation services

On activation, send the entered key, public organization ID, and a
user-readable device label. Accept only a configured benefit ID and
`status=granted`; then atomically store the credential envelope and safe
snapshot. Clear/zeroize the command input and credential working copies as
early as possible; never create a second persistent copy from Polar's echoed
response.

Validation reads key+activation ID from the keychain and sends both to Polar.
Deactivation calls Polar first, then removes local credential/snapshot after a
confirmed success. On transient deactivation failure retain local state and
tell the user it did not complete. A device-limit response links recovery to
Polar's portal rather than adding a privileged device-list API.

**Verify**: service tests cover annual/lifetime/Company, unknown benefit,
device limit, invalid/revoked/disabled/expired, transient errors, secure-store
failure, repeated deactivate, and response redaction.

### Step 5: Add deterministic offline evaluation

Implement a pure evaluator with injected time. Test one instant before, at,
and after day 7 and day 30. A transient error can extend only an already
validated granted state; it cannot create a license. Local `expires_at` takes
precedence when it passes. A confirmed restrictive state never returns to
grace without a later successful granted validation.

**Verify**:
`cargo test --locked --manifest-path src-tauri/Cargo.toml licensing::evaluator`
passes all boundary and precedence cases without network access.

### Step 6: Register safe Tauri commands and background refresh

Expose async commands:

- `activate_license(licenseKey, deviceLabel)`;
- `refresh_license()`;
- `deactivate_license()`;
- `get_license_status()`.

Return safe DTOs only. Use a single-flight mutation guard. Load cached status
before spawning an opportunistic refresh, and never block app startup on
Polar. Do not expose a command that returns the stored key or activation ID.

**Verify**: command tests prove safe serialization, no secret-bearing events,
single-flight behavior, fast cached startup, and graceful missing config.

### Step 7: Run the complete local verification suite

Exercise the command layer against a local mock server with injected public
IDs. Inspect test output, SQLite fixtures, serialized command DTOs, and source
for full keys and activation IDs. OS-packaged keychain and real Polar sandbox
smoke are intentionally deferred to Plans 003 and 005 so this coding plan can
finish in a headless executor environment.

**Verify**: `bun run check` passes; the secret scan finds only documented
ephemeral/keychain handling; no test contacts Polar.

## Test plan

- Rust tests cover secure storage, exact time boundaries, every Polar status,
  response minimization, benefit allow-listing, transient classification, and
  command redaction.
- Database tests use the existing in-memory `Db` pattern and prove upgrades
  preserve all current user data.
- HTTP tests use a local mock server; the normal suite never contacts Polar.
- Frontend wrapper tests assert DTO names and errors only, never full keys.

## Done criteria

- [ ] `bun run check` passes.
- [ ] Activate, Validate/Refresh, Deactivate, and cached Status pass against the local Polar mock.
- [ ] Public requests contain no access token and accept only configured Alfred benefits.
- [ ] Full key and activation ID are keychain-only and absent from SQLite/React/logs.
- [ ] The 7-day refresh and 30-day offline boundaries are deterministic.
- [ ] Revoked, disabled, expired, invalid, transient, and device-limit states are distinct.
- [ ] App startup reads cached state without waiting for Polar.
- [ ] Unconfigured/source builds retain all local functionality.
- [ ] Missing real Polar configuration preserves full source-build usability.
- [ ] The roadmap row is `DONE`.

## STOP conditions

- Polar's public endpoints require a privileged token.
- Secure storage must fall back to SQLite, localStorage, or plaintext files.
- A raw Polar response/customer object must be returned to React or logged.
- A transient failure grants a never-validated key or exceeds 30 days.
- Client-side state is proposed as authorization for hosted downloads.
- Any local workflow or data is gated/deleted by commercial state.
- A verification gate fails twice after a scoped correction.

## Maintenance notes

Public Polar response schemas may add fields; keep deserialization minimal and
ignore unknown PII. Any future provider adapter must preserve the same safe
snapshot boundary rather than leaking a provider SDK through the UI.
