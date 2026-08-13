# Agentflow

**Agentflow** is a local desktop app for building automations across your AI coding agents.

Compose workflows as visual graphs: write prompts, route them through agents you already use on your machine, optionally pin a **skill** and a **model**, then run the sequence and choose which outputs to keep.

> **Open source, paid official builds.** The complete source is available under
> [GPL-3.0-or-later](LICENSE). You can inspect, modify, and compile it without
> buying Agentflow. Official maintainer-built downloads will be paid when the
> sales channel launches; purchases will fund testing, signing, support, and
> release engineering.

| Get Agentflow | Agentflow fee | What you receive |
| --- | --- | --- |
| Official download | Paid | Maintainer-built and tested installer, code signing/notarization where available, and official release support |
| [Build from source](docs/building-from-source.md) | Free | A local build you compile yourself; it may be unsigned and has no official binary support |

Subscriptions or usage fees charged by Claude, Cursor, OpenAI, or other agent
providers are separate from Agentflow in both cases. See
[Open-source and distribution policy](docs/open-source.md) for details.

## Install

**User guide:** [docs/install.md](docs/install.md) — official installers,
source builds, OS requirements, and agent CLI setup.

Short version:

1. Buy the official installer when the download channel launches, or
   [compile Agentflow from source](docs/building-from-source.md) for free.
2. Install and sign in to at least one agent CLI you plan to use:
   - Claude Code → `claude`
   - Cursor Agent → `cursor-agent` (or `agent`)
   - Codex → `codex`
   - OpenCode → `opencode`
3. Confirm the CLI works in a normal terminal, then open Agentflow and run a workflow.

Agentflow does **not** replace those subscriptions or store their API keys. It shells out to CLIs that are already authenticated on your machine.

Schedules, file triggers, and webhooks only run while the app is open (including tray / menu bar).

## What it does

- **Workflows as automations** — sequences of inputs → agent steps → outputs. Every workflow can be run manually; scheduling is an optional later trigger for the same automation.
- **Multi-agent** — Claude Code, Cursor, Codex, and OpenCode via local CLIs.
- **Skills** — pin a `SKILL.md` skill on an agent step (invoked as `/skill-name …`).
- **Models** — pick the model/alias each agent should use (`sonnet`, `gpt-5`, `provider/model`, etc.).
- **Triggers** — optional cron schedules, file watchers, and loopback webhooks.
- **Local persistence** — workflows, runs, memories, and schedules in SQLite on disk.

## Core ideas

| Concept | Meaning |
| --- | --- |
| Prompt node | The instruction text fed into the next step |
| Agent node | Which CLI runs, with optional model + skill |
| Choose output | Pick which result continues downstream |
| Run | Manually enqueue/execute the automation |
| Schedule | Optional cron that fires the same runner while the app is open |

## Platforms

Desktop only: **macOS 11+**, **Windows 10/11**, and **Linux**. Android, iOS, and standalone website builds are not supported.

Official binaries are distributed through Agentflow's paid download channel,
not public GitHub Releases. The public repository remains fully buildable. See
[installing](docs/install.md), [building from source](docs/building-from-source.md),
and the [maintainer release runbook](docs/releasing.md).

## License

Agentflow is free software licensed under
[GPL-3.0-or-later](LICENSE), Copyright © 2026 nirzhuk. You may use, modify,
compile, and redistribute it, including commercially, provided you follow the
GPL. Distributed binaries must be accompanied by access to their corresponding
source under the same license.

The GPL does not grant trademark rights or permission to imply that an
unofficial build is an official Agentflow release. See
[BRANDING.md](BRANDING.md) and the
[open-source and distribution policy](docs/open-source.md).

## Develop

```bash
bun install
bun run dev      # desktop app (Tauri)
bun run build    # desktop installers for the current OS
bun run check    # frontend tests/build + Rust tests
```

Frontend Vite is started automatically by Tauri (`dev:frontend` / `build:frontend`). Do not use Vite alone as a website.

The complete platform prerequisites and expected artifact paths are in
[docs/building-from-source.md](docs/building-from-source.md).

### Stack

- **UI** — React + React Flow (`@xyflow/react`)
- **Shell** — Tauri 2
- **Data** — SQLite (`rusqlite`) in the Rust backend
- **Execution** — Rust agent adapters that spawn local CLIs

## Contributing

Issues and pull requests are welcome. Start with [CONTRIBUTING.md](CONTRIBUTING.md),
follow the [Code of Conduct](CODE_OF_CONDUCT.md), and report vulnerabilities as
described in [SECURITY.md](SECURITY.md).

Maintainers stage official installers in a private GitHub Release draft and
then deliver tested artifacts through the paid download channel. The draft
must never be published as a public GitHub Release. See
[docs/releasing.md](docs/releasing.md).
