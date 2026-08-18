# ADR 015: GitHub App device authorization for the desktop connector

- **Status:** Accepted for Plan 015 Stage A local mode
- **Date:** 2026-08-17
- **Related plans:** 008, 009, 010, 015

## Decision

The GitHub connected app uses a GitHub App user access token obtained with
GitHub's OAuth device flow. Alfred embeds only the app's public client ID. It
does not embed a client secret or GitHub App private key and it does not read
credentials managed by the `gh` CLI.

The publisher-owned GitHub App must request only these repository permissions:

- Metadata: read;
- Issues: read and write; and
- Pull requests: read and write.

The installer selects individual repositories or explicitly chooses all
repositories. API access is the intersection of that installation selection,
the app permissions, and the authorizing user's permissions. Alfred discovers
repositories through the user-access-token installation endpoints and persists
their numeric IDs in workflow action and trigger configuration. It resolves the
ID again in Rust before every operation.

Device authorization sessions and GitHub's opaque `device_code` remain in Rust
memory and expire after at most 15 minutes. Only the short, human-entered
`user_code` is returned to the setup modal. User and refresh tokens are stored
only in the OS credential store. Expiring user tokens and rotation are
supported without a client secret because the original grant used device flow.

## Existing `gh` workflows

The persisted `gitHost` node and its runner branch are unchanged. Those nodes
continue to delegate to the user's separately authenticated `gh` executable.
The GitHub connected app is available only through generic `appAction` nodes
and connected-app triggers. Alfred never calls `gh auth token`, reads GitHub CLI
credential files, or migrates an existing Git Host node.

## Organization and SSO behavior

Organization owners can require approval before installing the GitHub App. An
organization using SAML SSO can require the user to start an active SAML
session before authorizing. A repository absent from the installation resource
selector is unavailable to Alfred; it is never accepted as an owner/name string
or inferred from local Git configuration.

## Events

Local mode polls GitHub's repository Events API every 60 seconds while Alfred
is running. GitHub documents that this API is not real-time and can lag from
roughly 30 seconds to six hours. The first poll establishes a cursor without
replaying history. Subsequent events are normalized to IDs, action, issue/PR
number, actor, title/status, URL, and a bounded preview. Raw payloads are never
stored in workflow runs.

Bodies created by Alfred include an invisible source marker. Matching issue,
comment, pull-request, and review events are ignored so an Alfred-authored
comment cannot recursively trigger another workflow. Public webhooks remain
blocked on Plan 011.

## Consequences

- A distributable build needs an approved GitHub App registration and public
  client ID, but no confidential GitHub credential.
- Actions are attributed to the authorizing GitHub user and remain bounded by
  that user's access.
- Create endpoints do not provide an idempotency key. Ambiguous transport
  failure is reported as `delivery_unknown`; users must inspect the target
  repository before retrying.
- Local disconnect removes Alfred's token but cannot uninstall the GitHub App
  from an organization. Users must also revoke authorization or uninstall it
  in GitHub when remote revocation is required.
