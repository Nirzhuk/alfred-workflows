# Alfred

**Alfred** is a local desktop app for building automations across your AI coding agents.

Compose workflows as visual graphs: write prompts, route them through agents you already use on your machine, optionally pin a **skill** and a **model**, then run the sequence and choose which outputs to keep.

> **Open source, paid official builds.** The complete source is available under
> [GPL-3.0-or-later](LICENSE). You can inspect, modify, and compile it without
> buying Alfred, and paying never disables local workflows or your data.
> [Polar](https://polar.sh) is the merchant of record for the official builds:
> it hosts checkout, the customer portal, license keys, seats, and the installer
> downloads. Alfred runs no billing, account, license, webhook, email, or
> download backend of its own. Purchases fund testing, signing, support, and
> release engineering.

| Get Alfred | Alfred fee | What you receive |
| --- | --- | --- |
| **Alfred License** | **One-time**, one named user | Maintainer-built/tested installers downloaded from Polar, macOS code signing and notarization, official release support, and **one year of updates** |
| **Alfred Teams** | **One-time, per claimed seat** | The same, for every claimed seat — each gets its own license key, its own Polar downloads, and its own update year. Purchased on the Alfred website |
| [Build from source](docs/building-from-source.md) | **Free, forever, fully featured** | A local build you compile yourself, with every feature. It is unsigned and has no official binary support |

Both products are **one-time purchases**. There is no subscription, no annual
renewal, and no recurring charge.

### What "one year of updates" means

- **Paying once unlocks every feature permanently.** Nothing you paid for is
  ever taken away.
- Your purchase includes every Alfred build released within **one year** of
  buying it.
- After that year, **the version you have keeps working exactly as it does
  today, forever** — same features, same workflows, same data.
- Newer builds released after your year still install and run, and every
  workflow, memory, schedule, trigger, and file on your machine stays intact
  and usable. Only their paid features stay locked until you buy another year.
- Lapsing never disables the build you own, never removes a feature you paid
  for, and never touches your data.
- A refund or a revoked license is different: that does end access.
- <!-- TODO(owner-approval): the exact in-app wording shown when an update year
     has lapsed is drafted in plans/release-money/004 Step 2 and is marked
     "DRAFT — needs owner approval". Do not publish it here until approved. -->

Official builds for v0.5.0 update **manually** through Polar. Alfred has no
automatic updater: **Help → Download Latest Version…** opens Polar's customer
portal in your browser, and you sign in with your purchase email to reach your
personal downloads. Alfred never fetches an installer for you.

A license activates on at most **three devices**. It refreshes after 7 days when
the network allows and keeps working for at most **30 days offline**; a
confirmed revocation takes effect immediately.

Fees charged by Claude, Cursor, OpenAI, or other agent providers for their own
services are separate from Alfred in every case. See
[Open-source and distribution policy](docs/open-source.md) for details.

## Install

**User guide:** [docs/install.md](docs/install.md) — official installers,
source builds, OS requirements, and agent CLI setup.

Short version:

1. Buy an Alfred License through Polar, claim an Alfred Teams seat, or
   [compile Alfred from source](docs/building-from-source.md) for free —
   a source build has every feature, forever.
   <!-- TODO(polar-url): publish the approved Polar checkout link here. -->
2. Install and sign in to at least one agent CLI you plan to use:
   - Claude Code → `claude`
   - Cursor Agent → `cursor-agent` (or `agent`)
   - Codex → `codex`
   - OpenCode → `opencode`
   - GitHub Copilot → `copilot`
   - Gemini CLI → `gemini`
   - Grok Build → `grok`
3. Confirm the CLI works in a normal terminal, then open Alfred and run a workflow.

Alfred does **not** replace those subscriptions or store their API keys. It shells out to CLIs that are already authenticated on your machine.

Schedules, file triggers, and webhooks only run while the app is open (including tray / menu bar).

## What it does

- **Workflows as automations** — sequences of inputs → agent steps → outputs. Every workflow can be run manually; scheduling is an optional later trigger for the same automation.
- **Multi-agent** — Claude Code, Cursor, Codex, OpenCode, GitHub Copilot, Gemini, and Grok via local CLIs.
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

Official binaries are distributed through Polar's File Downloads benefit, not
public GitHub Releases. The Windows installer is an **unsigned beta**: Windows
reports an unknown publisher and may show a SmartScreen warning. Linux packages
are best-effort source-adjacent builds, not a supported paid download. The
public repository remains fully buildable. See
[installing](docs/install.md), [building from source](docs/building-from-source.md),
and the [maintainer release runbook](docs/releasing.md).

## License

Alfred is free software licensed under
[GPL-3.0-or-later](LICENSE), Copyright © 2026 nirzhuk. You may use, modify,
compile, and redistribute it, including commercially, provided you follow the
GPL. Distributed binaries must be accompanied by access to their corresponding
source under the same license.

The GPL does not grant trademark rights or permission to imply that an
unofficial build is an official Alfred release. See
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
then upload the accepted artifacts to Polar's File Downloads benefit. The draft
must never be published as a public GitHub Release. See
[docs/releasing.md](docs/releasing.md).
