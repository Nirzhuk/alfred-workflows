# Plan 021: Send personal notifications through Telegram

> **Executor instructions**: Ship this plan before Plan 023. Use Telegram's
> official Bot API and the generic Connected Apps action framework. V1 is
> outbound-only, supports one dedicated user-owned bot and one paired private
> chat, and never starts background polling. Do not add app events, a relay,
> shared-bot support, webhook takeover, or manual chat-ID entry.
>
> **Drift check (run first)**: confirm Plans 008 and 009 postconditions and
> inspect the current provider catalog, generic app-action registry, token
> store, connection DTOs, Slack private-connect flow, and Tauri URL
> capabilities. Preserve overlapping Connected Apps changes.

## Status

- **Priority**: P1
- **Effort**: M
- **Risk**: MEDIUM
- **Depends on**: Plans 008 and 009
- **Does not depend on**: Plan 010 events or Plan 011 relay
- **Category**: integration
- **Planned at**: 2026-08-13
- **Revised at**: 2026-08-16 after separating WhatsApp into Plan 023
- **Implementation status**: DONE (implementation, automated gates, and macOS
  package build green; live-bot smoke confirmed by the maintainer 2026-08-18)

## Product decisions

- The use case is an Alfred workflow sending a personal notification to the
  person who configured Alfred.
- V1 is outbound-only. It does not react to incoming Telegram messages or
  track delivery/read status.
- Each Alfred installation supports exactly one Telegram connection in V1.
- The user creates a fresh BotFather bot dedicated to Alfred. Shared bots and
  bots with an existing webhook are rejected.
- Setup pairs the bot to exactly one private chat through a short-lived,
  nonce-bearing `/start` deep link. There is no manual chat-ID path.
- A workflow supplies only one interpolatable plain-text `message`. The fixed
  chat ID is never a workflow input.
- The configured message template remains part of workflow JSON like every
  generic app action. The resolved/interpolated body is not added to run
  output, logs, analytics, connection metadata, or any second persistence path.
- The final message is limited to 4,096 Unicode characters after interpolation.
  Media, buttons, markup modes, and automatic message splitting are excluded.
- Sends are serialized and locally limited to a burst of five per minute and
  60 per hour. Excess sends fail with `rate_limited`; they are not queued.
- A connection is not ready until an explicit test notification succeeds.
- A timeout after request dispatch is `delivery_unknown`; Alfred never retries
  an ambiguous send automatically.
- The bot token and full chat ID live only in the OS credential envelope.
  SQLite and frontend DTOs contain masked identity information only.

## Product outcome

A user pastes a dedicated BotFather token, opens a Telegram deep link, presses
**Start**, and returns to Alfred to finish pairing. Alfred resolves the private
chat without asking for a numeric identifier, sends an explicit test
notification, and then exposes `telegram.send_personal_message` through the
generic `appAction` node.

The action reports that Telegram accepted the message. It never claims that a
person saw or read it.

Official references:

- [Telegram bots introduction](https://core.telegram.org/bots)
- [Telegram bot features and deep links](https://core.telegram.org/bots/features)
- [Telegram Bot API](https://core.telegram.org/bots/api)

## Scope

**In scope**:

- An action-only `telegram` connected-app provider with `private_bot` mode.
- One dedicated bot and one fixed private-chat recipient per installation.
- Backend-owned token validation and short-lived nonce pairing.
- `telegram.send_personal_message` with one interpolatable message field.
- Mandatory test send, masked connection identity, local throttling, bounded
  errors and responses, redaction, disconnect guidance, documentation, and
  deterministic contract tests.

**Out of scope**:

- Incoming-message triggers, runtime polling, webhooks, background sockets,
  delivery/read tracking, and offline queues.
- Shared bots, existing-webhook bots, webhook deletion or takeover, manual
  chat-ID input, groups, channels, and multiple Telegram connections.
- Dynamic recipients, contact lookup, broadcasts, replies, message history,
  Markdown/HTML mode, link-preview configuration, media, files, buttons,
  locations, and message splitting.
- An Alfred-owned bot, provider token, or cloud relay.

## Commands you will need

- `bun test`
- `bun run build:frontend`
- `cargo test --manifest-path src-tauri/Cargo.toml`
- `cargo check --manifest-path src-tauri/Cargo.toml`
- The Telegram mock/contract suite introduced by this plan.

## Implementation steps

### Step 1: Register the provider and tighten shared action errors

Add `telegram` to `ProviderCatalog`, not `AgentProviderId`, with connection mode
`private_bot`, action capability copy, and no event descriptors. Enforce the
one-connection limit in Rust, not only in the UI.

Before registering the action, make the shared `delivery_unknown` message
provider-neutral. It currently names Slack and would produce a false Telegram
error. Provider-specific recovery details may be added safely without exposing
request bodies or credentials.

Use the existing connection boundaries:

| Surface | Telegram data |
|---|---|
| Public/redacted DTO | bot display name/username and masked destination |
| Backend-only SQLite metadata | bot ID, private chat type, pairing mode, recipient mask |
| OS credential envelope | bot token and full fixed chat ID |

Build the canonical identity from provider, mode, bot ID, and chat ID before
discarding the plaintext chat ID from setup input. Store only the digest in the
identity column. Never put a token or full chat ID in SQLite, workflow JSON, or
frontend state beyond the active password form.

**Verify**: provider-catalog, one-connection, DTO serialization, and repository
scan tests prove the provider is action-only and routing secrets cannot cross
the command boundary.

### Step 2: Build bounded one-time pairing

The setup flow is:

1. Show that a fresh BotFather bot dedicated to Alfred is required.
2. Collect its token in a password field and clear the field on success,
   failure, cancellation, modal close, and unmount.
3. Rust validates the token with `getMe` and verifies `getWebhookInfo.url` is
   empty. Reject any existing webhook with dedicated-bot guidance.
4. Rust creates a cryptographically random base64url pairing nonce within
   Telegram's deep-link limit. Keep token and nonce in an opaque in-memory
   setup session for at most ten minutes.
5. After an explicit click, open
   `https://t.me/<bot_username>?start=<nonce>` and ask the user to press
   **Start**.
6. On **Finish pairing**, perform bounded short `getUpdates` requests, match the
   exact `/start <nonce>` message, and require a private chat. Fail closed on
   zero or multiple matches.
7. Immediately discard all update content not required for the match. Do not
   start a timer, background poller, or persistent update loop.
8. Validate the selected chat with `getChat` and show only a masked destination.
9. Require an explicit test message. Save the credential and metadata only
   after Telegram accepts the test.

Pairing sessions expire on timeout, modal close, process restart, or successful
completion. Nonces are one-use and never logged or persisted.

**Verify**: mock tests cover invalid/revoked token, non-bot identity, existing
webhook, nonce mismatch/replay, ambiguous match, group/channel rejection,
expired session, bounded `getUpdates`, test-send failure or ambiguity,
credential-store failure, and token/chat/update-content redaction.

### Step 3: Register `telegram.send_personal_message`

Register through Plan 009:

- `telegram.send_personal_message`
  - `message`: required textarea, interpolation enabled.

The descriptor has no recipient/chat field. After interpolation, Rust counts
Unicode characters and rejects empty or greater-than-4,096 messages. Use
`sendMessage` without `parse_mode`; do not add automatic chunking or a detached
retry task.

The executor loads token and fixed chat ID through the backend capability,
serializes sends per connection, and applies an in-memory token bucket:

- maximum burst: five sends per minute;
- rolling cap: 60 sends per hour;
- no persistent overflow queue.

Return only schema version, Telegram message ID, accepted timestamp, and masked
destination. Do not echo message content or raw Telegram JSON. A definitive
pre-dispatch failure may be retried by a later workflow attempt; a timeout or
connection loss after dispatch maps to `delivery_unknown` and is never retried
automatically.

**Verify**: fixtures cover success, blocked bot, chat not found, bot removed,
empty/oversized message, both local rate limits, 429 retry metadata, 5xx,
malformed/oversized response, cancellation, ambiguous timeout, concurrency
serialization, and complete token/message/chat-ID redaction.

### Step 4: Integrate setup and connection UI

Add a Telegram connect modal and route it through a small provider-to-connect
handler registry rather than spreading provider `if` branches. Disable Connect
when a Telegram connection already exists.

The row copy is: “Send plain-text notifications to your paired private chat.”
Show bot identity, masked destination, readiness, and reconnect state. In the
app-action form, the selected connection is the only destination choice.

On disconnect, delete the OS credential before metadata using Plan 008.
Explain that Alfred cannot revoke the BotFather token remotely; the user must
revoke/regenerate it through BotFather if needed.

**Verify**: frontend tests cover dedicated-bot copy, password behavior, pairing
states, masked destination, one-connection enforcement, mandatory test send,
reconnect/disconnect, no recipient field, and absence of event/trigger options.

### Step 5: Document and smoke-test the integration

Add `docs/telegram.md` and link it from Connected Apps documentation. Cover
BotFather setup, `/start` pairing, why a dedicated non-webhook bot is required,
what Alfred stores, local-only execution, throttling, ambiguous delivery,
disconnect, token revocation, and the absence of inbound features.

Manually validate in a packaged build on every shipping OS:

1. Pair a fresh dedicated bot.
2. Complete the mandatory test.
3. Send from a workflow and a schedule while Alfred is running in tray mode.
4. Restart Alfred and send again.
5. Exercise a disconnected/revoked bot.
6. Disconnect and verify the keychain entry and local metadata are gone.

## Done criteria

- [x] Telegram appears as a connected app, not a coding agent.
- [x] Exactly one dedicated, non-webhook bot can be paired per installation.
- [x] Pairing uses a one-use nonce and only accepts one private chat.
- [x] A successful explicit test is required before readiness.
- [x] The generic action accepts only one interpolatable 4,096-character
      plain-text message and exposes no recipient.
- [x] Sends are serialized and capped at five per minute and 60 per hour.
- [x] No inbound event, background polling, queue, group, channel, manual ID,
      shared-bot, media, or markup capability is added.
- [x] Outputs distinguish accepted from delivered/read and never echo content.
- [x] Tokens, full chat IDs, nonces, updates, resolved message bodies, and raw
      responses do not leak through DTOs, connection metadata, logs, analytics,
      errors, or outputs. Only the intentional workflow message template is
      persisted in workflow JSON.
- [ ] Automated tests and packaged manual gates pass.

## STOP conditions

- Product requires a shared bot, webhook takeover, manual chat ID, group,
  channel, inbound message, delivery tracking, cloud relay, or offline queue.
- A recipient field is added to workflow configuration.
- Pairing starts a background poller or can consume updates from a non-dedicated
  bot.
- A token, chat ID, nonce, update, resolved message body, or raw provider
  response enters connection SQLite, frontend DTOs, workflow output, logs, or
  analytics. The configured workflow message template is the sole intended
  persistence exception.
- Current Bot API deep-link, message-size, rate-limit, or authentication rules
  have not been rechecked immediately before implementation.

## Maintenance notes

- Re-check Telegram Bot API limits and error behavior before each release.
- Keep pairing dependencies isolated from runtime sending.
- Treat multiple accounts, incoming messages, groups/channels, richer content,
  managed bots, and relayed/offline delivery as separate plans.
