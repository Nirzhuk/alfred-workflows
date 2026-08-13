# Plan 014: Add Gmail with staged, least-privilege access

> **Executor instructions**: Ship send-only before requesting read scopes. Plans
> 008 and 009 are required; Plan 010 is required for triggers. Public use of
> sensitive/restricted scopes is blocked on Google's verification outcome.
>
> **Drift check (run first)**: verify the connection/action framework and review
> Google's current native-app OAuth, Gmail scope classification, verification,
> and Pub/Sub requirements immediately before work.

## Status

- **Priority**: P0 send / P1 read-events
- **Effort**: L send / XL verified event integration
- **Risk**: HIGH
- **Depends on**: Plans 008, 009; Plan 010 for events; Plan 011 for push relay
- **Category**: integration
- **Planned at**: 2026-08-11

## Product outcome

Phase 1 connects Google in the system browser and lets workflows send mail.
Phase 2 adds explicit search/read metadata and new-mail triggers only after
scope classification, consent, privacy policy, and Google verification are
ready. This avoids blocking a useful low-scope feature on a much heavier data
access review.

Official references: [OAuth for desktop apps](https://developers.google.com/identity/protocols/oauth2/native-app),
[send message API](https://developers.google.com/workspace/gmail/api/reference/rest/v1/users.messages/send),
[push notifications](https://developers.google.com/workspace/gmail/api/guides/push), and
[OAuth app verification](https://support.google.com/cloud/answer/13464321).

## Commands you will need

- `bun test`
- `bun run build:frontend`
- `cargo test --manifest-path src-tauri/Cargo.toml`
- `cargo check --manifest-path src-tauri/Cargo.toml`
- Run the Google OAuth/Gmail MIME and HTTP fixture suite added by this plan.

## Scope

**In scope**:

- Google desktop OAuth authorization-code + PKCE in the system browser.
- Phase 1 action: send email.
- Phase 2 actions: search/select messages and fetch bounded detail.
- Phase 2 local history polling; later Pub/Sub relay delivery.
- MIME construction, scope gating, refresh/revoke, quota/rate handling,
  verification checklist, redaction, and tests.

**Out of scope**:

- Attachments, bulk mail/marketing, contacts, Drive (Plan 016), mailbox sync,
  background indexing, service accounts/domain-wide delegation, or parsing
  arbitrary remote HTML in the frontend.
- Google's deprecated out-of-band OAuth flow.
- Persisting complete RFC 822 messages or refresh tokens in SQLite.

## Git workflow

Keep send-only, verified read access, local history, and Pub/Sub phases in
separate reviewable changes. Do not merge unverified read scopes into a public
release. Preserve unrelated work and do not commit/push without instruction.

## Implementation steps

### Step 1: Register environments and approve scope phases

Create separate Google Cloud OAuth clients for development and production as
Desktop apps. Configure consent-screen branding, support/developer contacts,
authorized domains/privacy links as required. Decide external vs internal
audience explicitly.

Capability matrix:

- Phase 1: request only identity basics plus `gmail.send` and offline access.
- Phase 2 metadata/search/history: select the narrowest current Gmail scopes
  that actually support the required `messages.list`, metadata, and history
  query behavior. Document why each is needed and its Google classification.
- Never request read scope simply because send is enabled.

**Verify**: security/product owner signs off the scope table and verification
plan; OAuth client contains no web-client secret embedded in desktop config.

### Step 2: Implement native OAuth and credential lifecycle

Run the browser flow in Rust with state + PKCE and a loopback callback on
`127.0.0.1` random port per current Google native-app guidance. Reject OOB/manual
code flows. Validate state, one callback, timeout, and returned account identity.
Store tokens in Plan 008's keychain envelope and metadata separately.

Implement refresh with rotation-safe overwrite, revoked grant handling,
incremental authorization for Phase 2, explicit disconnect/revoke, and Google
test-user/development mode errors. Update the exact Tauri opener allow-list for
Google auth origins only.

**Verify**: tests cover state mismatch/replay, cancel/timeout, refresh rotation,
revocation, scope upgrade, wrong account, and no token in command responses.

### Step 3: Register the send-mail action

Implement `gmail.send_email` through Plan 009 with recipients, CC/BCC, subject,
plain-text body, and optional reply headers only when safely modeled. Construct
RFC 2822/MIME and base64url in Rust. Set size/recipient limits well below Gmail
maximums and reject newline/header injection. HTML and attachments remain out
of v1.

On success return message/thread IDs and a safe summary. Honor 401 refresh,
quota/429 backoff, and stable errors. Treat an ambiguous network timeout as
unknown delivery and do not blind-retry a send.

**Verify**: MIME fixtures cover Unicode, CC/BCC, header injection, base64url,
oversize input, 401/403/429, and ambiguous timeout. Manually send only to a
controlled test mailbox.

### Step 4: Complete Google verification before public read features

Prepare verified domains, homepage/privacy policy, scope justification, demo
video, test instructions, and data-use/deletion description. Determine whether
the selected scopes require sensitive/restricted verification or a security
assessment. Keep Phase 2 behind a build-time/product capability gate until
approval; do not ask users to bypass an unverified-app warning as the release
strategy.

**Verify**: written approval/status and exact approved scopes are recorded in
docs. If assessment cost/timeline is unacceptable, mark Phase 2 deferred and
leave send-only production-safe.

### Step 5: Add explicit read/search actions

After scope approval, register:

- `gmail.search_messages`: bounded Gmail query + label filter, metadata result;
- `gmail.get_message`: explicit message ID, selected headers and bounded plain
  text preview; no attachments/raw MIME by default.

Pagination and queries execute in Rust. Sanitize/normalize MIME parts, do not
render provider HTML, and cap decoded depth/bytes. Action outputs include IDs,
thread ID, from/to, subject, date, labels, and opt-in preview.

**Verify**: nested MIME/multipart fixtures, malformed encoding, huge part,
pagination, missing message, restricted-scope error, and body-redaction tests.

### Step 6: Add local new-mail history polling

After Plan 010, register a `new_mail` event using Gmail history IDs and bounded
message metadata fetches. Store the last successful history ID in trigger
state, advance only after durable receipts, and handle expired history with a
bounded resync that establishes a fresh baseline without replaying the mailbox.

Default event data excludes body; include ID/thread/from/subject/date/labels and
an opt-in bounded preview only. UI says Agentflow must remain open.

**Verify**: tests cover duplicate/out-of-order history, expired history ID,
label filtering, deleted messages, restart, quota backoff, and no raw MIME in DB.

### Step 7: Add Pub/Sub push only through approved relay

Gmail `watch` uses Google Cloud Pub/Sub and must be renewed at least every seven
days according to current guidance. After Plan 011, configure least-privilege
topic/subscription identities, validate Pub/Sub push authentication, route by
mailbox/connection, and treat each notification as a hint to pull history—not
as trusted message content. Schedule renewal daily with alerting.

**Verify**: staging tests cover IAM misconfiguration, forged push, duplicate
notification, watch renewal/expiry, offline queue expiry, and mailbox mismatch.

## Test plan

- `bun test`, frontend build, Rust tests/check.
- OAuth and Gmail HTTP/MIME fixtures with no live credentials in repository.
- Manual send-only flow and revocation.
- After approval, controlled search/new-mail tests and restart checkpoint test.
- Search DB/logs/run JSON for fixture tokens, raw MIME, and full body sentinel.

## Done criteria

- [ ] Send-only works with `gmail.send` and no read scope.
- [ ] Native OAuth uses system browser/PKCE and keychain storage.
- [ ] Public read/events stay gated until Google scope approval is documented.
- [ ] Read outputs/events are bounded and body-minimized.
- [ ] Local history and relay Pub/Sub modes are honestly distinguished.
- [ ] Revoke/disconnect and all tests pass.

## STOP conditions

- The implementation proposes deprecated OOB OAuth or a desktop client secret.
- Google verification/security-assessment requirements are unknown for chosen
  read scopes.
- Product expects bulk/marketing email or attachments in this transactional v1.
- Pub/Sub is proposed without Plan 011 identity, queue, and operations ownership.

## Maintenance notes

- Renew Gmail watches daily and alert well before their expiry.
- Recheck scope classification, verification status, quota, and OAuth policies
  whenever actions expand.
- Keep Gmail and Google Drive scope grants incremental even if they share one
  Google account connection.
