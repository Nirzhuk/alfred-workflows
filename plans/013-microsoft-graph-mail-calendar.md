# Plan 013: Add Outlook Mail and Calendar through Microsoft Graph

> **Executor instructions**: Implement native desktop authorization and actions
> first. Events require Plan 010; Graph webhooks require Plan 011. Use only
> current Microsoft identity/Graph documentation when finalizing scopes and
> endpoints.
>
> **Drift check (run first)**: confirm Plans 008/009 postconditions and inspect
> `src-tauri/capabilities/default.json` before changing URL permissions. At plan
> time opener URL allow-listing only named `x-apple.systempreferences:*`.

## Status

- **Priority**: P0
- **Effort**: XL
- **Risk**: HIGH
- **Depends on**: Plans 008, 009; Plan 010 for events; Plan 011 for webhooks
- **Category**: integration
- **Planned at**: 2026-08-11

## Product outcome

Users connect a Microsoft work/school or personal account in the system
browser, then workflows can send email, select recent messages safely, and
create calendar events. A later event phase can trigger on new mail/calendar
changes while Agentflow is open; relay webhooks add offline delivery.

Use delegated Microsoft Graph permissions on behalf of the signed-in user, not
application-wide mailbox permissions. Copilot itself is handled separately in
Plan 017.

Official references: [delegated OAuth authorization-code flow](https://learn.microsoft.com/en-us/graph/auth-v2-user),
[Graph mail API](https://learn.microsoft.com/en-us/graph/api/resources/mail-api-overview?view=graph-rest-1.0),
[list messages](https://learn.microsoft.com/en-us/graph/api/user-list-messages?view=graph-rest-1.0), and
[change-notification webhooks](https://learn.microsoft.com/en-us/graph/change-notifications-delivery-webhooks).

## Commands you will need

- `bun test`
- `bun run build:frontend`
- `cargo test --manifest-path src-tauri/Cargo.toml`
- `cargo check --manifest-path src-tauri/Cargo.toml`
- Run the Microsoft OAuth/Graph fixture and contract suite added by this plan.

## Scope

**In scope**:

- Microsoft Entra public-client registration and system-browser PKCE flow.
- Tenant policy (`common`, organization-only, or explicit tenant) documented and
  configurable before release.
- Actions: send mail, list/select recent mail metadata, create calendar event.
- Optional fetch-message-detail action with explicit configuration and limits.
- Local delta/poll events and later relay webhooks.
- Consent, reconnect, revoke, pagination, throttling, redaction, and tests.

**Out of scope**:

- Application permissions, shared mailbox/admin consent in v1, Exchange EWS,
  mailbox synchronization, attachments, contact sync, Teams chat, or Copilot.
- Persisting full mail bodies or auth responses in runs.
- A client secret in the desktop public client.

## Git workflow

Land native auth, actions, local events, and relay webhooks in separate
reviewable changes. Do not mix Copilot work into this provider. Preserve
unrelated work and do not commit/push without instruction; update the plan row
after the shipped phases pass all gates.

## Implementation steps

### Step 1: Register a Microsoft public client and scope matrix

Create separate dev/staging/prod Entra registrations. Configure a native/public
client redirect supported by Microsoft's current desktop guidance—prefer a
short-lived loopback callback bound to `127.0.0.1` on a random port with PKCE.
Use the system browser, never an embedded webview.

Start with incremental capability scopes:

- identity: `openid`, `profile`, `offline_access`, and the minimal identity
  permission needed to label the account;
- send: `Mail.Send`;
- mail selector/trigger: choose `Mail.ReadBasic` if it supports all required
  fields, otherwise document why `Mail.Read` is necessary;
- calendar creation: `Calendars.ReadWrite` (or the narrowest current equivalent).

Do not request mail-read/calendar-write permissions until the user enables
those capabilities. Record tenant/account IDs and granted scopes as metadata.

**Verify**: registration review confirms public-client mode, exact production
redirects, no wildcard redirect, no desktop client secret, and least-scope
mapping tests.

### Step 2: Implement system-browser OAuth with PKCE

Generate state, nonce, verifier/challenge, and callback listener in Rust. Bind
the listener only to loopback, accept one callback, validate state/nonce, and
expire the attempt quickly. Open the exact Microsoft authorization origin via
the Tauri opener allow-list; never allow arbitrary URLs.

Exchange/refresh tokens in Rust and store through Plan 008. Handle user cancel,
port collision, timeout, consent-required, admin-policy, MFA/Conditional Access,
expired refresh, and account/tenant mismatch with stable errors. Reconnect may
upgrade scopes without creating a duplicate connection.

**Verify**: OAuth unit tests plus manual personal and organization tenant tests;
state mismatch, reused callback, malicious redirect, and cancelled browser do
not create a connection.

### Step 3: Register Graph mail/calendar actions

Implement through Plan 009:

- `microsoft.send_mail`: recipients, subject, plain-text body; HTML is opt-in
  and sanitized/clearly labeled;
- `microsoft.list_recent_mail`: folder/filter, bounded result count, metadata
  only by default;
- `microsoft.get_mail`: explicit message ID, bounded body preview, no attachments;
- `microsoft.create_calendar_event`: calendar, subject, start/end/timezone,
  location, attendees, optional description.

Validate email addresses, recipient/result limits, timezones, end-after-start,
and body size in Rust. Use Graph pagination; select only needed fields. Treat
ambiguous send/create timeouts as “result unknown” unless a safe idempotency
mechanism is used. Normalize outputs to IDs, web links, timestamps, and summary.

**Verify**: mocked Graph tests cover pagination, 401 refresh, 403 consent/admin
policy, 429 `Retry-After`, malformed mail/calendar input, DST/timezone, partial
responses, ambiguous timeout, and secret/body redaction.

### Step 4: Add local events with delta/checkpoints

After Plan 010, register `new_mail` and optionally `calendar_event_changed`.
Prefer Graph delta queries/checkpoint links over repeatedly listing entire
folders. Keep opaque delta links in app trigger state, not workflow JSON. Apply
allow-list filters (folder, sender, subject contains) after fetching minimum
metadata. New-mail normalized payload contains message ID, sender display/
address, subject, received timestamp, web link, and bounded preview only if the
user explicitly enabled it.

Poll only while Agentflow runs and explain this in UI. Advance delta checkpoint
only after receipt persistence. Handle reset/expired delta tokens with a bounded
resync that does not replay the whole mailbox.

**Verify**: fixtures cover duplicate pages, deleted/moved message, checkpoint
reset, reconnect, rate limit, and no body persistence by default.

### Step 5: Add optional Graph webhook delivery through the relay

After Plan 011, create/renew subscriptions from the provider adapter, route
Graph validation tokens exactly per current docs, validate client state, map
tenant/subscription to a paired device, and dedupe notifications. Fetch details
with the desktop-held token after delivery unless the relay ADR explicitly
approves another custody model. Renew well before expiry and surface failures.

**Verify**: staging validation handshake, renewal, lifecycle notification,
wrong client state, duplicate notification, tenant mismatch, offline expiry,
and revoked subscription tests.

### Step 6: Document enterprise readiness

Document scopes, admin-consent expectations, supported account types, tenant
selection, Conditional Access behavior, data retained, revocation, and how
local vs relay events behave. Include an admin-facing permission table.

## Test plan

- `bun test`, frontend build, Rust tests/check.
- Mock Graph API/error fixtures and OAuth negative cases.
- Manual personal Microsoft account and test-tenant work account.
- Send to a controlled inbox; create event across DST boundary; inspect output.
- Search SQLite/logs/run JSON for refresh tokens and full fixture bodies.

## Done criteria

- [ ] Native PKCE works without a desktop client secret.
- [ ] Permissions are incremental and capability-specific.
- [ ] Mail/calendar actions run through generic `appAction`.
- [ ] Full bodies/attachments are excluded unless explicitly fetched and bounded.
- [ ] Local events disclose open-app behavior; relay webhooks are separately gated.
- [ ] Disconnect/reconnect and enterprise policy failures are actionable.

## STOP conditions

- Registration requires a confidential-client secret in the desktop.
- Product has not chosen supported account/tenant policy.
- `Mail.Read`/broad calendar permissions are needed without a reviewed user
  benefit and consent explanation.
- Shared mailbox/application permission requirements appear; create a separate
  enterprise plan instead of broadening delegated v1.

## Maintenance notes

- Revalidate Graph permissions, subscription lifetimes, and national-cloud
  endpoints before each release.
- Keep Microsoft connection usable for both mail/calendar without conflating it
  with Plan 017's Copilot-facing Entra identity.
