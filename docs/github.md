# GitHub connected app

Alfred's GitHub connected app creates issues and pull requests, comments on an
issue or PR, fetches bounded issue context, and can start workflows from
repository activity. It is separate from the existing Git Host workflow node,
which continues to use the `gh` command-line tool.

## Publisher configuration

Register a GitHub App for Alfred with device flow enabled. Enable expiring user
access tokens and grant only these repository permissions:

- Metadata: read
- Issues: read and write
- Pull requests: read and write

Do not grant Contents, Administration, organization-wide administration, or
Git access. Local polling does not require a webhook URL. Configure the app to
be installable on the intended accounts and let each installer select the
repositories Alfred may use.

The build consumes public, non-secret configuration:

```sh
ALFRED_GITHUB_APP_CLIENT_ID=Iv1.example \
ALFRED_GITHUB_APP_INSTALL_URL=https://github.com/apps/example/installations/new \
bun run tauri build
```

`ALFRED_GITHUB_APP_CLIENT_ID` enables the Connect button.
`ALFRED_GITHUB_APP_INSTALL_URL` is optional but recommended so the setup modal
can take users to repository selection. Only an HTTPS `github.com/apps/...`
URL is accepted.

## Connecting

Open **Settings → Connected Apps → GitHub**. Select the repositories for the
GitHub App, open GitHub's device page, enter the one-time code, and approve the
request. Organization installations can require owner approval. For an
organization using SAML SSO, start an active SSO session before authorizing.

GitHub's opaque `device_code` exists only in the Rust process for the short
authorization attempt; the modal receives only the human-entered one-time
`user_code`. Access and refresh tokens go directly to the OS credential store
and are never returned to React, stored in SQLite, written into a workflow, or
printed through `gh`.

## Actions

Add an **App Action** step and choose GitHub:

- **Create GitHub issue** accepts a selected repository, title, bounded body,
  labels, and assignees.
- **Comment on GitHub issue or PR** accepts a selected repository, number, and
  bounded comment.
- **Create GitHub pull request** opens a PR for existing head/base branches; it
  never pushes a branch or source code.
- **Get GitHub issue or PR** returns bounded, explicitly untrusted context.

Repository fields persist a numeric repository ID. At runtime Rust resolves it
through the authorized GitHub connection before constructing an API path.
Provider response bodies and workflow input content are not written to logs.

GitHub does not offer an idempotency key for these create operations. If Alfred
reports that delivery is unknown, inspect the repository before retrying so a
duplicate issue, comment, or pull request is not created.

## Triggers and latency

Connected-app triggers support issue, issue-comment, pull-request, and review
activity. They poll only while Alfred is open, including in the tray. GitHub's
repository Events API is not real-time and GitHub documents latency ranging
from about 30 seconds to six hours, so these triggers are appropriate for local
convenience rather than urgent incident delivery.

The first poll establishes the current cursor and does not replay old activity.
Only normalized metadata and a preview of at most 1,000 characters enter the
workflow. Alfred-authored bodies carry a hidden source marker and are ignored
by these triggers to prevent recursive comment loops.

Public webhook delivery and execution while the desktop is closed require the
approved cloud relay from Plan 011 and are not part of this connector.

## Disconnecting and revocation

Disconnecting in Alfred marks the connection revoked, removes its OS credential,
and deletes its metadata after credential cleanup succeeds. To revoke access
remotely, also remove Alfred under **GitHub Settings → Applications → Authorized
GitHub Apps** or uninstall/reconfigure the GitHub App for the account. An
organization owner may need to perform the installation change.
