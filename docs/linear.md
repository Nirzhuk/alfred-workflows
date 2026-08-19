# Linear connected app (personal API key)

Alfred's Linear connected app creates issues, comments on issues, updates
issue status, fetches bounded issue context, and can start workflows from team
issue activity. The current mode uses a user-owned personal API key; public
one-workspace OAuth and relay webhooks arrive with the cloud-relay plan.

## Connection mode

Open **Settings → Connected Apps → Linear**. The personal-key mode is labeled
Advanced: the key acts as your Linear user in the selected workspace and has
the same rights you do there. Alfred requests nothing beyond what the key
already grants and never escalates workspace roles.

Alfred validates the key by reading the authenticated viewer and workspace
identity, then stores it only in the OS credential store. The key is never
returned to React, written into SQLite, embedded in workflow JSON, or logged.

## Actions

Add an **App Action** step and choose Linear:

- **Create Linear issue** accepts a selected team, title, bounded
  description, priority, optional assignee, and optional label names resolved
  against the team's labels.
- **Comment on Linear issue** adds a Markdown comment to a selected issue.
- **Update Linear issue status** moves a selected issue to a workflow state
  of its team.
- **Get Linear issue** returns bounded, explicitly untrusted issue context
  (title, state, assignee, labels, project, priority, and a truncated
  description preview).

Team, assignee, state, and issue fields persist opaque Linear IDs, never
names. Alfred appends an internal marker to descriptions and comments it
creates so polling never triggers workflows on its own output.

## Events

Add an **App trigger** and choose **Linear issue activity**. Local polling
checks the selected team's recently updated issues roughly every minute and
starts a workflow when an issue is created or updated. Connecting a trigger
establishes "now" and does not replay history.

Comment events require relay webhooks and are not delivered in local polling
mode. Events carry issue IDs, identifiers, status, and a bounded title; full
descriptions and comment bodies are never included and are only available
through an explicit fetch action.

## Rate limits and pagination

Linear personal keys share a 5,000-request hourly quota and complexity-based
GraphQL limits. Alfred paginates resource selectors, honors `Retry-After`, and
maps GraphQL `errors` arrays even when the HTTP status is 200. Resource
selector options are fetched on demand and cached briefly.

## Disconnect and revoke

Disconnecting removes the local credential and metadata. Because the key is
personal, revoke or rotate it in **Linear → Settings → API** whenever the
local app or database is removed unexpectedly.

## Failure modes

- `linear_token_invalid` — the key is malformed or Linear rejected it.
- `linear_identity_invalid` — Linear returned no valid workspace identity.
- `rate_limited` — the shared quota or complexity budget is exhausted;
  retry later.
- `scope_missing` — the key lacks permission for the requested team or issue.
