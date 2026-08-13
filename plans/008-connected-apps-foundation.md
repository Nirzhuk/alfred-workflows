# Plan 008: Build the secure Connected Apps foundation

> **Executor instructions**: Follow this plan step by step. Run every
> verification command before moving on. Stop at any STOP condition rather
> than inventing a security or product decision. When complete, update this
> plan's row in `plans/README.md`.
>
> **Drift check (run first)**: this workspace had no Git `HEAD` when planned.
> Run `shasum -a 256 src-tauri/Cargo.toml src-tauri/src/db/schema.sql
> src-tauri/src/db/migrate.rs src-tauri/src/lib.rs
> src/features/settings/components/settings-page/settings-page.tsx
> src/features/workflow/api.ts`. Expected hashes begin respectively with
> `e171b538`, `0d944a32`, `2aa18846`, `d770c27c`, `b29ec0f0`, and `6bde999a`.
> If any differ, re-read the affected file and reconcile this plan first.

## Status

- **Priority**: P0
- **Effort**: L
- **Risk**: HIGH
- **Depends on**: none
- **Category**: architecture / security
- **Planned at**: unversioned snapshot, 2026-08-11

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
- Tauri commands to list/get/disconnect connections and query provider catalog.
- Settings > Connected Apps with empty, connected, expired, error, and
  disconnect states.
- Token redaction and migration/round-trip tests.

**Out of scope**:

- Provider-specific authorization URLs, client IDs, scopes, API calls, events,
  or workflow nodes.
- A cloud service, browser callback endpoint, or Alfred user account.
- Importing secrets from the existing HTTP node or `triggers.secret`.
- A generic “paste OAuth token” field in React.

## Git workflow

This snapshot has no usable `HEAD`. Do not commit or push unless the repository
has since been initialized and the user asks. Keep implementation changes to
this plan's scope, preserve unrelated work, and update only this plan's index
row after all gates pass.

## Implementation steps

### Step 1: Define provider-neutral models and schema

Add an additive migration through the existing mechanism in
`src-tauri/src/db/migrate.rs`. Create `app_connections` with:

- `id TEXT PRIMARY KEY` (random UUID/ULID, never provider account ID);
- `provider_id TEXT NOT NULL` (no SQL CHECK enum; the Rust catalog validates);
- `display_name`, `external_account_id`, and `external_tenant_id`;
- `scopes_json TEXT NOT NULL DEFAULT '[]'`;
- `status TEXT NOT NULL` constrained to `connected|expired|error|revoked`;
- `expires_at`, `last_checked_at`, `last_error_code`, `created_at`, `updated_at`;
- `credential_ref TEXT NOT NULL UNIQUE`.

Do not store an access token, refresh token, authorization code, client secret,
raw provider error, email body, or Slack message in this table. Add repository
CRUD under `src-tauri/src/db/app_connections.rs` and register it in `db/mod.rs`.

**Verify**: migration works on both an empty database and a fixture containing
the existing schema; `PRAGMA table_info(app_connections)` contains no token or
secret column.

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

**Verify**: fake-store tests prove round trip, overwrite, delete, missing item,
and redacted formatting. Perform a manual packaged-app smoke test on each
shipping OS before calling this plan done.

### Step 3: Expose redacted Tauri commands

Add commands under `src-tauri/src/commands/integrations.rs` and register them in
`commands/mod.rs`/`lib.rs`:

- `list_app_providers`
- `list_app_connections`
- `get_app_connection`
- `disconnect_app_connection`

DTOs may contain provider/account labels, scope names, expiry, and status. They
must contain no token, credential payload, PKCE value, client secret, or
provider raw response. Normalize backend errors into stable codes such as
`credential_store_locked`, `credential_missing`, and `disconnect_failed`.

**Verify**: serialize every command response in a Rust test and assert known
secret fixture strings do not appear.

### Step 4: Add the frontend API/store and Settings surface

Add integration types/API/state under `src/features/integrations/`. Render a
Connected Apps card in the existing settings page style. Provider catalog rows
show provider name, short capability summary, connection status, account label,
and Connect/Manage/Disconnect actions. Connect remains disabled or marked
“Coming next” until the provider plan implements authorization.

Show actionable credential-store errors without surfacing raw backend details.
Disconnect requires confirmation and removes the item from UI only after the
backend succeeds.

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
  `rg -n "access_token|refresh_token|client_secret|Bearer " src tests`
  and confirm hits are types/tests/redaction logic only.
- Manually connect a fake token through the Rust test harness, restart the app,
  read metadata, and verify React never receives the token.

## Done criteria

- [ ] Connections have provider-neutral, non-secret SQLite metadata.
- [ ] OAuth credentials are only in the OS credential store.
- [ ] No command or event can serialize credentials to React or logs.
- [ ] Connected Apps settings handles normal and failure states.
- [ ] Empty-database and upgrade migrations pass.
- [ ] Frontend and Rust test/build gates pass.

## STOP conditions

- The selected credential library lacks a viable backend on any shipping OS.
- A provider implementation requires a client secret embedded in the desktop
  binary; move that flow to Plan 011 instead.
- Existing migration ordering or database initialization is unclear.
- Product wants secrets synced between devices; that requires a separate
  encrypted-sync and account design, not an extension of this plan.

## Maintenance notes

- Keychain schemas need versioned credential envelopes for safe rotation.
- Provider-specific scopes belong in provider modules, not the core table.
- Re-test keychain behavior under signed/notarized/package identities, because
  development and production app identities can receive different access.
