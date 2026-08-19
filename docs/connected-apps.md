# Connected Apps security and recovery

Alfred's Connected Apps foundation keeps authorization credentials outside the
workflow database. Provider integrations run in the Rust desktop process;
credentials are never returned to the React interface or written into workflow
JSON.

## What is stored where

SQLite contains only non-secret connection metadata: the provider and
connection mode, display/account/tenant labels, an opaque canonical identity
digest, granted scope names, health state and timestamps, and an opaque
credential reference. This lets Alfred show status and find workflows that use
a connection without reading a token.

The operating-system credential store contains a versioned envelope with the
access token, optional refresh token and expiry, and the minimum
provider-specific fields required for refresh. Alfred uses the service name
`com.alfred.connected-apps` and a random opaque reference as the account key.
Authorization codes, PKCE verifiers, OAuth state and nonce values exist only in
memory for the lifetime of one authorization attempt.

Provider setup and capability details:

- [Slack](slack.md)
- [GitHub](github.md)
- [Gmail](gmail.md)
- [Microsoft 365 mail and calendar](microsoft.md)
- [Telegram personal notifications](telegram.md)
- [Notion and the knowledge-source boundary](knowledge-connectors.md)

The provider-specific plans add authorization endpoints and API calls. The
foundation does not embed confidential OAuth client secrets or provide a field
for pasting OAuth tokens into the web interface.

## Disconnect and remote revoke

Settings > Connected Apps shows workflows, schedules, and triggers that depend
on a connection before disconnecting. Alfred then:

1. marks the local connection revoked;
2. deletes its system credential; and
3. deletes the SQLite metadata only after credential deletion succeeds.

If the credential store is locked or the entry is already missing, the revoked
metadata remains visible. The user may retry after unlocking the credential
store or explicitly choose **Remove local data**. Metadata-only cleanup does not
remove a stale credential and does not revoke the grant at the remote provider.

Deleting Alfred's SQLite database also does not revoke remote grants. Revoke
the application in the provider's account/security settings whenever the local
database or application is removed unexpectedly.

## Linux prerequisite

Linux requires an available, unlocked Secret Service implementation on the
desktop session's D-Bus. Common implementations include GNOME Keyring and the
KDE wallet Secret Service backend. On a minimal/window-manager installation,
install and start a compatible service before connecting an app. Alfred reports
a stable `credential_store_locked` error when the service is unavailable; it
does not fall back to plaintext files.

## Recovering stale entries

Use the operating system's credential manager to find entries for the
`com.alfred.connected-apps` service. Delete an entry without copying, printing,
or logging its value. If several opaque entries exist and the stale one cannot
be identified safely, do not guess: disconnect and remotely revoke all Alfred
connections first, remove all entries for that exact service, and reconnect the
ones still needed. Finally, use **Remove local data** in Alfred if revoked
metadata remains.

Keychain access can differ between development and signed/package identities.
Release testing must verify create, read, overwrite, and delete behavior in the
packaged app on every shipping operating system.

## Adding a provider

Every new provider must have a stable snake_case catalog ID, a narrow
capability summary, explicit connection modes, and a dedicated recovery path.
Build provider calls in the Rust process; validate credentials before storage
and persist them only in the operating-system credential store. React state,
workflow JSON, logs, errors, descriptors, and report output must contain only
redacted metadata. Add the provider's scopes, disconnect dependencies, and
reconnect/revoke behavior to automated tests before enabling Connect.

Every catalog provider also needs an entry in `APP_LOGOS` and an optimized SVG
in `src/assets/apps/`. The UI must not fetch brand images at runtime or embed
Logo.dev keys. Import the local SVG with `?no-inline`; this keeps it out of the
JavaScript bundle and maintains offline operation. SVGs must contain no
scripts, raster images, or remote references, and must be at most 5 KiB raw.

Transparent artwork is the default. Recolor an insufficiently legible mark for
Alfred's light and dark themes when possible (Sentry is the example). Set
`requiresSurface: true` only when a mark cannot meet a 3:1 non-text contrast
ratio safely without altering its identity; GitHub, Linear, and Notion are the
current exceptions. Unknown stored/future providers must retain the accessible
initial fallback until a reviewed local logo is added.
