# Gmail connected app (Plan 014, Phase 1: send-only)

Alfred's Gmail connected app lets workflows send plain-text email from one
connected Gmail account. It uses Google's native-app OAuth authorization-code
flow with S256 PKCE and a loopback callback on `127.0.0.1`. Only the public
OAuth client ID is compiled into the desktop; no web-client secret exists in
Alfred. Tokens are stored in the OS credential store and never returned to
React or written into SQLite.

## Publisher configuration

Register a Google Cloud OAuth client of type **Desktop app**. Configure the
consent screen with branding, support/developer contacts, and privacy links.
Authorize only these scopes for Phase 1:

- `openid`
- `email`
- `profile`
- `https://www.googleapis.com/auth/gmail.send`

Never add `gmail.readonly`, `gmail.modify`, or broader read scopes in Phase 1.
Read/search and new-mail triggers are a separately verified phase and are not
enabled by this build.

The build consumes public, non-secret configuration:

```sh
ALFRED_GMAIL_CLIENT_ID=1234567890-example.apps.googleusercontent.com \
bun run tauri build
```

`ALFRED_GMAIL_CLIENT_ID` enables the Connect button and is the capability gate
that keeps the send-only phase behind Google verification (Plan 014 Step 4).
Optionally set `ALFRED_GMAIL_OAUTH_PORT` to a fixed loopback port and register
`http://127.0.0.1:<port>/oauth/callback` as the client's redirect URI;
otherwise Alfred binds a random port per attempt.

## Verification gate

Public use of `gmail.send` requires Google's sensitive-scope verification.
Until that approval is recorded and a client ID ships in the distribution
build, the provider stays hidden. Do not ship a public build that asks users
to bypass an unverified-app warning.

## Connecting

Open **Settings → Connected Apps → Gmail**. Alfred opens Google in the system
browser with the exact authorization origin; the loopback callback validates
state and the `access_type=offline` grant must return a refresh token.
Google must return a verified account identity before the connection is saved.

Installed-app OAuth does not support incremental authorization: a future
read-access phase must reauthorize the same account for the full approved
union of scopes or create a deliberately separate connection.

## Actions

Add an **App Action** step and choose Gmail:

- **Send Gmail message** accepts To/CC/BCC recipients, a subject, and a
  plain-text body. HTML and attachments are not part of this phase.

Recipient counts, address length, subject length, and body size are bounded
below Gmail maximums, and newline/header injection is rejected. Messages are
assembled as RFC 2822 MIME in Rust and posted base64url-encoded. An ambiguous
network failure after submit is reported as unknown delivery and is never
blind-retried; check the account's Sent folder before sending again.

## Disconnecting and revocation

Disconnecting in Alfred removes the OS credential and connection metadata. To
revoke remotely, also remove Alfred under
**Google Account → Security → Third-party apps**.
