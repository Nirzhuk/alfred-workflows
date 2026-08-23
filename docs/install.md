# Install Alfred

Alfred is a **local desktop app**. It does not include AI models or agent
subscriptions. It runs workflows by calling CLIs you already use on this
machine.

There are three supported ways to get it:

| Option | Cost | Intended for |
| --- | --- | --- |
| **Alfred License** | **One-time**, one named user | People who want the maintainer-built/tested installers hosted by Polar |
| **Alfred Teams seat** | **One-time, per claimed seat** | Teams; every claimed seat gets its own license key and its own Polar downloads. Bought on the Alfred website |
| [Build from source](building-from-source.md) | **Free, forever, fully featured** | People comfortable installing Bun, Rust, and Tauri's platform prerequisites |

Both paid options are **one-time purchases** — no subscription, no annual
renewal, no recurring charge. Each includes **one year of updates**: paying once
unlocks every paid feature permanently, and the build you own keeps working
exactly as it does today, forever. After the year, newer builds still install
and run with all your data intact; only their paid features stay locked until
you buy another year. A refunded or revoked license is different and does end
access.

[Polar](https://polar.sh) is the merchant of record for the paid options. Polar
hosts checkout, the customer portal, license keys, seat management, and the
installer downloads. Alfred runs no billing, account, license, webhook, email,
or download backend of its own, and it never sends your workflows or workflow
data anywhere.

No Alfred purchase is needed to compile or run your own source build. A source
build is **free, fully featured, and stays that way forever** — it is not a
trial or a reduced edition. Paying never disables local workflows or your data. Agent provider
subscriptions and usage charges remain separate. See the
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
| GitHub Copilot | `copilot` | `copilot --version` |
| Gemini CLI | `gemini` | `gemini --version` |
| Grok Build | `grok` | `grok --version` |

Each CLI must already be logged in the same way you use it in a terminal.
Alfred does not collect provider passwords or API keys; it reuses the
credentials those CLIs already store on your machine.

If a run fails with “CLI not found”, open a terminal, confirm the command
above works, then fully quit and reopen Alfred so it picks up an updated
`PATH`.

## Install an official build

Official installers are delivered through Polar's File Downloads benefit to an
Alfred License holder or to a claimed Alfred Teams seat. This public source
repository does not publish the maintainer-signed installers as public GitHub
Release assets.

1. Open Polar's customer portal.
   <!-- TODO(polar-url): publish the approved Polar customer-portal link here. -->
2. Sign in with the email address you purchased with. Polar authenticates you by
   email; there is no Alfred account or password.
3. Download the file for your platform, then compare its SHA-256 checksum with
   the checksum manifest published beside the downloads.

Only treat a build as official when it came from Polar and its checksum matches
the published manifest.

### macOS

1. Download the `.dmg` for your chip (Apple Silicon or Intel).
2. Open it and drag **Alfred** into Applications.
3. Launch from Applications (or Spotlight).

Both official macOS disk images are Developer ID signed, notarized, and
stapled. An unsigned self-build may need separate approval under System
Settings → Privacy & Security.

### Windows — unsigned beta

1. Download the **NSIS** `.exe` installer from Polar.
2. Run the installer and follow the prompts.
3. Launch **Alfred** from the Start menu.

The Windows installer is an **unsigned beta**. It is not Authenticode signed.
Windows will report an unknown publisher and SmartScreen will very likely warn
you before it runs. That warning is expected — this is not a warning-free
Windows release. Continue only if the file came from Polar and its published
SHA-256 checksum matches.

### Linux — best effort

Linux packages are **best effort**, not a supported paid download. Build from
source if you need a supported path on Linux. When a package is offered, pick
one:

- **AppImage** — mark executable (`chmod +x …AppImage`) and run
- **`.deb`** — `sudo dpkg -i …deb` (Debian/Ubuntu)
- **`.rpm`** — install with your distro’s package tool

WebKitGTK is required for the UI (same family of libs other Tauri apps use).

### Updating

**v0.5.0 official builds update manually.** Alfred has no automatic updater and
does not download installers for you. In the app, **Help → Download Latest
Version…** (also in the tray/menu-bar menu) opens Polar's customer portal in
your browser; sign in by email and download the newer file, then install it over
your existing copy. Your workflows, runs, schedules, and memories are local and
survive an upgrade install.

A license activates on at most **three devices**. It refreshes after 7 days when
the network allows and keeps working for at most **30 days offline**; a
confirmed revocation takes effect immediately. Use Polar's customer portal to
free a device slot, change billing, or manage Teams seats.

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
| “agent CLI not found” | Install the selected CLI, verify it in Terminal/PowerShell, restart Alfred |
| Agent runs but returns auth errors | Log in with that CLI’s own login command; Alfred does not re-auth for you |
| Schedule never fires | Keep Alfred running (tray is enough); confirm the schedule is enabled |
| Webhook not reachable from another device | By design — binds to `127.0.0.1` only |
| macOS won’t open the app | Prefer a notarized build, or allow it under Privacy & Security |

## Related docs

- [Building from source](building-from-source.md) — complete platform setup
- [Open-source policy](open-source.md) — what is free and what the paid build funds
- [Releasing](releasing.md) — maintainer-only signing and Polar distribution
- [README](../README.md) — product overview and developer setup
