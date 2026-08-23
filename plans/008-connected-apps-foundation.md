# Plan 008: Build the secure Connected Apps foundation

> **Executor instructions**: Follow this plan step by step. Run every
> verification command before moving on. Stop at any STOP condition rather
> than inventing a security or product decision. When complete, update this
> plan's row in `plans/README.md`.
>
> **Drift check (run first)**: baseline reconciled at Git commit `36835c9` on
> 2026-08-13. Run `shasum -a 256 src-tauri/Cargo.toml src-tauri/src/db/schema.sql
> src-tauri/src/db/migrate.rs src-tauri/src/lib.rs
> src/features/settings/components/settings-page/settings-page.tsx
> src/features/workflow/api.ts`. Expected hashes begin respectively with
> `7f8063ce`, `6e175896`, `564d3fb7`, `a329ec9a`, `6a62a170`, and `980f0274`.
> If any differ, re-read the affected file and reconcile this plan first.
>
> **Drift status at 2026-08-20**: all six baseline hashes now differ (actual
> prefixes `287fa8f5`, `2caa3535`, `394cb42b`, `848e2626`, `4fead5b9`,
> `8aaf448a`). The repository has moved well past `36835c9` — plans 009, 010,
> and the provider plans 012–016 all landed on top of this foundation. The
> hashes are stale by expectation, not by regression: `bun run check` and the
> security invariants below were re-verified against current `HEAD` and pass.
> Treat the 2026-08-13 hash list as a historical baseline, not a live gate.

## Status

- **Priority**: P0
- **Effort**: L
- **Risk**: HIGH
- **Depends on**: none
- **Category**: architecture / security
- **Planned at**: 2026-08-11; reconciled at `36835c9`, 2026-08-13
- **Implementation**: code complete. Automated gates re-verified at `HEAD`
  (`5e62adf`) on 2026-08-20: `bun run check` passes end to end — 234 frontend
  tests across 43 files, `tsc && vite build` clean, 371 Rust tests passing,
  0 failures. The architecture contract was re-checked by inspection at the
  same commit and holds (see "Verified at HEAD" below). The only outstanding
  work is packaged-OS credential-store smoke testing, which cannot be run from
  a development checkout.

## Why this matters

Every Slack, Microsoft, Google, GitHub, Linear, and knowledge-source feature
needs the same primitives: connection records, secure tokens, refresh, revoke,
health, and UI. Building provider OAuth directly inside workflow nodes would
duplicate sensitive code and put credentials into workflow JSON or SQLite.

This plan creates the provider-neutral base only. It deliberately does not add
any provider OAuth flow or workflow action.

## Current state

- The app is local Tauri 2 + React; there is no Alfred account or backend.
- `src-tauri/src/db/schema.sql:14-20` hard-codes four *agent CLI* providers.
  Connected apps are a separate concept and must not extend that enum/table.
- `src/features/workflow/types.ts:204-225` stores generic HTTP headers inside a
  node. That path is not acceptable for OAuth access/refresh tokens.
- `src-tauri/src/db/schema.sql:65-76` has a plaintext `triggers.secret`; it is a
  local webhook bearer secret, not a general credential vault.
- Settings currently contains Appearance, General/Runs, Notifications, and
  Data sections. It has no Connected Apps surface.
- Rust has no OAuth or OS-keychain dependency. Provider HTTP should remain in
  Rust so browser CSP changes are not needed for API calls.

## Architecture contract

Use three distinct data classes:

1. **SQLite metadata**: connection ID, provider ID, display name, account and
   workspace identifiers, scopes, status, expiry, timestamps, and a non-secret
   keychain reference.
2. **OS credential store**: access token, refresh token, and provider-specific
   secret material. Use a random opaque connection ID as the account key.
3. **Memory only**: authorization code, PKCE verifier, nonce/state, and freshly
   refreshed tokens before keychain persistence.

Never return tokens to React, write them to workflow JSON, include them in run
events, or log them. The provider catalog and connection data are not related
to `AgentProviderId` or the CLI adapter registry.

## Commands you will need

- `bun test`
- `bun run build:frontend`
- `cargo test --manifest-path src-tauri/Cargo.toml`
- `cargo check --manifest-path src-tauri/Cargo.toml`
- `rg -n "access_token|refresh_token|client_secret|Bearer " src src-tauri tests`

## Scope

**In scope**:

- SQLite migration for non-secret connection metadata.
- A Rust `integrations` module containing provider IDs, connection models,
  repository, redacted command DTOs, and `TokenStore` abstraction.
- OS credential-store implementation using a maintained cross-platform Rust
  keychain library, plus an in-memory fake for tests.
- A provider-neutral native-OAuth helper for loopback callbacks, state, nonce,
  and PKCE utilities. Provider plans supply authorization request configuration;
  token exchange remains provider-owned.
- A shared refresh service and provider registration seam used by scheduled
  health checks and one explicit on-demand recovery attempt.
- Tauri commands to list/get/disconnect connections and query provider catalog.
- Settings > Connected Apps with empty, connected, expired, error, and
  disconnect states.
- Token redaction and migration/round-trip tests.

**Out of scope**:

- Provider-specific authorization URLs, client IDs, scopes, API calls, events,
  or workflow nodes.
- A cloud service, public/cloud browser callback endpoint, or Alfred user
  account. A short-lived loopback callback on the local device is in scope.
- Importing secrets from the existing HTTP node or `triggers.secret`.
- A generic “paste OAuth token” field in React.

## Git workflow

Use the current repository history and branch conventions. Do not commit or
push unless the user asks. Keep implementation changes to this plan's scope,
preserve unrelated work, and update only this plan's index row after all gates
pass.

## Implementation steps

### Step 1: Define provider-neutral models and schema

Add an additive migration through the existing mechanism in
`src-tauri/src/db/migrate.rs`. Create `app_connections` with:

- `id TEXT PRIMARY KEY` (random UUID/ULID, never provider account ID);
- `provider_id TEXT NOT NULL` (no SQL CHECK enum; the Rust catalog validates);
- `display_name`, `external_account_id`, and `external_tenant_id`;
- `connection_mode TEXT NOT NULL` (for example `native_oauth`, `private_bot`,
  or `incoming_webhook`);
- `identity_key TEXT NOT NULL` (an opaque deterministic digest of the
  provider-defined account/tenant/installation identity);
- `scopes_json TEXT NOT NULL DEFAULT '[]'`;
- `status TEXT NOT NULL` constrained to `connected|expired|error|revoked`;
- `expires_at`, `last_checked_at`, `last_error_code`, `created_at`, `updated_at`;
- `credential_ref TEXT NOT NULL UNIQUE`.

Do not store an access token, refresh token, authorization code, client secret,
raw provider error, email body, or Slack message in this table. Add repository
CRUD under `src-tauri/src/db/app_connections.rs` and register it in `db/mod.rs`.
Create a unique index on `(provider_id, connection_mode, identity_key)`.
Providers compute the key only after validating the external identity; do not
persist a connected record whose identity is still unknown. Reconnecting the
same canonical identity upgrades the existing row, while different tenants,
Slack installation modes, or webhook installations remain distinct.

**Verify**: migration works on both an empty database and a fixture containing
the existing schema; `PRAGMA table_info(app_connections)` contains no token or
secret column. Repository tests cover reconnect-upgrade and distinct-account,
tenant, installation-mode, and nullable-display-metadata cases.

### Step 2: Add a secure token-store boundary

Create `src-tauri/src/integrations/` with `models.rs`, `catalog.rs`,
`token_store.rs`, and `mod.rs`. Define a serializable-to-keychain credential
envelope with an explicit version, token values, optional expiry, and only the
provider fields needed to refresh. Its Rust type must not derive `Debug` in a
way that reveals values; implement a redacted representation if diagnostics
need one.

The `TokenStore` interface must support `put`, `get`, and `delete` by opaque
credential reference. Add an in-memory implementation for unit tests. Evaluate
the current maintained `keyring` crate (or equivalent) for macOS Keychain,
Windows Credential Manager, and Linux Secret Service. Use an app-specific
service name such as `com.alfred.connected-apps`.

Disconnect order is: mark connection revoked/inactive, delete keychain entry,
then remove metadata only when cleanup succeeds or the user explicitly accepts
a “metadata only” cleanup. Never silently orphan a usable token.

Add `oauth_native.rs`: a one-shot loopback listener bound to `127.0.0.1` on a
random port, S256 PKCE verifier/challenge and state/optional nonce generation,
one-time callback acceptance, and a short attempt timeout. It accepts provider
authorization configuration and returns the verified code plus the in-memory
attempt context needed for provider-owned token exchange (`verifier`, redirect
URI, and optional nonce). It never exchanges or stores tokens. The provider
validates any returned ID token, including nonce, after exchange.

Add a refresh service: providers may register a
`needs_refresh(connection)` predicate and a refresh handler that receives only
a connection plus backend token-access capability. Both the jittered scheduled
health loop and an action's single on-demand 401 recovery call this service.
Serialize refresh per connection so rotating tokens cannot race; persist the
rotated credential before updating metadata. The service is the sole owner of
refresh-derived `connected|expired|error` transitions. Disconnect/revoke and
other lifecycle operations retain ownership of their own transitions.

**Verify**: fake-store tests prove round trip, overwrite, delete, missing item,
and redacted formatting. OAuth-helper tests cover state mismatch, second
callback ignored, port collision retry, timeout expiry, PKCE derivation, and
attempt-context lifetime. Refresh tests cover scheduled and on-demand success,
rotation serialization, retryable vs terminal errors, and status ownership.
Perform a manual packaged-app smoke test on each shipping OS before calling
this plan done — Linux here, macOS and Windows in Plan 005 matrix E (see
"Ownership split" below).

### Step 3: Expose redacted Tauri commands

Add commands under `src-tauri/src/commands/integrations.rs` and register them in
`commands/mod.rs`/`lib.rs`:

- `list_app_providers`
- `list_app_connections`
- `get_app_connection`
- `get_app_connection_usage` — returns workflows and triggers that directly
  reference the connection plus schedules that transitively run a workflow
  containing a matching `appAction`; include enabled/disabled state.
- `disconnect_app_connection`

DTOs may contain provider/account labels, scope names, expiry, and status. They
must contain no token, credential payload, PKCE value, client secret, or
provider raw response. Normalize backend errors into stable codes such as
`credential_store_locked`, `credential_missing`, and `disconnect_failed`.

Register every command in `commands/mod.rs`/`lib.rs` and verify it works from a
packaged app with `src-tauri/capabilities/default.json`. Provider plans extend
the opener allow-list only for their exact authorization origins.

**Verify**: serialize every command response in a Rust test and assert known
secret fixture strings do not appear.

### Step 4: Add the frontend API/store and Settings surface

Add integration types/API/state under `src/features/integrations/`. Render a
Connected Apps card in the existing settings page style. Provider catalog rows
show provider name, short capability summary, connection status, account label,
and Connect/Manage/Disconnect actions. Connect remains disabled or marked
“Coming next” until the provider plan implements authorization.

Show actionable credential-store errors without surfacing raw backend details.
Disconnect requires confirmation, lists direct and transitive dependencies from
`get_app_connection_usage`, and removes the item from UI only after the backend
succeeds.

**Verify**: add store tests for load, refresh, disconnect success/failure, and
redaction; `bun run build:frontend` passes.

### Step 5: Document the security boundary

Add `docs/connected-apps.md` explaining metadata vs keychain storage, local
execution, how to revoke a provider, Linux keychain prerequisites, and that
deleting the SQLite database does not automatically revoke remote grants.
Document a recovery path for stale keychain entries without printing values.

## Test plan

- `bun test`
- `bun run build:frontend`
- `cargo test --manifest-path src-tauri/Cargo.toml`
- `cargo check --manifest-path src-tauri/Cargo.toml`
- Search staged changes for token fixtures and suspicious fields:
  `rg -n "access_token|refresh_token|client_secret|Bearer " src src-tauri tests`
  and confirm hits are types/tests/redaction logic only.
- Manually connect a fake token through the Rust test harness, restart the app,
  read metadata, and verify React never receives the token.

## Done criteria

- [x] Connections have provider-neutral, non-secret SQLite metadata.
- [x] OAuth credentials are only in the OS credential store.
- [x] No command or event can serialize credentials to React or logs.
- [x] Canonical provider identity prevents accidental duplicates without
  collapsing distinct tenants or installation modes.
- [x] Scheduled and on-demand refresh share one race-safe service.
- [x] Disconnect warns about dependent workflows, schedules, and triggers.
- [x] The loopback OAuth helper is provider-neutral and tested.
- [x] Connected Apps settings handles normal and failure states.
- [x] Empty-database and upgrade migrations pass.
- [x] Frontend and Rust test/build gates pass.

## Verified at HEAD (2026-08-20, commit `5e62adf`)

Re-run of the plan's own gates and a fresh read of the security boundary:

- `bun run check` passes: 234 frontend tests / 43 files, 1367 assertions, 0
  failures; `tsc && vite build` succeeds (378 modules); 371 Rust tests pass, 0
  failures, 0 ignored.
- **No credential reaches SQLite.** `app_connections`
  (`src-tauri/src/db/schema.sql:139-156`) stores only metadata plus a
  `credential_ref` pointer; there is no token, secret, or payload column.
- **No credential reaches React.** Every `#[tauri::command]` in
  `src-tauri/src/commands/integrations.rs` returns `AppConnectionDto`,
  `AppProviderDto`, `AppConnectionUsage`, `ActionDescriptor`, or a
  provider-specific authorization DTO. The backend record `AppConnection`
  (`src-tauri/src/integrations/models.rs:51`) deliberately has no `Serialize`
  derive, and `AppConnectionDto` omits `credential_ref`, `identity_key`, and
  `provider_metadata`. Serialization tests assert the fixture strings and the
  `credentialRef` / `accessToken` keys never appear
  (`src-tauri/src/commands/integrations.rs:432-456`,
  `src-tauri/src/integrations/models.rs:242-243`).
- **No credential reaches logs.** `CredentialEnvelope`
  (`src-tauri/src/integrations/token_store.rs:17-62`) has a hand-written
  `Debug` that prints `[REDACTED]` for `access_token`, `refresh_token`, and
  `provider_fields`, plus a `Drop` that zeroizes them; covered by
  `debug_output_is_redacted`.
- **No credential reaches workflow JSON.** `AppActionNodeData`
  (`src/features/workflow/types.ts:376-383`) persists only `connectionId`;
  Rust resolves it to a credential at execution time.
- `rg` over `src/` for `access_token|refresh_token|client_secret` returns zero
  hits. The provider paste-token connect forms added after this plan
  (Telegram, Sentry, Notion, Linear) are inbound-only: the value goes straight
  to a Rust command and the form field is cleared — no outbound path was
  introduced.

No regression against the architecture contract was found.

## Release validation still required

Packaged credential-store smoke testing is the only remaining work. It cannot
be executed from a development checkout: each case needs a signed or packaged
bundle on a clean machine of that OS, so it is release-acceptance work rather
than implementation work. Everything else in this plan is verified at `HEAD`.

- [x] macOS debug `.app` bundle compiles with the default capability set.
- [ ] **Packaged Linux build: Secret Service smoke test and restart check.**
  Needs a Linux desktop session with a running Secret Service provider.
  **This plan owns this one.**

### Ownership split (coordinator decision, 2026-08-20)

The signed-macOS and packaged-Windows smoke tests that used to sit here have
**moved to matrix E of
[`plans/release-money/005-run-polar-paid-release-acceptance.md`](release-money/005-run-polar-paid-release-acceptance.md)**
(rows E10 and E11). They are no longer this plan's to run or to track.

| Smoke test | Owner | Why |
| --- | --- | --- |
| Signed/notarized macOS package: create/read/overwrite/delete credential + restart persistence | **Plan 005, matrix E (row E10)** | 005 already mandates clean Apple Silicon and Intel macOS machines and signed/notarized DMGs. Identical setup; running it twice buys nothing. |
| Packaged Windows build: Credential Manager smoke + restart | **Plan 005, matrix E (row E11)** | 005 already mandates a clean Windows 10/11 x64 machine. |
| Packaged Linux build: Secret Service smoke + restart | **This plan** | 005 has **no** Linux environment. Folding it in would silently widen a release-blocking plan's hardware requirements, and a requirement nobody can meet is a requirement that gets waived. |

Nothing was dropped in the move: all three cases still exist, each with exactly
one owner. The credential store is the same `keyring`-backed store licensing
uses, so proving it under a signed macOS identity and a packaged Windows
identity inside 005 covers connected apps on those two platforms too.

Consequence for this plan's status: it is **not** blocked on macOS or Windows
hardware any more. The single remaining gate is a Linux desktop session with a
Secret Service provider.

## STOP conditions

- The selected credential library lacks a viable backend on any shipping OS.
- A provider implementation requires a confidential client secret embedded in
  the desktop binary; move that flow to Plan 011 instead.
- Existing migration ordering or database initialization is unclear.
- Product wants secrets synced between devices; that requires a separate
  encrypted-sync and account design, not an extension of this plan.

## Maintenance notes

- Keychain schemas need versioned credential envelopes for safe rotation.
- Provider-specific scopes belong in provider modules, not the core table.
- Re-test keychain behavior under signed/notarized/package identities, because
  development and production app identities can receive different access.
