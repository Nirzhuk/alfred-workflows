# Microsoft 365 connected app (Plan 013)

Alfred's Microsoft connected app lets workflows send Outlook email, select
recent message metadata, and create calendar events for one signed-in Microsoft
account. It uses Entra ID's native-app OAuth authorization-code flow with S256
PKCE and a loopback callback on `127.0.0.1`. Only the public Entra client ID is
compiled into the desktop; there is no desktop client secret. Tokens are stored
in the OS credential store and never returned to React or written into SQLite.

## Publisher configuration

Register a Microsoft Entra **public client / native** app. Do not create a
confidential client or embed a client secret in Alfred. Configure the redirect
as a loopback URI on `127.0.0.1` (a random port per attempt, or a fixed port
when `ALFRED_MICROSOFT_OAUTH_PORT` is set). Use the system browser, never an
embedded webview.

The build consumes public, non-secret configuration:

```sh
ALFRED_MICROSOFT_CLIENT_ID=00000000-0000-0000-0000-000000000000 \
ALFRED_MICROSOFT_TENANT=common \
bun run tauri build
```

`ALFRED_MICROSOFT_CLIENT_ID` enables the Connect button. Optional
`ALFRED_MICROSOFT_TENANT` is one of `common` (default; personal and
work/school), `organizations`, `consumers`, or a tenant GUID.
`ALFRED_MICROSOFT_OAUTH_PORT` pins the loopback port when the Entra app
requires a registered redirect such as `http://127.0.0.1:<port>/oauth/callback`.

## Tenant and account policy

| Build setting | Who can connect |
| --- | --- |
| `common` (default) | Personal Microsoft accounts and work/school accounts |
| `organizations` | Work/school only. Personal accounts fail with `microsoft_personal_account_blocked`. |
| `consumers` | Personal accounts only. Work/school accounts fail with `microsoft_work_account_blocked`. |
| tenant GUID | That tenant only. Other tenants fail with `microsoft_account_mismatch`. |

Reconnect upgrades scopes only when the canonical Microsoft identity (`tid` +
`oid`) matches the existing connection.

## Incremental permissions

Identity is always requested. Mail and calendar permissions are added only when
the user enables those capabilities in Settings:

| Delegated permission | Why Alfred requests it | When requested |
| --- | --- | --- |
| `openid`, `profile`, `offline_access` | ID token and refresh | Always |
| `User.Read` | Label the signed-in account | Always |
| `Mail.Send` | Send mail from workflows | User enables Send |
| `Mail.ReadBasic` | List/get metadata and `bodyPreview` | User enables Read |
| `Calendars.ReadWrite` | Create calendar events | User enables Calendar |

`Mail.Read` is not requested. Message bodies and attachments are never stored.
HTML send opt-in escapes tags so they are not executed.

Admin consent may still be required by tenant policy for some of these
delegated permissions. Alfred cannot grant admin consent itself; the user or an
admin must approve the public client in Entra.

## Connecting

Open **Settings → Connected Apps → Microsoft 365**. Alfred opens
`https://login.microsoftonline.com` in the system browser with the exact
authorization origin. The loopback callback validates `state`. After code
exchange, Alfred validates the ID token signature, issuer, audience, expiry,
and nonce before saving the connection.

Conditional Access, MFA, and device compliance complete in the system browser.
Alfred does not bypass those policies. A blocked public client or expired
refresh surfaces a stable reconnection error.

## Actions

Add an **App Action** step and choose Microsoft 365:

- **Send Outlook email** (`microsoft.send_mail`): To, subject, and body.
  HTML is opt-in and escaped.
- **List recent Outlook mail** (`microsoft.list_recent_mail`): folder/filter
  and a bounded metadata list.
- **Get Outlook message** (`microsoft.get_mail`): explicit message ID and a
  bounded preview. No attachments.
- **Create Outlook calendar event** (`microsoft.create_calendar_event`):
  calendar, subject, start/end, IANA timezone, optional location, attendees,
  and description.

Recipient counts, address length, subject length, body size, and time windows
are validated in Rust. Ambiguous send/create timeouts are reported as unknown
delivery and are never blind-retried.

## Local events

`microsoft.new_mail` and `microsoft.calendar_event_changed` poll Graph delta
queries **while Alfred is open**. The first poll stores a checkpoint and does
not replay the mailbox. An expired delta token triggers a bounded resync
without replaying history. Opaque delta links stay in trigger state, not
workflow JSON. Graph change-notification webhooks are a later relay phase
(Plan 011) and are not enabled by this connection.

New-mail payloads include message ID, sender display/address, subject,
received timestamp, web link, and a bounded preview only when the trigger
explicitly enables it.

## Disconnecting and revocation

Disconnecting in Alfred removes the OS credential and connection metadata. To
revoke remotely, also remove Alfred under
**Microsoft account → Privacy → Apps and services** or the Entra enterprise
application for the tenant.

## Data retained

SQLite stores only redacted connection metadata: display name, account/tenant
IDs, granted scope names, and an opaque credential reference. Access and
refresh tokens stay in the OS credential store. Authorization codes, PKCE
verifiers, OAuth state, and nonce values exist only in memory for one
authorization attempt.
