# Plan 004: Publish official installers as public GitHub Release assets

> **Executor instructions**: read [docs/releasing.md](../../docs/releasing.md)
> (the operator runbook this plan defers to) and the
> [signing reference](reference-verified-installer-signing.md). Distribution
> changed on **2026-08-25**: Alfred's official binaries are **free** and are
> published as **public GitHub Release assets** by a tag push. This plan no
> longer delivers anything through Polar. Do not enable an automatic updater,
> live sales, or a Polar download boundary in this plan. Follow every gate,
> stop on a STOP condition, and update the release-money index when done.
>
> **Drift check (run first)**:
> `git diff --stat ecb94d6..HEAD -- .github/workflows/release.yml src src-tauri docs README.md plans/release-money`
> Reconcile the build matrix, version files, signing decisions, menu behavior,
> and distribution copy before editing.

## Status

- **Priority**: P0
- **Effort**: M
- **Risk**: MEDIUM (public binary distribution custody and GPL source parity)
- **Depends on**: `reference-verified-installer-signing.md` only — no longer on
  Plan 003
- **Category**: release infrastructure, docs
- **Planned at**: commit `ecb94d6`, 2026-08-15
- **Rewritten at**: 2026-08-20 against the two-product perpetual model
  ([007](007-two-product-perpetual-model.md)).
- **Rewritten again at 2026-08-25** for the free/public distribution pivot:
  Alfred ships as open source with free public binaries, so the Polar File
  Downloads delivery channel, the private-draft-only rule, and all
  purchase-gated download copy are **superseded**. Plans 003/005 and
  RECONCILIATION still cite this plan's old Polar-era steps; treat those
  citations as historical until licensing is either revived (new plan required)
  or formally dropped.

## Why this matters

Distributing binaries publicly under GPL-3.0-or-later requires that the exact
corresponding source is reachable, that users can verify what they download,
and that nothing reaches the public before the gates prove it. The release
pipeline is therefore: build → stage privately → verify on clean runners →
attach checksums → publish. A red gate leaves the release a private draft.

## Current state (implemented and verified)

- `.github/workflows/release.yml` triggers on `v*` tag push (manual dispatch
  kept for test builds). It builds macOS Apple Silicon/Intel DMGs, Windows NSIS
  EXE + MSI, and Linux `.deb`/`.rpm`/`.AppImage`.
- `verify-version` enforces lockstep across `package.json`,
  `src-tauri/Cargo.toml`, and `src-tauri/tauri.conf.json`, and rejects a pushed
  tag that differs from `v<version>`.
- Artifacts land on a **draft** release first. Clean-runner gates download the
  exact staged DMGs/EXE and exercise install → launch twice → uninstall;
  macOS gates additionally verify `codesign`, `spctl`, and
  `stapler validate`; the Windows gate asserts the installer is **unsigned**
  (waived Authenticode beta).
- The `publish-release` job attaches `SHA256SUMS.txt` to the release, uploads
  an acceptance-manifest artifact (schemaVersion 2, `distribution:
  "public-github-releases"`: version, source commit, filenames, sizes,
  architectures, signing status, SHA-256; never secrets), then flips the draft
  public with `--latest`. A failed gate means never published.
- macOS Developer ID signing/notarization/stapling proven end-to-end
  (2026-08-13, run 31695713076; flow unchanged since).
- The in-app **Help → Download Latest Version…** action opens
  `https://github.com/Nirzhuk/alfred-workflows/releases/latest`
  (`LATEST_RELEASES_URL` in
  `src/features/licensing/download-latest.ts`); the opener capability already
  allowed `https://github.com/*`. Frontend tests cover the success and
  browser-failure paths.
- Living docs (README, install, open-source, releasing, release-todo,
  BRANDING, building-from-source) state the free/public model.
- `ALFRED_RELEASE_DATE` is deliberately **unset** in distribution builds: unset
  means the update-window logic never locks anything, which matches free
  distribution. Revisit only inside a future licensing plan.

## Commands you will need

| Purpose | Command | Expected on success |
| --- | --- | --- |
| Full local check | `bun run check` | all gates pass |
| Workflow YAML parse | `ruby -ryaml -e "YAML.load_file('.github/workflows/release.yml')"` | no output |
| Inspect release | `gh release view v<VERSION> --json assets,isDraft,isLatest` | `isDraft:false`, required assets + `SHA256SUMS.txt` listed |
| macOS verify | `codesign --verify --deep --strict <app>` and `xcrun stapler validate <dmg>` | exit 0 |
| Hash | `shasum -a 256 <installer>` | matches `SHA256SUMS.txt` |
| Updater guard | `rg -n 'uploadUpdaterJson|createUpdaterArtifacts|plugins.*updater' .github src-tauri` | `uploadUpdaterJson: false`; no enabled updater config |
| Hygiene | `bun run verify:release-hygiene` | PASS on all scans |

## Scope

**In scope**:

- `.github/workflows/release.yml` and focused release scripts;
- the in-app download-latest action and its tests;
- `src-tauri/capabilities/default.json` only for approved release-page URLs;
- `README.md`, `docs/install.md`, `docs/open-source.md`, `docs/releasing.md`,
  `docs/release-todo.md`, `BRANDING.md`, `docs/building-from-source.md`;
- this plan and the release-money index status.

**Out of scope**:

- Polar products, checkout links, portal configuration, or any Polar-hosted
  downloads — reviving paid licensing requires a **new written plan** that
  states what payment buys now that official builds are free;
- Tauri updater plugin, updater JSON, or any automatic update service;
- public Homebrew cask or third-party CDN mirrors (separate deferred decisions);
- Windows signing beyond the accepted unsigned-beta waiver;
- `api-licenses/` or any server deployment.

## Git workflow

- Branch: `codex/004-public-github-release`.
- Use imperative commits such as `Publish official installers on public GitHub releases`.

## Steps

### Step 1: Convert every customer/operator surface to the free public model — DONE

README (positioning, options table, manual-update paragraph, platforms,
contributing tail), `docs/install.md` (two free routes, GitHub Releases
instructions, checksum verification against `SHA256SUMS.txt`, updating),
`docs/open-source.md` (rewritten policy), `docs/releasing.md` (rewritten
runbook incl. rollback), `docs/release-todo.md` (owner checklist),
`BRANDING.md`, `docs/building-from-source.md`.

**Verify**:

```bash
rg -n 'File Downloads|customer portal|one-time purchase|paid download' \
  README.md docs BRANDING.md
```

Expected: only historical/archival mentions (`docs/polar-operator-handoff.md`,
`docs/release-acceptance/`) and explicit model-change statements.

### Step 2: Make the in-app update action truthful — DONE

**Download Latest Version…** opens the fixed public releases URL in the system
browser; Alfred never resolves or fetches an installer itself. Browser failure
shows the manual URL plus rebuild-from-source instructions. No updater plugin,
key, or manifest exists; `uploadUpdaterJson: false` stays enforced by
`bun run verify:release-hygiene`.

**Verify**: frontend tests cover open-success and browser-failure paths;
`bun run check` passes.

### Step 3: Tag-triggered gated publication pipeline — DONE

As described under *Current state*: lockstep versions, tag-equals-version gate,
private draft staging, clean-runner installer smoke gates, checksum
attachment, acceptance-manifest artifact, publish-with-`--latest` only after
every gate passes.

**Verify**: `actionlint` (or YAML parse) clean; one full green run observed
before the next real tag.

### Step 4: Cut v1.0.0 — IN PROGRESS

State ready locally: commit `87b3020 Bump version to 1.0.0` and annotated tag
`v1.0.0`, full `bun run check` green on that exact tree, working tree clean,
WIP preserved in `stash@{0}`.

Remaining:

- [ ] Push `git push origin main v1.0.0` (owner go-live decision).
- [ ] Watch Actions → release until published; confirm both DMGs, NSIS EXE,
      MSI, Linux packages, and `SHA256SUMS.txt` on the public release.
- [ ] Polish the release-notes body (supported OSes, agent CLIs, unsigned-beta
      Windows warning, manual-update policy, GPL notice + tagged source link).
- [ ] Spot-download one artifact and match it against `SHA256SUMS.txt`.

**Verify**: `gh release view v1.0.0` shows `isDraft:false`, `isLatest:true`,
and the complete asset list; hashes match.

### Step 5: Post-launch maintenance — TODO

- Keep every tag that has a published release: the tag **is** the GPL
  corresponding-source anchor named in the release body. Deleting it breaks the
  release's source links.
- Rollback per the runbook: unmark latest / delete assets / delete release and
  tag together.
- If the repository is ever renamed, update `LATEST_RELEASES_URL` and ship it —
  released binaries keep pointing at the old slug forever.

## Test plan

- Workflow gates: version alignment, tag equality, required filenames,
  architectures, checksums, draft-before-publish, absence of updater config.
- Packaged smoke tests use the exact downloaded draft artifacts on native
  runners (both DMG architectures + Windows silent install/uninstall).
- Frontend tests cover the manual download action and its failure path.
- `bun run verify:release-hygiene` guards architecture copy, secrets, and the
  updater-off invariant.

## Done criteria

- [x] Living docs describe only the free public-GitHub-Releases architecture.
- [x] No surface sells annual/lifetime/subscription tiers or paid downloads.
- [x] The app opens the public releases page instead of promising automatic
      updates.
- [x] CI stages a draft, gates it on clean runners, attaches SHA256SUMS.txt,
      and publishes only when every gate passes.
- [x] Both macOS DMGs pass downloaded-artifact smoke tests; Windows NSIS
      passes install/launch×2/uninstall and stays verifiably unsigned.
- [x] Corresponding source (the tag) is linked from every release body.
- [x] No signing private key, token, or secret ships in Alfred or CI artifacts.
- [ ] v1.0.0 (or the first agreed version) is actually published with matching
      checksums.
- [ ] `bun run check`, hygiene scan, and YAML parse pass at HEAD.
- [ ] The roadmap row is updated to `DONE`.

## STOP conditions

- A release goes public while any gate is red.
- Published bytes differ from the attached `SHA256SUMS.txt`.
- A published release loses its corresponding-source tag/link (GPL violation).
- Windows is marketed as signed or warning-free anywhere.
- An automatic updater or asset backend is introduced without a separate
  approved plan.
- The repository is renamed without updating `LATEST_RELEASES_URL` and cutting
  a follow-up release.

## Maintenance notes

The distribution boundary is now "what CI proved before publishing", not a
payment wall. Re-read the runbook after any workflow edit; the gates are the
only thing standing between a broken build and the public. If paid licensing
returns, write a fresh delivery plan — do not resurrect this plan's removed
Polar sections silently.
