# Alfred public-release TODO

**Status:** pipeline ready — owner follow-ups below.
**Model:** Alfred is free and open source (GPL-3.0-or-later). Official
maintainer-built installers are published as **public GitHub Release assets**
by [`.github/workflows/release.yml`](../.github/workflows/release.yml) when a
`vX.Y.Z` tag is pushed. macOS builds are Developer ID signed, notarized, and
stapled; Windows ships as an explicitly unsigned beta. There is no automatic
updater and no paid download boundary anymore.

The operator runbook lives in [releasing.md](releasing.md).

## What already works

- [x] Tag-push (`v*`) and manual-dispatch triggers; version lockstep gate
  across `package.json`, `src-tauri/Cargo.toml`, `src-tauri/tauri.conf.json`,
  plus tag-equals-version check.
- [x] Draft staging → clean-runner installer gates → checksum attachment →
  auto-publish. A failed gate leaves the release private.
- [x] macOS DMGs signed/notarized/stapled on both architectures (verified
  2026-08-13, run 31695713076; the flow is unchanged).
- [x] Windows NSIS EXE verified by CI: unsigned, silent install, two launches,
  uninstall.
- [x] `SHA256SUMS.txt` + acceptance manifest (JSON/txt) produced per run.
- [x] In-app **Help → Download Latest Version…** opens this repository's
  GitHub releases page (`LATEST_RELEASES_URL` in
  `src/features/licensing/download-latest.ts`); opener capability allows it.
- [x] All six Apple signing secrets present in GitHub; workflow permissions
  set to read/write (verified via API).

## Before pushing the first public tag

- [ ] Confirm the repository stays `Nirzhuk/alfred-workflows`. The releases
  URL is baked into released binaries; renaming the repo breaks the in-app
  Download Latest link for every shipped copy.
- [ ] Decide the first version number (currently `0.5.0` everywhere) and
  whether to ship it as-is or bump.
- [ ] Optional dry run: **Actions → release → Run workflow** from `main`,
  confirm all legs green and the draft looks right, then delete that draft.
- [ ] Write release notes for the first public release: supported OS versions,
  required agent CLIs, the unsigned-beta Windows warning, manual-update policy,
  known limitations, GPL notice with a link to the tagged source.

## Owner follow-ups (can trail the first release)

### Website

- [ ] Create the website. Minimum viable: landing page linking the GitHub
  releases page, docs, and source; state the license and the unsigned-beta
  Windows warning. The desktop app must stay Tauri-wrapped — do not turn Vite
  into a hosted site (see `scripts/guard-desktop-tauri.mjs`).

### Prod Polar configuration

Polar's original role (merchant of record for paid official binaries) is gone:
binaries are free and public. Before touching prod Polar, decide what it is
for now:

- [ ] **Donations/sponsorships only** — simplest path; no app changes needed.
- [ ] **Revive paid licensing** (support contracts, hosted features, priority
  builds?) — then revisit all of: checkout/portal links
  (`VITE_POLAR_*` env seam), the dormant in-app licensing UI, and possibly
  `ALFRED_RELEASE_DATE` wiring. Do not half-configure this.
- [ ] Whatever the choice: remove or fulfill every remaining Polar reference
  outside `plans/release-money/` (historical archive) so docs match reality.

### In-app licensing subsystem (dormant)

- [ ] Decide the fate of `src/features/licensing/` and
  `src-tauri/src/licensing/`: activation UI, key storage, and validation exist
  and are tested, but nothing gates features on them today ("Plan 008 owns
  the gating" per `models.rs`). Free distribution needs no gating; keep the
  code dormant or strip it deliberately — not both.

### Windows smoke test (gaming PC)

CI already proves silent install/launch/uninstall. On real hardware, check:

- [ ] SmartScreen warning appears as documented and the installer runs after
  "More info → Run anyway".
- [ ] Launch from the **Start menu** (not terminal): agent CLIs are detected.
- [ ] One real workflow end-to-end; output selection, history, and SQLite
  persistence survive restart.
- [ ] Tray/menu-bar behavior, a schedule, a file trigger, and the loopback
  webhook while running.
- [ ] Failure UX for a missing/unauthenticated CLI is acceptable.
- [ ] Upgrade-over-install keeps user data; uninstall behavior understood.

### Unlocked options (each needs its own decision)

- [ ] Automatic updater: public signed release assets make a Tauri updater
  feasible now. Requires updater keys/manifest plus relaxing
  `verify-release-hygiene`'s `uploadUpdaterJson: false` guard deliberately.
- [ ] Homebrew cask: blocked before only because public binaries would bypass
  the paywall; that reason is gone. Cask would need the repo layout it expects.
- [ ] Promote Linux packages from informational artifacts to advertised
  downloads once someone has actually exercised them.
