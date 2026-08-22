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

---

## 5. Main surfaces (UI)

- **Sidebar** — Schedules / Settings nav; workflow list (+ create, reorder, context menu)
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
