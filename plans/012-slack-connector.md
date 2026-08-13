# Plan 012: Add the Slack connected app

> **Executor instructions**: Ship this in phases. Plans 008 and 009 are hard
> dependencies. Plan 010 is required for events. A branded bot installation and
> HTTP Events API are blocked on Plan 011. Slack's current native PKCE can power
> a user-token action mode, but desktop redirects cannot request bot scopes; do
> not hide that product difference or embed a Slack client secret.
>
> **Drift check (run first)**: verify the provider catalog, action registry, and
> (for events) app-event runtime postconditions. Review current Slack OAuth,
> scopes, rate limits, token rotation, and Events API docs immediately before
> implementation because Slack platform rules change.

## Status

- **Priority**: P0
- **Effort**: L local MVP / XL public distribution
- **Risk**: HIGH
- **Depends on**: Plans 008, 009; Plan 010 for events; Plan 011 for public bot OAuth
- **Category**: integration
- **Planned at**: 2026-08-11

## Product outcome

V1 lets a workflow send a channel message or thread reply and react to an
`app_mention`. There are three deliberately different connection modes:

1. **Developer/private workspace mode**: an advanced user creates their own
   Slack app and supplies bot/app tokens through a backend-owned secret form.
   Socket Mode avoids a public inbound endpoint while Alfred is open.
2. **Native user action mode**: Slack PKCE authorizes user scopes through a
   localhost loopback redirect without a client secret. Messages act with the
   user's identity; bot scopes and `app_mention` events are unavailable.
3. **Public Alfred bot app**: normal workspace bot installation, token
   rotation, and HTTP event delivery through Plan 011. This is the complete
   branded bot/event mode.

Do not present private mode or native user mode as the complete bot integration.

Official references: [OAuth v2](https://api.slack.com/authentication/oauth-v2),
[desktop PKCE](https://docs.slack.dev/authentication/using-pkce/),
[Socket Mode](https://api.slack.com/apis/connections/socket),
[incoming webhooks](https://api.slack.com/messaging/webhooks), and
[token rotation](https://api.slack.com/authentication/rotation).

## Commands you will need

- `bun test`
- `bun run build:frontend`
- `cargo test --manifest-path src-tauri/Cargo.toml`
- `cargo check --manifest-path src-tauri/Cargo.toml`
- Run the provider-specific mock/contract suite introduced by this plan.

## Scope

**In scope**:

- Slack provider catalog/connection health/disconnect.
- Actions: list accessible conversations, send message, reply in thread.
- Event: `app_mention`; optionally explicit channel-message events after scope
  and product review.
- Private BYO-app, native PKCE user-action, and public bot OAuth phases.
- Slack formatting/limits, rate limits, pagination, retries, and redaction.

**Out of scope**:

- Reading all workspace history, DMs, user-directory sync, file upload, slash
  commands, admin APIs, or impersonating a user.
- Using generic HTTP-node headers for Slack tokens.
- Persisting entire message/thread content in run history.

## Git workflow

Implement private actions, local events, and public OAuth/relay phases as
separate reviewable changes. Preserve prerequisite framework code and unrelated
work; do not commit/push without instruction. Update the plan row only when the
phases claimed as the shipped scope have passed their gates.

## Implementation steps

### Step 1: Register the provider and minimum scopes

Add `slack` to the connected-app catalog, not `AgentProviderId`. Define separate
capability scope sets so send-only connections do not request read/event access.
The final scope list must be generated from Slack's current method/event docs;
at minimum expect `chat:write`, a conversation-read scope for selectors, and
`app_mentions:read` for mention events. Do not request broad history/DM/admin
scopes by default.

Store Slack team ID/name, enterprise ID if present, bot user ID, granted scopes,
expiry/rotation metadata, and installation mode as non-secret metadata. Tokens
and app-level token remain in Plan 008's credential store.

**Verify**: scope tests prove each action/event declares exactly its required
capabilities and a send-only connection cannot enable mention events.

### Step 2: Implement private BYO-app connection

Add an explicitly advanced setup flow with instructions generated from the
current Slack app manifest requirements. Collect bot token and, for Socket
Mode, app-level token via a password field sent directly to a Rust command and
discarded from React state after success. Never save it in localStorage,
workflow JSON, SQLite, analytics, or errors.

Validate tokens with an authenticated identity call before storing. Record the
returned team/bot metadata. Offer a send-only Incoming Webhook connection only
as a simpler action-only option; store the webhook URL as a secret and never
display it after save.

**Verify**: bad/revoked/wrong-token types produce stable errors; memory/UI are
cleared after submission; source, DB, logs, and run events contain no token.

### Step 3: Evaluate and implement native PKCE user-action mode

Before choosing the default Slack connection UX, record a product decision on
whether messages sent as the signed-in user are acceptable. Current Slack PKCE
supports public desktop/mobile clients and custom URI or localhost redirects,
but desktop redirects cannot request bot scopes. V1 deliberately chooses the
Plan 008 loopback helper; a later custom-URI flow requires a separate Tauri
deep-link/single-instance lifecycle design. Enabling PKCE is a one-way app
setting without Slack support, and current PKCE refresh tokens have a 30-day
expiry, so refresh health and proactive reconnect UX are required.

If approved, use Plan 008's loopback OAuth helper for state, verifier, callback,
and timeout handling; supply Slack's authorization configuration and keep token
exchange/error mapping in the provider. Exchange without `client_secret`,
request only the user scopes needed by actions, and label the connection/actions
“Send as you.” Register refresh with Plan 008's shared service. Do not enable
`app_mention`, Socket Mode bot events, or claim bot identity for this connection.
If user-token behavior is not acceptable, document the rejection and proceed
from private mode to the relay-backed bot mode.

**Verify**: loopback interception/hijack attempts, state mismatch, token
rotation, refresh-token expiry/reconnect, revoked scopes, and user-vs-bot
capability tests.

### Step 4: Register Slack actions

Implement through Plan 009:

- `slack.send_message`: conversation selector + text;
- `slack.reply_in_thread`: conversation + thread timestamp + text.

List conversations server-side with pagination/search and a short bounded
cache containing IDs/names only. Validate Slack text/block limits. Start with
plain mrkdwn text; do not accept arbitrary Block Kit JSON in v1. Return message
timestamp/channel/permalink where available, not the token or raw response.

Honor `Retry-After`, request at most one on-demand auth recovery through Plan
008's shared refresh service, and map Slack errors to framework codes. Do not
retry a non-idempotent send after an ambiguous network timeout unless Slack
offers a safe idempotency mechanism for that method; report “delivery unknown.”

**Verify**: mocked HTTP tests cover pagination, channel not found, missing
scope, rate limit, unauthorized, timeout ambiguity, output normalization, and
secret redaction.

### Step 5: Add local `app_mention` via Socket Mode

After Plan 010, register an `app_mention` event descriptor. The Slack adapter
maintains one Socket Mode connection per Slack installation while at least one
enabled trigger needs it. Acknowledge envelopes within Slack's deadline, dedupe
by envelope/event ID, reconnect with jitter, and checkpoint receipts before run
enqueueing.

Normalize only team/channel/user IDs, event timestamp, thread timestamp,
permalink, and a bounded text preview. Treat Slack text as untrusted input. The
UI states clearly that local Socket Mode events work only while Alfred runs.

**Verify**: fixture tests cover duplicate envelope, reconnect, ack, edited/
deleted/unsupported subtypes, bot-loop prevention, and payload minimization.

### Step 6: Add public bot OAuth and relay event delivery

After Plan 011 is production-ready, create the Alfred Slack bot app. The relay
generates/binds OAuth state, receives the HTTPS redirect, exchanges the code
with the server-held client secret, and transfers/stores the grant according to
the approved relay ADR. Support token rotation if enabled. Implement uninstall/
revocation cleanup.

Configure HTTP Events API to the relay, verify Slack signatures and timestamp
freshness before enqueue, handle URL verification, dedupe event IDs, and route
by installation/team. Do not send Slack signing/client secrets to desktop.

**Verify**: staging install/uninstall/reinstall, two workspaces, revoked scope,
forged/old signature, duplicate event, offline queue expiry, and tenant-routing
tests all pass.

### Step 7: Add operational/user documentation

Document private setup, “Send as you” PKCE, and one-click bot OAuth separately. Include
requested scopes, data retained, how to revoke, local/offline behavior, bot-loop
rules, and workspace-admin approval expectations. Prepare Slack Marketplace
materials only after public mode security review; do not promise approval.

## Test plan

- Framework frontend/Rust test gates.
- Slack Web API and Socket Mode fixture tests with sanitized official payloads.
- Manual private workspace: connect, send, reply, mention, restart, disconnect.
- Public staging: OAuth + event while desktop online/offline + uninstall.
- Search logs/SQLite/run JSON for `xoxb-`, `xapp-`, webhook URLs, and fixture text
  excluded by normalization.

## Done criteria

- [ ] Slack is a connected app, not an agent provider or HTTP secret field.
- [ ] Send/reply use one generic app-action node.
- [ ] Mention events are deduplicated, bounded, and loop-safe.
- [ ] Private, native user, and public bot setup modes are honestly labeled.
- [ ] Public OAuth client/signing secrets exist only in relay secret storage.
- [ ] Disconnect/revoke works and all tests pass.

## STOP conditions

- Public bot OAuth is requested before Plan 011 security/identity approval.
- Any implementation embeds `client_secret`, signing secret, `xoxb`, or `xapp`
  values in frontend/binary/config/workflow JSON.
- Required Slack scopes expand to DM/history/admin access without product review.
- Marketplace distribution requires a material architecture change; document it
  as a follow-up instead of silently broadening v1.

## Maintenance notes

- Recheck Slack scope/method rate tiers and token-rotation policy each release.
- Keep Socket Mode and HTTP Events adapters behind the same normalized event.
- Monitor app uninstall/revocation and purge metadata/queued events promptly.
