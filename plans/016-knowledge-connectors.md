# Plan 016: Add Notion, Google Drive, and SharePoint as bounded context sources

> **Executor instructions**: Treat this as a read-first staged rollout. Reuse
> Plans 008/009; do not create a second indexing system or sync database. Public
> confidential OAuth depends on Plan 011. Each provider stage needs an explicit
> scope and data-retention review.
>
> **Drift check (run first)**: inspect current context-node interpolation and
> runner output-size behavior after Plan 009. Verify the action framework can
> return bounded structured text without persisting credentials or arbitrary
> binary content.

## Status

- **Priority**: P1 Notion / P1 Google Drive / P2 SharePoint
- **Effort**: XL total
- **Risk**: HIGH
- **Depends on**: Plans 008, 009; Plan 010 for change events; Plan 011 for public OAuth/webhooks
- **Category**: integration roadmap
- **Planned at**: 2026-08-11
- **Implementation status**: IN PROGRESS — Stage A private Notion and a local,
  read-only Obsidian vault slice are implemented with bounded retrieval actions;
  public Notion OAuth remains behind Plan 011. Stage B is blocked on Plan 014's
  Google account/scope approval path. Stage C is deferred pending Plan 013,
  tenant policy, and demonstrated enterprise demand.

## Why these apps

Agents become much more useful when a workflow can retrieve product specs,
runbooks, and team decisions at execution time. The safe first product is
**on-demand retrieval from user-selected resources**, not silent organization-
wide indexing.

Priority:

1. Notion for startup/product docs.
2. Google Drive/Docs for general workspace documents.
3. SharePoint/OneDrive for Microsoft enterprise tenants, reusing Microsoft
   identity concepts without coupling to Outlook actions.
4. Obsidian for local-first Markdown knowledge without an OAuth or relay
   dependency (added during execution).

## Commands you will need

- `bun test`
- `bun run build:frontend`
- `cargo test --manifest-path src-tauri/Cargo.toml`
- `cargo check --manifest-path src-tauri/Cargo.toml`
- Run content-extraction, scope, provider contract, and prompt-injection fixtures.

## Scope

Register read actions through Plan 009, with an optional future “App Context”
presentation that is still persisted as generic `appAction`. Every fetch:

- identifies a specific page/file/site selected by the user;
- rechecks provider permission at run time;
- extracts plain text in Rust;
- preserves source ID/title/URL/updated timestamp;
- caps bytes/blocks/pages and marks truncation;
- treats document text as untrusted context, not instructions;
- does not cache full content beyond the workflow run by default.

Binary files, embedded media, comments, version history, hidden properties, and
organization-wide crawls are out of v1.

## Git workflow

Land one provider stage at a time, with extraction adapters isolated from the
framework. Do not commit/push without instruction. Preserve prerequisite and
unrelated changes; update the index only for the provider stages actually
completed and verified.

## Implementation steps

## Stage A: Notion

### A1: Connection modes and permissions

Private MVP may use an internal integration token entered through the secure
backend flow and requires users to share individual pages/databases with that
integration. Public OAuth uses the relay to protect the integration client
secret. Store workspace/bot owner metadata and scopes, not tokens.

### A2: Actions

- `notion.search_resources`: bounded search among resources shared with the
  integration;
- `notion.get_page`: retrieve title/properties allow-list and recursively
  paginate block children to a strict depth/byte limit;
- `notion.query_database`: selected database + descriptor-defined filters,
  bounded rows/properties.

Normalize blocks to plain text with explicit representations for unsupported
blocks. Do not follow arbitrary external embeds/URLs. Cache resource IDs/titles
briefly for selectors only.

### A3: Optional change events

After Plan 010/011, use current Notion webhooks for selected page/database
changes. Verify signatures/verification token, dedupe, and treat notifications
as hints to fetch the selected resource on desktop. Do not enqueue full page
content from the relay.

**Verify Stage A**: pagination, recursive depth, moved/deleted/unshared page,
database schema change, unsupported block, revoked token, webhook replay, and
source-permission tests.

## Stage B: Google Drive and Docs

### B1: Extend Google authorization incrementally

Reuse the Google connection concept from Plan 014 but request Drive scopes only
when Drive is enabled. Decide between `drive.file`-style user-selected access
and broader read-only access based on the actual resource picker/search UX;
document limitations and Google verification classification. Prefer narrow,
user-selected access even if it adds a selection step.

### B2: Actions

- `google_drive.list_selected_files` or a bounded selector/search permitted by
  the approved scope;
- `google_drive.get_document`: export Google Docs/Sheets/Slides to a safe text
  representation, or download supported text/PDF only if parsers and limits are
  reviewed;
- `google_drive.get_file_metadata`.

For v1 prioritize Google Docs plain text. Sheets need explicit row/column caps;
Slides need slide-order labels; PDFs should defer to a reviewed PDF extraction
path. Reject executables, archives, huge binaries, password-protected files, and
external URL fetches.

### B3: Optional changes

Use Drive change tokens/polling through Plan 010 while the app is open. Public
push channels require Plan 011 and provider lifecycle/renewal handling. Events
contain file ID/name/type/change kind/URL/timestamp only and prompt an explicit
fetch action for content.

**Verify Stage B**: scope upgrade/revoke, shared-drive boundaries, export
formats, large Docs/Sheets, deleted/unshared files, change-token reset, and
cross-account resource-ID tests.

## Stage C: SharePoint and OneDrive

Start only when Microsoft enterprise users need it. Add Graph delegated scopes
incrementally to the Plan 013 Microsoft connection, or create a separate
connection capability if tenant policy requires distinct consent. Select
specific sites/libraries/files; do not request tenant-wide application access.

Actions:

- `microsoft_drive.list_resources` within selected site/drive;
- `microsoft_drive.get_document` with bounded text extraction;
- `microsoft_drive.get_metadata`.

Use Graph delta only for selected scopes/resources. Respect tenant Conditional
Access, sensitivity labels, download restrictions, and item permissions. If
Microsoft Information Protection requires SDK/policy work, stop and split a
dedicated enterprise plan.

**Verify Stage C**: tenant/site isolation, shared link vs direct permission,
restricted download, sensitivity-label behavior, delta reset, and revoked user.

## Cross-provider UX and safety

- Connected Apps displays exactly which resources are shared/selected.
- Node settings persist immutable provider IDs plus a display snapshot.
- Retrieval outputs carry citations (title + provider URL) for downstream agent
  prompts and Activity details.
- Add prompt delimiters and an “external document—untrusted” label.
- No provider text enters diagnostics/analytics. Run-history retention follows
  existing local policy, and users can delete runs containing fetched context.
- Cache only selector metadata unless a separate encrypted content-cache plan is
  approved.

## Test plan

- Shared provider conformance suite plus provider fixture suites.
- `bun test`, frontend build, Rust tests/check after every stage.
- Prompt-injection fixtures embedded in documents remain delimited as data.
- Huge/deep/malformed content truncates safely without memory spikes.
- Search SQLite/logs for full source sentinel text outside intended local run
  payload; verify keychain tokens never appear.

## Done criteria

- [ ] Retrieval is user-selected, permission-checked, bounded, and cited.
- [ ] Notion and Drive ship before broad enterprise SharePoint work.
- [ ] No organization-wide indexing or hidden background cache exists.
- [ ] Change events carry metadata only and use explicit fetch for content.
- [ ] Scope upgrades are incremental and provider verification is complete.
- [ ] All provider/framework regression gates pass.

## Execution record — 2026-08-17

- [x] Drift check confirmed the generic action registry returns bounded
  structured output and credentials stay behind `TokenAccessCapability`.
- [x] Added a provider-neutral untrusted-output marker and runner prompt warning
  for external document results.
- [x] Added private Notion connection validation with bot/workspace identity,
  OS credential storage, redacted metadata, and reconnect-safe replacement.
- [x] Added `notion.search_resources`, `notion.get_page`, and
  `notion.query_database` using the generic `appAction` registry.
- [x] Pinned current `2026-03-11` Notion API semantics, including data sources.
- [x] Enforced page/property/result/depth/call/byte limits, citations, explicit
  truncation, unsupported-block markers, and no embed/media URL fetching.
- [x] Added prompt-injection, pagination, recursive depth, unshared/revoked,
  rate-limit, property allow-list, identity, redaction, and token-store tests.
- [x] Added a read-only local Obsidian vault connection plus bounded
  `obsidian.search_notes` and `obsidian.read_note` actions. Vault paths stay in
  the OS credential store; hidden folders, traversal, and symlinks are rejected.
- [x] `bun test`, `bun run build:frontend`, Rust tests, and Rust check pass for
  this slice (Rust network fixtures require localhost binding permission).
- [ ] Public Notion OAuth/webhooks: blocked by Plan 011's unapproved relay ADR.
- [ ] Google Drive: stopped before implementation because Plan 014 is TODO and
  no selected-file scope/verification decision exists.
- [ ] SharePoint/OneDrive: deferred by this plan until Microsoft account/tenant
  policy exists and enterprise demand justifies the sensitivity-policy review.

## STOP conditions

- Product requests full-workspace indexing/search without a separate storage,
  encryption, retention, and deletion design.
- A broad Drive/SharePoint scope is required without verification/admin review.
- Provider sensitivity/download policy cannot be enforced by the API.
- Extraction requires executing macros/scripts or fetching arbitrary embeds.

## Maintenance notes

- Provider document/block/file formats evolve; fixture test unsupported cases.
- Keep extraction adapters isolated so safer parsers can replace them.
- Re-evaluate local encrypted caching only after measuring latency and demand.
