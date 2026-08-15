# Slack connected app

Alfred currently supports an advanced, local-only Slack connection for actions.
It is not the public Alfred bot and it does not receive events while Alfred is
closed.

## Private workspace setup

1. Create a Slack app in the workspace you control.
2. Add the bot scopes `chat:write` and `channels:read`.
3. Add `groups:read` only if private-channel selectors are needed.
4. Install or reinstall the app after changing scopes.
5. Copy the `xoxb-` bot token into **Settings → Connected Apps → Slack**.

The token is submitted directly to Rust, validated with Slack `auth.test`, and
stored in the operating-system credential store. It is cleared from the React
form after every attempt. SQLite retains only the workspace/bot identity,
connection mode, and granted scopes.

The connection provides two generic app actions:

- **Send Slack message**: select an accessible conversation and send bounded
  mrkdwn text.
- **Reply in Slack thread**: additionally supply the parent message timestamp.

The selector stores only channel IDs and names. Action results contain only the
channel ID, message timestamp, and permalink when Slack provides one. Alfred
does not accept arbitrary Block Kit JSON, read message history, or automatically
retry a send whose delivery became ambiguous.

## Capabilities not enabled yet

- Native Slack PKCE would send actions as the signed-in user, not as a bot. The
  product decision in `docs/adr/012-slack-connection-modes.md` remains pending.
- Local `app_mention` requires Socket Mode, an `xapp-` token, and a shared
  connection lifecycle. It is not exposed by the current UI.
- Public branded bot installation, HTTP Events API, token rotation, and offline
  delivery remain blocked on approval and implementation of ADR 011.
- Incoming Webhook URLs are validated syntactically by the backend but are not
  accepted until Alfred can verify and retain their workspace identity.

## Data and revocation

Slack message text is sent to Slack at action execution time and remains subject
to normal local workflow/run history. Tokens and raw Slack responses are not
written to workflow JSON or run output.

Disconnect the connection in Connected Apps to delete its credential and local
metadata. To revoke it independently, remove the private app from Slack's app
management UI; then remove the stale Alfred connection locally.

Recheck Slack scopes, method rate limits, and token policy before every release.

