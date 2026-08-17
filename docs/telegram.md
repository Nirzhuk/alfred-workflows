# Telegram personal notifications

Alfred supports one local, outbound-only Telegram connection. A workflow can
send a plain-text notification to the private chat paired during setup. Alfred
does not receive Telegram events, monitor replies, track reads, run a Telegram
poller in the background, or queue messages while the app is closed.

## Create and pair a dedicated bot

1. Open `@BotFather` in Telegram and use `/newbot` to create a fresh bot used
   only by Alfred.
2. In Alfred, open **Settings → Connected Apps → Telegram**, paste the token,
   and select **Validate bot**.
3. Select **Open Telegram**. In the private one-to-one chat that opens, press
   **Start**.
4. Return to Alfred, enter or keep the explicit test notification, and select
   **Finish pairing and send test**.

The pairing link contains a random, one-use nonce and expires after ten
minutes. Alfred checks at most three short batches of bot updates for the exact
`/start` message, rejects groups and channels, validates the private chat, and
immediately discards unrelated update content. There is no manual chat-ID
entry.

The bot must be dedicated to Alfred and must not have a webhook. Telegram makes
`getUpdates` and webhooks mutually exclusive, and consuming another app's bot
updates would be unsafe. Alfred rejects an existing webhook and never deletes
or takes it over.

## Sending from workflows

Choose **Send Telegram notification** in a generic Connected App action. The
only action field is an interpolatable `message`. The destination is always the
private chat fixed during pairing.

Messages are plain text and limited to 4,096 Unicode characters after
interpolation. Alfred does not enable markup modes, buttons, media, dynamic
recipients, message splitting, or replies. Sends for the connection are
serialized and limited locally to five per minute and 60 per hour. Excess
sends fail immediately and are not queued.

A successful action means Telegram accepted the request; it does not mean the
recipient saw or read it. If a request times out or the response is lost after
dispatch, Alfred reports `delivery_unknown` and does not retry automatically,
because retrying could create a duplicate notification.

## Local data and revocation

The bot token and full numeric chat ID live together only in Alfred's operating-
system credential entry. SQLite keeps the bot identity, private-chat type,
pairing mode, and a masked destination such as `private chat ••••1234`. The
frontend receives only the bot label and masked destination. Pairing nonces,
Telegram updates, resolved message bodies, and raw API responses are not saved
to connection metadata or action output.

Telegram calls are made directly by the Rust desktop process while Alfred is
running, including tray mode. Restarting Alfred preserves the connection
through the OS credential store; it does not preserve an unfinished pairing
session.

Disconnecting Telegram deletes the OS credential before Alfred removes local
metadata. Alfred cannot revoke a BotFather token remotely. If a token may be
compromised—or if local credential deletion fails—revoke or regenerate it with
`@BotFather`, then remove any stale Alfred metadata or credential-manager entry.

Before release, recheck Telegram Bot API authentication, deep-link, message-
size, update, and rate-limit behavior, and run the packaged pairing/send/restart
smoke test on every shipping operating system.
