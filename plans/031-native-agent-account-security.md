# Plan 031: Add secure native-agent accounts and credential lifecycle

> **Executor instructions**: This plan creates the credential boundary used by
> the Alfred harness. It must not import or scrape credentials owned by Claude,
> Codex, Cursor, OpenCode, Copilot, Gemini, or Grok CLIs. Stop on any ambiguity
> about token custody, provider client registration, or revocation semantics.
> Update `plans/README.md` only after all gates pass.
>
> **Drift check (run first)**: re-read `src-tauri/src/integrations/oauth_native.rs`,
> `token_store.rs`, `refresh.rs`, `db/schema.sql`, `db/migrate.rs`, and the
> connected-app settings/store. Reuse tested primitives, but keep native-agent
> account metadata separate from Connected Apps metadata.

## Status

- **Priority**: P0
- **Effort**: L
- **Risk**: CRITICAL
- **Depends on**: Plan 030
- **Category**: agent authentication / security
- **Planned at**: 2026-08-24
- **Implementation**: FOUNDATION DONE; managed-product contract cutover
  implemented 2026-08-26 with focused verification pending

## Goal

Give the `alfred` harness a provider-neutral account lifecycle without making
provider tokens part of workflow graphs, React state, SQLite payloads, logs, or
run events.

Native-agent accounts are not Connected Apps and are not CLI credentials.

## Architecture contract

Account identity has four separate dimensions: provider, stable product,
runtime, and billing. Provider/product values are registry-validated Rust
strings, not SQL enums. The stable product registry is:

```text
claude_code_subscription  claude_api
chatgpt_codex             openai_api
opencode_go               opencode_zen
cursor_cloud              github_copilot_subscription
gemini_api                grok_api
```

Use three data classes:

1. **SQLite metadata**: opaque account ID, provider, product, harness, display
   identity, auth method, scopes/capabilities, optional managed runtime id and
   version, opaque runtime profile reference, billing source/owner,
   entitlement observation, status, expiry, last error, and an optional secret
   credential reference.
2. **OS credential store**: access token, refresh token, provider secret fields,
   and provider-owned account tokens.
3. **Memory only**: authorization code, state, PKCE verifier, nonce, exchanged
   token response, and refresh material before persistence.

Recommended service namespace:

```text
com.alfred.agent-harness
```

Do not reuse the Connected Apps service key blindly. A native-agent token and a
Slack/Gmail/Microsoft connection have different lifecycle, revocation, and
provider-account semantics.

Recommended metadata table: `agent_accounts`. It must not contain token or
secret columns. Use a random opaque account ID as the primary key and a unique
provider/harness/identity key only after provider identity validation.

A provider may declare one of these custody modes:

- `alfred_managed`: Alfred stores and refreshes the credential.
- `runtime_managed`: a bundled provider runtime owns the credential in an
  isolated app-specific home; Alfred receives only redacted account state.

The custody mode must be explicit in provider registration and diagnostics.

Managed subscription accounts resolve through a managed runtime profile and
must not have a fake secret reference. Direct API/PAYG products may have a
secret reference; a managed PAYG server product can require both a profile and
its separate secret. Neither `runtime_profile_ref` nor `credential_ref` may
cross command DTOs, React state, workflow graphs, diagnostics, or logs.

Entitlement is an observation, not billing identity. Store one of `unknown`,
`eligible`, `limited`, `exhausted`, or `ineligible`, plus a source and an RFC
3339 observation timestamp when the state is not `unknown`. Never infer
entitlement from a credential's existence or silently switch products after
exhaustion.

## Scope

**In scope**:

- Transactional `agent_accounts` contract rebuild and repository.
- Agent-specific credential envelope/store namespace.
- Redacted account DTOs and Tauri commands.
- Native authorization session lifecycle.
- Refresh/revoke/disconnect state machine.
- Provider registration seam for auth handlers.
- Settings surface for native accounts.
- Security and redaction tests.

**Out of scope**:

- Provider-specific OAuth URLs or token exchanges.
- CLI credential import.
- A generic paste-token field as the default UX.
- Alfred cloud identity or token relay.
- Automatic account sharing across users/devices.

## Implementation steps

### Step 1: Define account metadata and migration

Rebuild `agent_accounts` transactionally for the new contract, preserving
legacy rows and retry-safe cleanup references, with fields equivalent to:

```text
id
provider_id
product_id
harness
identity_key
display_name
external_account_id
external_workspace_id
auth_method
custody_mode
managed_runtime_id
managed_runtime_version
runtime_profile_ref
scopes_json
billing_source
billing_owner
entitlement_state
entitlement_source
entitlement_observed_at
status
expires_at
last_checked_at
last_error_code
credential_ref (nullable)
created_at
updated_at
```

Do not add SQL provider or product enums. Rust registries validate known
values; SQL may enforce only structural invariants and the closed entitlement
state set.
Do not store access tokens, refresh tokens, authorization codes, raw ID tokens,
client secrets, or raw provider responses.

Add repository CRUD, reconnect-upgrade behavior, and distinct-account tests.

**Verify**: empty and existing database migrations; schema inspection confirms
no token/secret fields; duplicate identity rules are deterministic.

### Step 2: Add agent credential storage

Reuse the secure store behavior from `integrations/token_store.rs`, but use the
agent-harness service namespace and account-scoped credential references.

The envelope must support:

- version;
- access token or runtime credential reference;
- optional refresh token;
- expiry;
- provider fields needed for refresh only;
- custody mode.

Debug formatting and serialization must redact every secret field. Zeroize
intermediate buffers and rotated values where the existing store permits.

**Verify**: round-trip, overwrite, delete, missing, malformed, wrong-version,
redacted-debug, and zeroization-oriented tests.

### Step 3: Add authorization-session management

Create a provider-neutral in-memory authorization attempt registry with:

- opaque attempt ID;
- provider/harness;
- expiry;
- cancellation flag;
- provider-owned context;
- no persistence of authorization code or PKCE verifier.

Native public-client providers may reuse `oauth_native.rs`. Providers that own
their own authorization server/runtime flow must return a validated, redacted
completion result through the same lifecycle.

Commands should include equivalents of:

- `list_agent_accounts`
- `start_agent_authorization`
- `complete_agent_authorization`
- `cancel_agent_authorization`
- `refresh_agent_account`
- `disconnect_agent_account`

The frontend must never receive tokens or raw authorization responses.

**Verify**: timeout, cancellation, duplicate completion, state mismatch,
provider mismatch, and app restart behavior are explicit and safe.

### Step 4: Register refresh and revocation handlers

Extend or reuse the existing refresh-service pattern with an agent-account
registration seam. Serialize refresh per account. Persist rotated credentials
before marking the account connected.

Classify errors as:

- retryable provider/network failure;
- terminal revoked/invalid grant;
- credential-store failure;
- unsupported auth mode;
- provider policy denial.

Disconnect must delete provider credentials and metadata in an explicit order.
If deletion partially fails, surface an actionable recovery state rather than
claiming the account is disconnected.

**Verify**: concurrent refresh, rotation, 401 recovery, terminal revoke,
credential-store lock, and retryable failure fixtures.

### Step 5: Add native-account settings UI

Add a Native Agents section separate from Connected Apps. Show:

- provider and account label;
- harness/provider mode;
- auth method;
- connected/expired/error/revoked state;
- expiry and last refresh when safe;
- Connect, Reconnect, Refresh, Disconnect;
- provider-specific explanation when native support is gated.

Do not show raw token values, provider raw errors, account cookies, or CLI
credential locations.

## Subagent-ready ownership slices

- **Schema/repository**: migration, metadata CRUD, identity rules.
- **Credential boundary**: envelope, store namespace, redaction, zeroization.
- **Authorization lifecycle**: attempt registry, commands, cancellation.
- **Frontend**: account settings/store and safe error states.
- **Security verification**: secret scans, serialization tests, concurrency and
  failure fixtures.

The integration owner freezes account/status enums before provider plans begin.

## STOP conditions

- A provider requires Alfred to ship a confidential client secret.
- The only available route is scraping a CLI credential store.
- A runtime-managed provider credential cannot be isolated from unrelated Alfred
  data.
- Token or raw authorization response would cross the Tauri/UI boundary.
- Revocation semantics are unknown and the UI would claim the account is gone.
- A provider asks for broad scopes without a documented capability need.

## Verification

```bash
bun test src/features/integrations
bun run build:frontend
cargo test --locked --manifest-path src-tauri/Cargo.toml integrations db
cargo check --locked --manifest-path src-tauri/Cargo.toml
```

Also inspect staged source for credential leakage and verify all command DTOs are
redacted. Run the full repository gate before marking done.

## Done criteria

- [x] Native accounts have separate redacted metadata and secret storage.
- [x] No CLI credential import or scraping exists.
- [x] Authorization attempts are memory-only and cancellable.
- [x] Refresh/revoke/disconnect states are explicit.
- [x] Settings exposes safe native account lifecycle.
- [x] Provider plans can register auth and refresh handlers without editing the
      workflow runner.
- [ ] Re-run focused and full verification after the managed-product cutover.
