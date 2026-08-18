# Install Alfred

Alfred is a **local desktop app**. It does not include AI models or agent
subscriptions. It runs workflows by calling CLIs you already use on this
machine.

There are two supported ways to get it:

| Option | Cost | Intended for |
| --- | --- | --- |
| Desktop License | Paid per named user | People who want the maintainer-built/tested download and official update channel; annual/lifetime options are planned |
| Company member seat | Paid per active member | Includes the Desktop entitlement plus organization/hosted features; current Windows builds are unsigned beta artifacts |
| [Build from source](building-from-source.md) | Free | People comfortable installing Bun, Rust, and Tauri's platform prerequisites |

No Alfred purchase is needed to compile or run your own source build. Agent
provider subscriptions and usage charges remain separate. See the
[open-source and distribution policy](open-source.md).

## Requirements

| | |
| --- | --- |
| **OS** | macOS 11+, Windows 10/11 (64-bit), or a modern Linux desktop (x64) |
| **Agents** | At least one supported CLI installed and signed in (see below) |
| **Optional** | `git` — used when a run reports working-tree file changes |

You only need the CLIs for the agents you put on a workflow. Unused providers
can stay uninstalled.

### Supported agent CLIs

Install and authenticate each tool with its own installer / login flow. Alfred
looks for these binaries on your `PATH` (and common install locations):

| Provider | Binary | Check |
| --- | --- | --- |
| Claude Code | `claude` | `claude --version` |
| Cursor Agent | `cursor-agent` (or `agent`) | `cursor-agent --version` |
| Codex | `codex` | `codex --version` |
| OpenCode | `opencode` | `opencode --version` |

Each CLI must already be logged in the same way you use it in a terminal.
Alfred does not collect provider passwords or API keys; it reuses the
credentials those CLIs already store on your machine.

If a run fails with “CLI not found”, open a terminal, confirm the command
above works, then fully quit and reopen Alfred so it picks up an updated
`PATH`.

## Install an official build

Official installers will be delivered to either a standalone Desktop License
holder or an active Company/Enterprise member seat. Every paid member seat
includes Desktop. This public source repository does not publish the
maintainer-signed installers as public GitHub Release assets.

Only treat a build as official when it comes from the purchase/download channel
linked by this repository. Verify the published checksum when one is provided.

### macOS

1. Download the `.dmg` for your chip (Apple Silicon or Intel).
2. Open it and drag **Alfred** into Applications.
3. Launch from Applications (or Spotlight).

Official releases should be signed and notarized. An unsigned self-build may
need separate approval under System Settings → Privacy & Security.

### Windows

1. Download the **NSIS** `.exe` installer from the official download channel
   (or the `.msi`, when offered).
2. Run the installer and follow the prompts.
3. Launch **Alfred** from the Start menu.

The current Windows installer is an **unsigned beta**. Windows will report an
unknown publisher and may show a SmartScreen warning. Continue only if the file
came from Alfred's official download channel and its published checksum matches.

### Linux

From the official download channel, pick one:

- **AppImage** — mark executable (`chmod +x …AppImage`) and run
- **`.deb`** — `sudo dpkg -i …deb` (Debian/Ubuntu)
- **`.rpm`** — install with your distro’s package tool

WebKitGTK is required for the UI (same family of libs other Tauri apps use).

## Build it yourself for free

Follow [Build Alfred from source](building-from-source.md). The short form,
after installing the prerequisites, is:

```bash
bun install --frozen-lockfile
bun run check
bun run build
```

Self-built artifacts appear under `src-tauri/target/release/bundle/`. They do
not use the maintainers' signing identities and are not covered by official
binary support.

## First launch checklist

1. Install Alfred for your OS (above).
2. Confirm at least one agent CLI works in a normal terminal.
3. Open Alfred and create a workflow with that agent.
4. Run once manually before enabling schedules or file/webhook triggers.

## How Alfred behaves on your machine

- **Local data** — workflows, runs, schedules, and memories live in an on-disk
  SQLite database under the app’s application-support directory. Nothing is
  uploaded to an Alfred cloud.
- **Local history search** — searchable run history covers persisted run steps
  and saved memories using SQLite FTS5. It uses no embeddings or remote search
  service. The search tables are a derived index that Alfred can rebuild from
  canonical local rows; deleting canonical runs or memories removes them from
  search. History may include prompts and agent/tool results, so treat it as
  private local data.
- **Schedules & triggers** — cron, file watchers, and local webhooks only fire
  while Alfred is running (including when the window is closed but the app
  stays in the menu bar / tray). Fully quitting the app pauses automations.
- **Webhooks** — listen on loopback only (default port `8787`). Override with
  the `ALFRED_HTTP_PORT` environment variable and restart the app.
- **Permissions** — agent steps inherit whatever tools and file access those
  CLIs already have. Treat workflow prompts like instructions you would type
  into the agent yourself.

## Troubleshooting

| Symptom | What to try |
| --- | --- |
| “`claude` / `cursor-agent` / `codex` / `opencode` CLI not found” | Install the CLI, verify it in Terminal/PowerShell, restart Alfred |
| Agent runs but returns auth errors | Log in with that CLI’s own login command; Alfred does not re-auth for you |
| Schedule never fires | Keep Alfred running (tray is enough); confirm the schedule is enabled |
| Webhook not reachable from another device | By design — binds to `127.0.0.1` only |
| macOS won’t open the app | Prefer a notarized build, or allow it under Privacy & Security |

## Related docs

- [Building from source](building-from-source.md) — complete platform setup
- [Open-source policy](open-source.md) — what is free and what the paid build funds
- [Releasing](releasing.md) — maintainer-only signing and paid distribution
- [README](../README.md) — product overview and developer setup
