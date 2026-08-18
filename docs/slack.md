# Slack connected app

Alfred currently supports an advanced, local-only Slack connection for actions
and `app_mention` triggers over Socket Mode. It is not the public Alfred bot,
and mention triggers run only while Alfred is open (including tray mode).

## Private workspace setup

1. Create a Slack app in the workspace you control.
2. Add the bot scopes `chat:write` and `channels:read`.
3. Add `groups:read` only if private-channel selectors are needed.
4. To receive mentions, also add `app_mentions:read`, subscribe to the
   `app_mention` bot event, and enable Socket Mode.
5. Generate an app-level token with `connections:write` for Socket Mode.
6. Install or reinstall the app after changing bot scopes.
7. Enter the `xoxb-` bot token and, for mentions, the `xapp-` app-level token in
   **Settings → Connected Apps → Slack**.

The tokens are submitted directly to Rust. Alfred validates the bot token with
Slack `auth.test` and validates the app-level token by requesting a temporary
Socket Mode URL. Both are stored in one operating-system credential entry and
cleared from the React form after every attempt. SQLite retains only workspace,
enterprise (when present), and bot identity metadata, connection mode, and
granted capabilities.

The connection provides two generic app actions:

- **Send Slack message**: select an accessible conversation and send bounded
  mrkdwn text.
- **Reply in Slack thread**: additionally supply the parent message timestamp.

The selector stores only channel IDs and names. Action results contain only the
channel ID, message timestamp, and permalink when Slack provides one. Alfred
does not accept arbitrary Block Kit JSON, read message history, or automatically
retry a send whose delivery became ambiguous.

It also registers **Connected app → Slack → App mention**. An optional channel
filter is available. Alfred maintains one WebSocket per Slack installation,
acknowledges envelopes before processing them, reconnects when Slack refreshes
the URL, and fans each accepted event into Plan 010's per-trigger durable
receipt queue.

Only the event ID, workspace/channel/user IDs, message and thread timestamps,
an optional permalink, and a bounded text preview enter workflow data. Blocks,
attachments, headers, wrapper tokens, and raw event bodies do not. Edited,
unsupported-subtype, and bot-authored mention events are acknowledged but
ignored to prevent loops.

## Capabilities not enabled yet

- Native Slack PKCE would send actions as the signed-in user, not as a bot. The
  product decision in `docs/adr/012-slack-connection-modes.md` remains pending.
- Public branded bot installation, HTTP Events API, token rotation, and offline
  delivery remain blocked on approval and implementation of ADR 011.
- Incoming Webhook URLs are validated syntactically by the backend but are not
  accepted until Alfred can verify and retain their workspace identity.

## Data and revocation

Slack message text is sent to Slack at action execution time and remains subject
to normal local workflow/run history. A bounded mention preview is stored as
untrusted workflow-event data. Tokens and raw Slack responses/events are not
written to workflow JSON or run output.

Disconnect the connection in Connected Apps to delete its credential and local
metadata. To revoke it independently, remove the private app from Slack's app
management UI; then remove the stale Alfred connection locally.

Recheck Slack scopes, method rate limits, and token policy before every release.
