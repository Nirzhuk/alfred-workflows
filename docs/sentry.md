# Sentry incident connector (auth token)

Alfred's Sentry connector reads issue alerts and updates issue status in the
projects an auth token can access. The current mode uses a user-owned auth
token; public OAuth and webhook installation arrive with the cloud-relay
plan.

## Connection mode

Open **Settings → Connected Apps → Sentry**. The auth-token mode is labeled
Advanced. Create the token in **Sentry → Settings → User Auth Tokens** with
the narrowest scopes:

- `org:read` and `project:read` — list organizations and projects;
- `event:read` — read issue summaries and watch issue activity;
- `event:write` — optional; enables the update-status action.

Alfred validates the token, aggregates the scopes of every organization the
token can see, and refuses to connect when `event:read` is missing. The token
is stored only in the OS credential store and never returned to React,
written into SQLite, embedded in workflow JSON, or logged.

## Sensitive-data boundary

Stack traces, request data, breadcrumbs, and user context are sensitive.
Alfred never fetches or persists them by default. The get-issue action
returns issue metadata and a bounded latest-event summary (type, title, and a
truncated message value); raw event entries are stripped before results are
released to a workflow. Secrets Sentry scrubs are not persisted anywhere by
Alfred and are never treated as safe.

## Actions

Add an **App Action** step and choose Sentry:

- **Get Sentry issue** accepts a selected project and an issue short ID like
  `BACKEND-123` (or a numeric issue ID) and returns bounded metadata.
- **Update Sentry issue status** resolves, ignores, or unresolves an issue.
  Ignoring accepts an explicit duration: forever, 1 hour, 1 day, or 1 week.

Projects persist opaque project IDs. Every issue reference is verified to
belong to the selected project before any action runs, so one connection can
never cross project boundaries by accident.

## Events

Add an **App trigger** and choose **Sentry issue alert**. Local polling
checks the selected project's recent issues roughly every minute and starts a
workflow when an issue is created, resolved, regressed, or updated.
Connecting a trigger establishes "now" and does not replay history.

Events carry project and issue IDs, short ID, level, status, and a bounded
title. Stack traces and event payloads are never included.

## Rate limits and pagination

Alfred honors `Retry-After` on 429 responses and keeps polling well under
Sentry's per-organization API-token limits. Project listings are cached
briefly; issue lists are bounded to the most recent 100 issues per poll.

## Disconnect and revoke

Disconnecting removes the local credential and metadata. Revoke or rotate
the token in **Sentry → Settings → User Auth Tokens** whenever the local app
or database is removed unexpectedly.
