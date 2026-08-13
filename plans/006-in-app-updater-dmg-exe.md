# Plan 006: Signed in-app updates for direct DMG/EXE installs

> **Executor instructions**: Follow this plan step by step. Run every
> verification command and confirm the expected result before moving to the
> next step. If anything in the "STOP conditions" section occurs, stop and
> report — do not improvise. When done, update the status row for this plan in
> `plans/README.md`.
>
> **Drift check (run first)**: Run `rg -n "checkUpdates|alfred:check-updates|Automatic updates" src`
> and read the complete event handler plus its menu wiring. At plan time it is
> still a hardcoded `0.1.0` alert. If a real updater implementation or different
> event contract has landed, stop and reconcile this plan before editing.

## Status

- **Priority**: P1
- **Effort**: L
- **Risk**: HIGH (a bad key, channel decision, or manifest can strand installs)
- **Depends on**: plan 004 `DONE`; plan 005 `DONE`; final GitHub owner/repo known
- **Category**: direction
- **Planned at**: unversioned snapshot 2026-08-11

## Why this matters

Users who installed a direct DMG or Windows NSIS EXE should receive signed
updates without replacing their data. Homebrew owns upgrades for cask installs
and must never be bypassed. Linux deb/rpm/AppImage installs remain a documented
re-download channel in this plan and must not enter the self-update flow.

Tauri update signatures are independent of Apple notarization and Windows
Authenticode. All three trust systems must remain enabled for their respective
purposes.

## Current state

The current `workflow-canvas.tsx` handler only shows:

```tsx
window.alert(
  "Alfred 0.1.0\n\nAutomatic updates aren’t set up yet. You’ll get a notice here once they are.",
);
```

- The event is wired from the native menu to `alfred:check-updates`.
- No updater or process plugin is installed.
- `tauri.conf.json` has no updater public key, endpoint, or
  `createUpdaterArtifacts` setting.
- Capabilities lack updater and process-restart permissions.
- The release workflow passes updater signing environment variables but uses
  the unsupported `includeUpdaterJson` action input. Current
  `tauri-apps/tauri-action@v1` uses `uploadUpdaterJson` and defaults to MSI when
  both MSI and NSIS are present unless `updaterJsonPreferNsis` is enabled.
- Plan 005 defines the Homebrew token/receipt behavior needed here. Executable
  path matching is not reliable because brewed apps run from `/Applications`.

## Commands you will need

| Purpose | Command | Expected |
| --- | --- | --- |
| Generate keys | `bunx tauri signer generate -w ~/.tauri/alfred.key` | private + `.pub` files |
| Add updater | `bun run tauri add updater` | Rust/JS plugin configured |
| Add process plugin | `bun run tauri add process` | Rust/JS plugin configured |
| Frontend build | `bun run build:frontend` | exit 0 |
| Rust tests | `cargo test --manifest-path src-tauri/Cargo.toml` | all pass |
| Inspect release | `gh release view <tag> --json assets,isDraft,isPrerelease` | update assets shown |

## Scope

**In scope**:

- Generate, back up, and configure the Tauri updater keypair
- `tauri-plugin-updater` and `tauri-plugin-process` Rust + JavaScript bindings
- Updater/restart capabilities with least privilege
- GitHub `latest.json` endpoint and signed update artifacts
- NSIS selection for Windows updates
- Receipt-based Homebrew detection that fails closed
- Explicit exclusion of Linux package installs from self-update UI
- Manual check/download/install UX with progress and platform-specific restart
- End-to-end tests across two signed published versions

**Out of scope**:

- Apple/Windows platform signing (plan 004)
- Homebrew cask authoring (plan 005)
- Delta patches, rollback, or a custom production update service
- Self-updating deb/rpm/AppImage installs
- Quiet startup checks until the manual flow is proven in production

## Steps

### Step 0: Confirm prerequisites and create updater keys

Before generating a key:

1. Confirm plans 004 and 005 are `DONE`.
2. Confirm the final public GitHub owner/repository and release URL.
3. Confirm who owns recovery and release access for updater secrets.

Generate the keypair once:

```bash
bunx tauri signer generate -w ~/.tauri/alfred.key
```

- Store the private-key **contents** in `TAURI_SIGNING_PRIVATE_KEY`.
- Store `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` only if a password was set.
- Store an encrypted recovery copy outside GitHub Actions with access limited to
  release maintainers.
- The `.pub` contents are safe to commit in `tauri.conf.json`; a path is not
  valid for the updater `pubkey` setting.
- Never print or commit the private key.

**Verify**:

```bash
test -f ~/.tauri/alfred.key
test -f ~/.tauri/alfred.key.pub
gh secret list
```

Confirm the private key is recoverable from the approved backup before shipping
the first updater-enabled release.

### Step 1: Add updater and restart dependencies

Add the updater plugin for desktop targets and initialize it in `lib.rs`:

- Rust: `tauri-plugin-updater = "2"`
- JavaScript: `@tauri-apps/plugin-updater`
- Initialization: `tauri_plugin_updater::Builder::new().build()`

Add the process plugin for relaunching after macOS installation:

- Rust: `tauri-plugin-process = "2"`
- JavaScript: `@tauri-apps/plugin-process`
- Initialization: `tauri_plugin_process::init()`

Prefer target-scoped Rust dependencies under the existing desktop
`cfg(any(target_os = "macos", windows, target_os = "linux"))` section.

Add capabilities:

- `updater:default`
- `process:allow-restart`

Do not grant unrelated process permissions.

**Verify**:

```bash
bun install
cargo check --manifest-path src-tauri/Cargo.toml
rg -n "plugin-updater|plugin-process|updater:default|process:allow-restart" package.json src-tauri
```

### Step 2: Configure artifacts, endpoint, and release action

In `src-tauri/tauri.conf.json`:

```json
"bundle": {
  "createUpdaterArtifacts": true
},
"plugins": {
  "updater": {
    "pubkey": "<CONTENTS OF alfred.key.pub>",
    "endpoints": [
      "https://github.com/<owner>/<repo>/releases/latest/download/latest.json"
    ],
    "windows": {
      "installMode": "passive"
    }
  }
}
```

Keep HTTPS and signature verification mandatory.

In `.github/workflows/release.yml`, replace the unsupported input and select
the same installer family users download directly:

```yaml
uploadUpdaterJson: true
uploadUpdaterSignatures: true
updaterJsonPreferNsis: true
```

Keep `TAURI_SIGNING_PRIVATE_KEY` and its optional password in the build
environment. The action must upload `latest.json`, signature files, macOS app
archives, and the signed NSIS update artifact.

Before the first public updater-enabled release, make NSIS the only advertised
Windows installer and change the Windows bundle matrix from `nsis,msi` to
`nsis`. Update install docs at the same time. This avoids installing an NSIS
update over an MSI-managed installation. If product requirements retain MSI,
STOP and design explicit installer-family detection plus a safe MSI policy;
one static `windows-x86_64` manifest entry cannot distinguish install history.

**Verify after a CI build**:

- `latest.json` contains valid entries for `darwin-aarch64`,
  `darwin-x86_64`, and `windows-x86_64`.
- The Windows URL selects the NSIS setup EXE, not MSI.
- Every manifest signature matches the corresponding `.sig` contents.
- If Linux entries are generated by the shared build configuration, they are
  structurally valid even though the UI does not invoke self-update on Linux.

### Step 3: Implement install-channel detection that fails closed

Expose a Rust command returning both platform and channel:

```text
platform: "macos" | "windows" | "linux"
channel:  "brew" | "direct" | "unsupported" | "unknown"
```

Rules:

- **Windows**: after the Step 2 matrix/docs change, supported v1 installers are
  direct NSIS installs → `direct`. If MSI remains available, detect it and
  return `unsupported` or `unknown`; never cross-update it with NSIS.
- **Linux**: return `unsupported`; the UI links to release downloads and never
  calls `downloadAndInstall`.
- **macOS Homebrew**:
  1. Look for `brew` using the existing PATH/common-location strategy,
     including `/opt/homebrew/bin/brew` and `/usr/local/bin/brew`.
  2. Run `brew list --cask --versions alfred` without invoking a shell.
  3. A successful non-empty result means `brew`.
  4. If Homebrew execution fails but a current Alfred receipt exists under a
     known Caskroom prefix, return `brew` or `unknown`, never `direct`.
- **macOS direct**: no installed cask receipt and the running app is a normal
  signed bundle → `direct`.
- Any ambiguous/error state → `unknown`.

Only `direct` may enter the install flow. `brew`, `unsupported`, and `unknown`
must never call updater installation APIs.

Do not use `current_exe()` path containing `Homebrew`/`Caskroom` as the primary
signal; the cask installs `Alfred.app` into `/Applications`.

**Verify with unit tests using an injected command/path probe**:

- Apple Silicon Homebrew receipt
- Intel Homebrew receipt
- Homebrew binary missing but receipt present
- Homebrew installed but Alfred cask absent
- Direct DMG install
- Ambiguous/stale receipt failure
- Windows direct and Linux unsupported

Also install the real plan-005 cask and confirm the running app returns `brew`.

### Step 4: Replace the update UI stub

Create a focused updater module/hook rather than expanding the canvas event
handler. Preserve the existing menu event and dialog style.

Behavior:

1. Prevent concurrent checks/downloads from repeated menu clicks.
2. Read platform/channel from the Rust command.
3. `brew` → show `brew upgrade --cask alfred`; never call updater install.
4. `unsupported` → explain that Linux updates use release downloads.
5. `unknown` → show a safe manual-download path; never install automatically.
6. `direct` → call `check()` from `@tauri-apps/plugin-updater`.
7. No update → show the installed version from `getVersion()`.
8. Update available → show version/notes, request confirmation, then call
   `downloadAndInstall` with progress.
9. macOS → call `relaunch()` after successful installation.
10. Windows → the updater exits the app during installer execution; do not rely
    on code after `downloadAndInstall()` running. Tell the user beforehand that
    Alfred will close and restart/reopen as appropriate.

Remove the hardcoded `0.1.0` and the “Automatic updates aren’t set up” text.
Do not add quiet startup checks in this plan's first implementation.

**Verify**:

```bash
bun run build:frontend
rg -n "Automatic updates aren’t set up|Alfred 0.1.0" src
```

The `rg` command must return no matches.

### Step 5: Run a real two-version update test

The production endpoint uses `/releases/latest`, which resolves only a
published full release—not a draft or prerelease. Do not claim end-to-end
coverage using draft assets.

Before the first public updater release, use a staging GitHub repository or
other controlled static endpoint compiled into staging builds:

1. Build and install signed/notarized staging version `0.1.0`.
2. Publish a full staging release for signed version `0.1.1` using the same
   updater key.
3. Confirm the staging `latest.json` URL resolves without authentication.
4. Update direct installs on Apple Silicon, Intel macOS, and Windows NSIS.
5. Verify user data, workflows, schedules, and settings survive.
6. Install through the real Homebrew cask and prove no updater network/install
   call occurs.
7. On Linux, prove the UI stays on the manual-download path.
8. Tamper with a staged artifact/signature and confirm installation is rejected.
9. Inspect Windows installed-app/uninstaller identity after updating to ensure
   NSIS did not switch to MSI.

Never ship a production build containing the staging endpoint. Repeat a
production smoke check after the first full release is published.

### Step 6: Document release maintenance

Update `docs/releasing.md`:

- Direct DMG/NSIS installs consume `latest.json`.
- Homebrew uses `brew upgrade --cask alfred`.
- Linux uses release downloads in v1.
- CI must retain `uploadUpdaterJson: true`,
  `updaterJsonPreferNsis: true`, updater signatures, and the private-key secret.
- Windows install docs advertise NSIS only unless a separate MSI-safe updater
  design exists.
- A release must not be published if any required manifest platform entry or
  signature is missing.

## Done criteria

- [ ] Updater and process plugins are configured with least-privilege capabilities
- [ ] Updater public key and GitHub endpoint are embedded; private key is backed up
- [ ] Release CI uses `uploadUpdaterJson` and prefers NSIS
- [ ] Homebrew detection uses a tested receipt signal and fails closed
- [ ] Brew, unknown, and Linux channels never call `downloadAndInstall`
- [ ] Direct ARM64 DMG, Intel DMG, and Windows NSIS installs update successfully
- [ ] A tampered signature is rejected
- [ ] Windows stays on the NSIS installer family and user data survives updates
- [ ] MSI is no longer advertised/published, or MSI installs are safely detected
  and excluded from the NSIS updater path
- [ ] Stub text and hardcoded version are removed
- [ ] Frontend build and Rust tests pass
- [ ] `plans/README.md` status row for 006 is `DONE`

## STOP conditions

- Plan 004 or plan 005 is not `DONE`
- Final GitHub owner/repository or production endpoint is unknown
- Updater private key has no verified recovery copy
- Homebrew detection cannot distinguish an installed receipt safely
- Required platform entries/signatures are missing or malformed in `latest.json`
- Windows manifest selects MSI instead of the supported NSIS channel
- MSI remains publicly installable without reliable installer-family detection
- End-to-end testing is attempted only with draft/prerelease assets
- Any implementation requires disabling TLS or signature verification

## Maintenance notes

- Losing the updater private key prevents trusted updates for installed clients;
  recovery requires a coordinated reinstall.
- Never rotate the updater key casually. If rotation is necessary, first ship a
  client capable of trusting both old and new keys.
- GitHub `/releases/latest` ignores drafts and prereleases.
- Re-test Homebrew receipt behavior after changes to the cask token or tap.
- Revisit Linux only as a separate, package-manager-aware plan.
