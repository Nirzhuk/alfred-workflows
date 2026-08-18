# Knowledge connectors

Alfred retrieves knowledge on demand through the generic **App Action** node.
It does not index a workspace, run a background crawler, or maintain a second
sync database.

## Trust and retention boundary

- A workflow stores immutable provider resource IDs. The provider rechecks the
  connection and its current access on every run.
- Rust fetches and normalizes provider responses. Tokens never enter workflow
  JSON, action output, React connection state, diagnostics, or analytics.
- Every knowledge descriptor marks its output as untrusted external data. The
  runner places it under an explicit prompt warning before a downstream agent
  sees it. Document text is context, never instructions or authorization.
- Results contain a provider ID, resource ID, title, provider URL, update time
  when available, truncation state, and a citation artifact.
- Full content is not placed in selector caches. The existing action selector
  cache holds IDs and titles for 30 seconds only.
- Retrieved text can exist in local workflow run history because it is an
  intentional run result. Existing local run deletion removes that retained
  copy. No additional document cache exists.
- Embedded media, comments, version history, signed file URLs, arbitrary
  external embeds, hidden/unlisted properties, and organization-wide crawls
  are excluded.

## Obsidian: local vault

Status: implemented as a read-only local connector.

Open **Settings → Connected Apps → Obsidian**, choose a vault folder containing
`.obsidian`, and connect it. Alfred stores the canonical vault path in the
operating-system credential store. After the folder-picker attempt, the path is
cleared from React state; the backend never returns it, writes it to workflow
JSON, or includes it in action output. There is no Obsidian account, token,
network request, background watcher, or local content index.

Available actions:

- `obsidian.search_notes` searches Markdown note paths and up to 1 MiB of text
  per note, returning at most 10 or 25 matches with bounded snippets.
- `obsidian.read_note` reads one selected `.md` note and returns at most 48 KiB
  with an explicit truncation marker and an `obsidian://open` citation.

The connector ignores hidden directories (including `.obsidian`), non-Markdown
files, non-UTF-8 paths, and all symlinks. Note identifiers are vault-relative;
absolute paths and `..` traversal are rejected. A scan visits no more than
20,000 notes or 100,000 directory entries. Note contents use the same untrusted
external-document prompt boundary as cloud knowledge sources.

## Notion: private internal integration

Status: implemented for the local/private stage.

Create an internal integration in Notion, enable read-content capability, and
share each page or database explicitly through **Add connections**. Then open
**Settings → Connected Apps → Notion** and enter the internal integration
token. Alfred validates the bot and workspace identity before putting the token
in the operating-system credential store.

The local metadata record contains the workspace ID, bot ID, bot-owner type,
pinned Notion API version, declared read scopes, and `content_cache=disabled`.
It contains no token. Public Notion OAuth is intentionally unavailable until
the cloud-relay approval gate in Plan 011 is complete.

Available actions:

- `notion.search_resources` searches titles only among pages and data sources
  shared with the integration, with a maximum of 50 results.
- `notion.get_page` retrieves one selected page. It includes only explicitly
  named properties, recursively paginates block children, and caps output at
  24 KiB, 400 blocks, six nested levels, and 64 provider calls.
- `notion.query_database` queries one selected data source. It supports only
  the declared typed filters in the action descriptor, includes at most 12
  named properties per row, and returns no more than 25 rows or 48 KiB of
  structured row data.

The adapter pins `Notion-Version: 2026-03-11`. Unsupported blocks are represented
explicitly. Media and embed URLs are not followed or returned. A 404 is treated
as a moved, deleted, or unshared selection; 401, 403, and 429 responses map to
stable reconnect, scope, and rate-limit errors without preserving provider
response bodies.

## Google Drive and Docs

Status: deferred at the authorization gate.

Plan 014 has not yet supplied a verified Google account connection or an
approved Drive scope-upgrade path. Alfred must not substitute a pasted access
token or silently request broad `drive.readonly` access. Implement Drive only
after product/security records the selected-file UX, exact scope, Google
verification classification, cross-account resource behavior, and retention
review. Google Docs plain text comes first; Sheets, Slides, PDFs, binaries, and
change delivery remain separate reviewed slices.

## SharePoint and OneDrive

Status: deferred pending Microsoft demand and account authorization.

Plan 013 has not yet supplied the delegated Microsoft account connection, and
tenant policy remains undecided. Do not request tenant-wide application access.
A future slice must select specific sites, drives, libraries, and files; review
Conditional Access, sensitivity labels, and download restrictions; and stop if
the API cannot enforce those policies. Graph delta and webhooks are not part of
the local Notion stage.

## Verification

Automated tests cover descriptor secrecy, prompt-injection labeling, UTF-8-safe
byte truncation, property allow-lists, unsupported block representations,
external URL omission, pagination, recursive depth, revoked/unshared/rate-limit
mapping, validated connection identity, token/metadata separation, Obsidian
path traversal, symlink exclusion, hidden-folder exclusion, and absolute-path
redaction.
