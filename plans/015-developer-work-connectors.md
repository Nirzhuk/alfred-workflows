# Plan 015: Add GitHub, Linear, and Sentry in priority order

> **Executor instructions**: This is a staged provider rollout, not permission
> to implement all three simultaneously. Complete the shared framework first,
> ship and validate each stage, then reassess demand before starting the next.
> Do not remove or silently change the existing `gh`-CLI Git Host node.
>
> **Drift check (run first)**: confirm Plans 008/009 and, for triggers, 010.
> Re-read `GitHostNodeData` and the `"gitHost"` runner path to preserve current
> workflows. Review each provider's current official auth, scope, API, webhook,
> and rate-limit documentation before implementing its stage.

## Status

- **Priority**: P1 GitHub / P1 Linear / P2 Sentry
- **Effort**: XL total
- **Risk**: HIGH
- **Depends on**: Plans 008, 009; Plan 010 for events; Plan 011 for public OAuth/webhooks
- **Category**: integration roadmap
- **Planned at**: 2026-08-11
- **Implementation status**: IN PROGRESS — All three local modes are
  implemented and automated gates pass: GitHub App device authorization,
  repository-scoped actions, and local event polling (Stage A); Linear
  personal-API-key actions and issue polling (Stage B); Sentry auth-token
  issue actions and alert polling (Stage C). Remaining before the public
  go/no-go: a publisher-owned GitHub App client ID and live packaged smoke,
  and measured event-delivery demand. Public Linear/Sentry OAuth and relay
  webhooks stay gated on Plan 011; local events are polling-only and Sentry
  stack traces are never fetched.

## Why these apps

For a coding-workflow product, these close the most valuable operational loop:

- **GitHub**: code/repository system of record; create issues/PRs and react to
  review or issue activity.
- **Linear**: focused product/engineering work tracking; turn workflow findings
  into assigned issues and react to status changes.
- **Sentry**: production incident signal; trigger investigation workflows and
  update issue state after a fix.

GitHub and Linear are the initial “real feature” integrations. Sentry follows
only after event delivery proves reliable.

## Commands you will need

- `bun test`
- `bun run build:frontend`
- `cargo test --manifest-path src-tauri/Cargo.toml`
- `cargo check --manifest-path src-tauri/Cargo.toml`
- Run the conformance and provider contract suite after every provider stage.

## Scope

All actions use Plan 009's generic `appAction`; all events use Plan 010's
normalized payload. Credentials are Plan 008 connections. Public confidential
OAuth and webhook ingress use Plan 011. Provider modules own pagination,
rate-limit handling, scope checks, and normalized errors.

Never put repository, issue, stack-trace, or comment content in logs. Event
payloads contain IDs/URLs/titles/status and a bounded preview, with explicit
fetch actions for details.

**Out of scope**: replacing local Git operations, pushing branches, broad
organization administration, arbitrary GraphQL, full source-code sync, and
automatic collection of Sentry request/local-variable data.

## Git workflow

Use one reviewable change series per provider and preserve the existing `gh`
node. Stop between GitHub, Linear, and Sentry for the stated go/no-go review.
Do not commit/push without instruction and update the index only for completed
stages accurately represented by the status.

## Implementation steps

## Stage A: GitHub connected app

### A1: Preserve the existing CLI path and choose auth

The current Git Host node delegates to the user's authenticated `gh` CLI. Keep
its node type and saved workflows working. Add a new GitHub connected app only
for descriptor-driven actions/events.

Evaluate current GitHub App device authorization vs OAuth device flow for a
desktop public client. Prefer a flow that does not embed a client secret and can
request installation/repository-specific access. If minimum permissions cannot
be achieved without a relay-hosted GitHub App private key/client secret, use
Plan 011. A local advanced mode may explicitly reuse `gh` as a credential
broker, but must never scrape its credential files or print `gh auth token` to
logs/React.

**Verify**: auth ADR documents repository selection, org approval, token expiry,
SSO handling, and how existing CLI-node users are unaffected.

### A2: Register GitHub actions

Initial actions:

- `github.create_issue`: repository, title, bounded body, labels/assignees;
- `github.comment_on_issue`: repository, issue/PR number, bounded body;
- `github.create_pull_request`: repository, existing head/base, title/body,
  draft flag—this action does not push code;
- optional `github.get_issue_or_pull_request` for bounded context.

Resource selectors list only installations/repositories visible to the grant.
Validate owner/repo and numbers in Rust. Use stable idempotency/duplicate
mitigation for create operations where possible; ambiguous timeout returns an
unknown-result state with a search/recovery hint.

**Verify**: official API fixtures cover pagination, 401/403/404 privacy,
secondary rate limits, SSO/org approval, validation errors, and duplicate risk.

### A3: Add GitHub events

Start with `issues`, `issue_comment`, `pull_request`, and `pull_request_review`
events filtered by selected repository/action. Local mode may poll updated items
with checkpoints; production webhook mode depends on Plan 011 and a GitHub App.
Verify webhook signature and delivery ID at relay ingress and dedupe again at
desktop. Prevent workflows from recursively commenting on their own bot output.

Normalized data contains installation/repository IDs, event/action, issue/PR
number, actor, title/status, URL, and bounded preview only.

**Verify**: edited/redelivered/out-of-order events, branch/repo deletion,
installation suspend/uninstall, bot-loop, and cross-installation routing tests.

## Stage B: Linear connected app

### B1: Implement auth modes and catalog

Private/local MVP may accept a personal API key through a backend-owned secret
form. Public OAuth requires the relay because its client secret must not ship in
desktop. Store workspace/user IDs/scopes as metadata. Clearly label personal
token mode as advanced and one-workspace OAuth as the normal product path.

### B2: Register Linear actions

- `linear.create_issue`: team, title, description, priority, assignee, labels;
- `linear.comment_on_issue`;
- `linear.update_issue_status`;
- `linear.get_issue` for bounded context.

Cache team/project/workflow-state options briefly and invalidate on permission
errors. Validate IDs belong to the connected workspace. Keep GraphQL query
selection minimal and map `errors` even when HTTP status is 200.

### B3: Add Linear events

Use provider webhooks through Plan 011 for issue/comment/update events. Verify
the current signature algorithm and webhook timestamp/secret handling from
official docs. Filter by team/project/status and dedupe delivery/entity update
IDs. Add bot-loop/source-marker rules for Alfred-created comments.

**Verify Stage B**: personal token and OAuth staging flows, pagination/GraphQL
partial errors, 429/backoff, revoked token, webhook forgery/replay, workspace
isolation, and no description/comment bodies in logs.

## Stage C: Sentry incident connector

Begin only after GitHub/Linear event reliability and demand are measured.

Initial read/action/event surface:

- `sentry.get_issue`: issue metadata and bounded latest-event summary;
- `sentry.update_issue_status`: resolve/ignore with explicit reason;
- `sentry.issue_alert` event: project, issue/event IDs, title, level, culprit,
  first/last seen, count, URL—no full stack/local variables by default.

Use the narrowest organization/project scopes. Public integration OAuth and
webhook installation should use Plan 011; a local auth-token mode is advanced.
Treat stack traces, request data, breadcrumbs, and user context as sensitive;
fetch them only through an explicit action with strong limits and never persist
secrets scrubbed by Sentry as if they were safe by default.

**Verify Stage C**: project isolation, alert duplicate, issue merge, resolved/
regressed transitions, rate limits, PII fixture exclusion, and bot-loop tests.

## Test plan

- Connected Apps shows capability/scopes and repository/workspace/project
  boundaries before consent.
- App Action settings use searchable resource selectors and persist IDs plus
  display snapshots, never names as identity.
- Trigger settings show local-only vs relay delivery.
- Add contract-test suites per provider and a shared conformance suite for
  timeout, cancellation, 401 refresh, 403 missing scope, 429, 5xx, redaction,
  output limits, disconnect, and descriptor stability.

Run `bun test`, `bun run build:frontend`, and all Rust tests after every stage.

## Done criteria

- [x] Existing `gh` node/workflows remain compatible.
- [x] GitHub ships first with scoped repository access and tested actions/events.
- [x] Linear ships second with strict workspace isolation.
- [ ] Sentry starts only after a measured go/no-go gate.
- [x] No provider adds runner branches or provider-specific React node types.
- [x] Public OAuth/webhooks contain no confidential secrets in desktop.

## STOP conditions

- A provider requires organization-wide/admin scope for an MVP action.
- GitHub connected-app work would break or auto-migrate existing `gh` nodes.
- Linear/Sentry public OAuth is attempted without Plan 011 secret custody.
- Event payloads require full comments, stack variables, or repository content
  by default.
- The previous stage has unresolved duplicate execution or credential leakage.

## Maintenance notes

- Provider APIs and app-review rules change; pin API versions where supported.
- Track adoption/error rates without recording resource names or content.
- Split later provider growth into separate plans rather than expanding this
  rollout indefinitely.
