# Alfred — product & engineering specs

Living doc for what Alfred is, how it works, and how we build it. Prefer updating this when product decisions change.

Related docs: [README.md](README.md), [docs/install.md](docs/install.md),
[docs/releasing.md](docs/releasing.md),
[docs/connected-apps.md](docs/connected-apps.md),
[docs/app-actions.md](docs/app-actions.md),
[docs/design-system.md](docs/design-system.md), [plans/](plans/).

---

## 1. Product

| | |
| --- | --- |
| **Name** | Alfred |
| **Repo / package** | `alfred` |
| **Bundle id** | `com.nirzhuk.alfred` |
| **Version** | `0.5.0` (see `package.json` / `tauri.conf.json`) |
| **License** | GPL-3.0-or-later |
| **Author** | nirzhuk |

**One-liner:** Local desktop app for composing multi-agent workflow automations that shell out to AI coding CLIs already installed and signed in on the machine.

**Not in scope (current):**

- Mobile (Android / iOS) or a deployable website / PWA
- Storing or replacing agent API keys / subscriptions
- Running schedules / file triggers / webhooks when the app is fully quit (they run while the app is open, including tray / menu bar)

---

## 2. Platforms & distribution

- **Desktop only:** macOS 11+, Windows 10/11, Linux
- **Dev:** `bun install` → `bun run dev` (Tauri desktop)
- **Build:** `bun run build` → OS installers (`app`/`dmg`, `deb`/`rpm`/`appimage`, `nsis`/`msi`)
- **Vite** (`dev:frontend` / `build:frontend`) exists only as Tauri before-commands — not a standalone site
- **Ship:** paid official installers staged as private GitHub Release drafts;
  public GPL source remains buildable (see install/releasing docs)

---

## 3. Tech stack

| Layer | Choice |
| --- | --- |
| UI | React 19, TypeScript, Vite 7 |
| Graph editor | `@xyflow/react` (React Flow) |
| Client state | Zustand |
| Desktop shell | Tauri 2 (Rust) |
| Persistence | SQLite via `rusqlite` (bundled) |
| Connected-app credentials | Native OS credential store via `keyring` |
| Package manager | Bun |
| Plugins | dialog, notification, opener, window-state, single-instance |

---

## 4. Core concepts

| Concept | Meaning |
| --- | --- |
| **Workflow** | Named automation: graph (nodes + edges), optional working directory, schedule, triggers |
| **Input** | Instruction / context for downstream steps (legacy graph type: `prompt`) |
| **Agent** | Local CLI step: provider + optional model + optional skill |
| **Memories** | Inject selected / pinned library items into the run context |
| **Output** | Choose what to keep / pin / set as final result (legacy: `chooseOutput`) |
| **Run** | One execution (manual, schedule, or event); statuses: pending → running → completed \| failed \| cancelled |
| **Schedule** | Optional cron (one per workflow); ticker while app is open |
| **Trigger** | File watcher or loopback webhook; fires the same runner with a payload |
| **Library** | Durable notes / artifacts / agent outputs; star to inject on next run |
| **Skill** | `SKILL.md` package pinned on an agent step (`/skill-name …`) |
| **Connected App** | Provider-neutral local connection metadata in SQLite; OAuth credentials remain in the OS credential store |
| **App Action** | One descriptor-driven workflow node whose validation, credentials, and execution stay in Rust |

Inputs can be blocked to protect their label, prompt, attachments, size, and
canvas position from accidental edits. The blocked state is stored with the
workflow graph and can be reversed from the node, its context menu, or settings.

### Agent providers

| Id | CLI |
| --- | --- |
| `claude_code` | `claude` |
| `cursor` | `cursor-agent` / `agent` |
| `codex` | `codex` |
| `opencode` | `opencode` |
| `github_copilot` | `copilot` |
| `gemini` | `gemini` |
| `grok` | `grok` |

---

## 5. Main surfaces (UI)

- **Sidebar** — History / Schedules / Settings nav; workflow list (+ create, reorder, context menu)
- **Canvas** — React Flow editor, add-step panel, toolbar (cwd, run/save, etc.)
- **Activity panel** — This run / Result / Library / Live log
- **Schedules page** — All cron schedules across workflows
- **Settings** — Theme, OS notifications, and Connected Apps status/disconnect
- **Modals** — rename, schedule, triggers, output, memories inspector, confirm delete
- **Tray / menu bar** — presence while running / scheduled work matters

---

## 6. Runtime (high level)

1. Start via UI `run_workflow`, schedule tick (~20s), file/webhook trigger, or tray
2. Insert SQLite `runs` row (`trigger_kind` + optional `payload_json`); spawn worker
3. Load graph → topological order → prelude (pinned memories + trigger payload)
4. Walk nodes: Input → Memories → Agent (CLI adapter) → Output (persist / final)
5. Persist `run_steps`, emit `run://event` for the activity UI; finish status; refresh tray

Schedules and event triggers only fire while Alfred is running (including tray).

---

## 7. Data (SQLite)

Primary entities (see `src-tauri/src/db/schema.sql`):

- `workflows` — name, description, working directory, `graph_json`, sort order
- `agents` — provider registry / metadata as used by the app
- `runs` / `run_steps` — execution history
- `schedules` — one cron row per workflow
- `triggers` — file / webhook configs
- `memories` / `memory_links` — library + cross-workflow links
- `app_connections` — non-secret provider/account/scopes/health metadata; credentials are never stored in SQLite

### Local history search

Alfred provides searchable run history across persisted run steps and saved
memories. Search stays on the machine and uses SQLite FTS5 only—there is no
embedding model, remote search service, or network request. The FTS tables are a
derived index that can be rebuilt from canonical `runs`, `run_steps`, and
`memories` rows. Creating, updating, or deleting canonical data updates or
removes the corresponding search documents transactionally.

Run history can contain prompts, agent/tool results, errors, and saved memory
text. Treat it as private local data with the same sensitivity as the workflows
that produced it.

### Scoped atomic memory

Each memory is one compact claim or note. Its semantic type—`preference`,
`fact`, `decision`, `constraint`, `lesson`, `episode`, `checkpoint`, `note`,
`output`, or `artifact`—is separate from its rendering/content kind (`text`,
`note`, or `artifact`). Memory has three visibility scopes:

- **User memory** uses the fixed local identity `local-user` and is visible to
  every workflow on this installation.
- **Workspace memory** is visible to workflows whose configured absolute
  working-directory path has the same purely lexical normalization. Alfred
  removes `.` and collapses safe `..` components; it does not query the
  filesystem, resolve symlinks, probe mounts, or contact network paths.
- **Workflow memory** is visible to its owning workflow and to workflows with
  an explicit legacy memory link.

Lifecycle is explicit: `active`, `superseded`, or `retracted`. Inactive or
expired records remain inspectable for correction history but never enter a
prompt. Explicit deletion physically removes the canonical local row; deleting
a workflow removes its workflow memory while preserving user/workspace memory
that originated there.

Pinned active memory forms bounded core context. The complete block is at most
6,000 UTF-8 bytes: 1,500 for user, 2,000 for workspace, and 2,500 for
workflow/linked memory, with unused capacity flowing forward and no item above
1,500 bytes. Overflow remains in the library and the prompt reports only an
omitted count. Durable memory is reference data, not authorization: it cannot
override the current request, workflow instructions, permissions, or safety
boundaries, and instructions embedded in memory text are ignored.

### Automatic recall

New workflows default **Automatic recall** on; existing workflows migrated
from earlier Alfred versions remain off until the user opts in from the
Memories inspector. The same switch disables recall again without changing the
workflow graph or deleting memory.

Immediately before every Agent or Custom agent step, Alfred searches against
that step's current accumulated prompt. Candidate visibility reuses the scoped,
active, unexpired memory rules above. Ranking is deterministic: local exact
FTS5 result position, scope, salience, confidence, and last-confirmed recency;
when exact search has no matches, recent in-scope memory is the fallback.
Pinned core memory and memories already loaded by a Memory node are excluded
from recall so prompt text is not duplicated. Utility nodes never receive
automatic memory.

Recalled context is capped at 8 items and 6,000 UTF-8 bytes, with at most 1,200
bytes per item and 8,000 newest bytes of the current prompt used for its query.
Every included id, reason, rank, score, and rendered byte count is recorded in
`run_memory_uses` and shown in History; the audit row never copies a memory body
or search query. A retrieval or FTS5 failure is non-fatal and the agent proceeds
without recalled context while preserving pinned core behavior.

V1 has no embeddings, remote retrieval API, network call, or model download.
Retrieved text is explicitly untrusted reference data: it cannot override
current instructions, authorize actions, expand connected-app scope, or grant
tool access.

### Reviewable post-run memory suggestions

Memory review is a **candidate-only** pipeline, off by default globally and per
workflow. Enabling it requires choosing one supported agent provider in the
Memory review settings and explicitly acknowledging the cost: after each
eligible completed run (never failed or cancelled runs), Alfred may make at
most **one additional model invocation** with that CLI. The reviewer receives a
bounded digest of persisted run text — at most 32 KiB, built from canonical run
steps with control characters stripped — plus up to 12 relevant existing
memories for context. The digest stays inside the same local CLI boundary the
user already chose; no other data leaves the machine.

The reviewer returns strict JSON proposing `create`, `supersede`, or `retract`
candidates (at most five). Every candidate is deterministically validated for
size, scope, target visibility, duplicate hashes, secret-like material,
invisible characters, and instruction-like language before storage; malformed
responses are rejected whole without a "repair" call. Candidates never change
canonical memory by themselves: approval is transactional and revalidated, and
a stale candidate becomes `blocked` instead of being adapted. Users can edit
title/body/scope/type while pending, approve, reject, or retry a failed review
once manually. Reviews never modify skills and never write directly to memory;
failures persist only stable codes (`auth_required`, `provider_unavailable`,
`timeout`, `invalid_response`, `internal`) and never affect run status or
output. Review can be disabled globally or per workflow at any time, and
decided suggestion history can be physically deleted through Data settings.


---

## 8. Repo layout

```
src/                          # React app
  components/                 # shared UI (kebab folders)
  features/workflow/          # canvas, store, api, nodes, panels
  features/settings/          # settings, theme, notifications
  features/integrations/      # Connected Apps and generic App Action UI/state
src-tauri/src/
  agents/                     # CLI adapters
  runner/                     # execute graphs
  db/                         # SQLite + schema
  scheduler/                  # cron ticker
  triggers/                   # file + webhook
  skills/                     # SKILL.md discovery
  commands/                   # Tauri invoke handlers
  integrations/               # provider catalog, OAuth, keychain, refresh, action registry
docs/                         # install, releasing
plans/                        # product / licensing / release handoffs
scripts/guard-desktop-tauri.mjs
```

---

## 9. Engineering conventions (current)

### Desktop-only

- Never add Android/iOS targets or treat Vite as a deployable website
- Always use `bun run dev` / `bun run build` for the desktop shell

### Components

- Shared UI → `src/components/<kebab-name>/`
- Feature UI → `src/features/<feature>/components/<kebab-name>/`
- Folder + files: kebab-case; export: PascalCase
- Colocate `*.tsx`, `index.ts`, optional `*.test.tsx` in the same folder
- Import via folder path, not a lone PascalCase file at a components root

### Style / UX (working defaults)

- `docs/design-system.md` is the visual source of truth; shared decisions use
  semantic CSS custom properties in `App.css`, not feature-local literals
- Alfred prefers Infer for interface text, uses bundled Geist as its local
  fallback, and uses Geist Mono for utility accents; the desktop UI makes no
  runtime font request
- Application-owned UI uses identical tokens on macOS, Windows, and Linux;
  platform branches are limited to OS chrome, shortcuts, permissions, and
  native system surfaces
- The root `data-platform` attribute is the CSS platform boundary. Components
  must not implement their own user-agent checks
- The type scale is 11/12/14/16/20/24px and weights are limited to
  400/500/600/700; fractional font weights are not allowed
- Shared spacing follows the documented four-pixel grid; controls, cards,
  dialogs, icons, motion, elevation, and layering use their semantic token scales
- Product dialogs use the shared top-anchored precision-sheet shell with a
  blurred scrim, mono context rail, trapped keyboard focus, and opener focus return
- Sidebar item text is always 14px/400, icons are 18px, and section labels are
  16px/600 across workflow and Settings navigation
- Navigation is flat by default: selection uses a subtle surface background,
  never a weight shift, border, or shadow
- Interactive components preserve dimensions across default, hover, pressed,
  selected, focus-visible, disabled, error, and loading states
- Keyboard focus, accessible names, AA contrast, and reduced-motion behavior
  are required in light and dark themes
- Title-bar content reserves macOS traffic-light space but uses the standard
  content inset on Windows, Linux, unknown platforms, and in fullscreen
- Keep activity panels compact (e.g. Library list max-height) when lists can grow large
- Menus: shared `src/components/menu/` (dropdowns portaled when needed)

### Open / planned (see `plans/`)

- Offline licensing / freemium entitlements (Polar) — **not implemented yet**
- Release signing secrets, Homebrew cask, in-app updater — tracked in `plans/004`–`006`

---

## 10. Open questions (fill in)

These need product / team answers. Edit this section as decisions land.

### Product

- [ ] Target user (solo local-first? teams later?)
- [ ] What “done” looks like for v1 vs freemium paid tier
- [ ] Offline / tray expectations: any “run when quit” story, or stay “app must be open”?
- [ ] Default privacy story: data never leaves machine except agent CLIs?

### Code style

- [ ] Formatter / linter of record (Prettier? Biome? rustfmt only? ESLint?)
- [ ] Prefer CSS modules / Tailwind later, or stay global `App.css`?
- [ ] Test policy: unit required for new components? Rust tests for runner?
- [ ] Commit message style (conventional commits?)
- [ ] Absolute import alias (`@/…`) required vs relative?

### Specs process

- [ ] Is `specs.md` the source of truth, or should product live under `docs/`?
- [ ] Who updates this on merge — author of the PR?
- [ ] How do `plans/` handoffs relate (link only vs promote into specs when shipped)?
