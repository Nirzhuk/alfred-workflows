# Alfred v0.5.0 release TODO

**Status:** Not ready to publish  
**Primary deliverables:** signed, notarized, and stapled macOS `.dmg`
installers for Apple Silicon and Intel, plus an explicitly unsigned-beta
Windows x64 NSIS `.exe` installer, delivered through Polar's File Downloads
benefit. Linux stays best-effort. The public GitHub repository distributes
source code only.

[Polar](https://polar.sh) is the merchant of record: it hosts checkout, the
customer portal, license keys, seats, and downloads. Alfred ships no payment
gateway, account service, license server, webhook receiver, email service,
server database, or server backup, and no Polar access token or webhook secret
ships in the app. v0.5.0 updates **manually** through Polar; there is no
automatic updater.

This is the launch gate. The detailed implementation notes live in
[`plans/release-money/`](../plans/release-money/README.md) — in particular
[plan 004: publish signed Polar downloads](../plans/release-money/004-publish-signed-polar-downloads.md)
and the
[verified installer-signing reference](../plans/release-money/reference-verified-installer-signing.md).
The earlier commercial entitlement/update gateway design is **rejected** and is
not part of this release path.

## What already works

- [x] Tauri is configured to bundle macOS DMG, Windows NSIS/MSI, and Linux
  packages.
- [x] The manual GitHub Actions release workflow has macOS ARM64, macOS Intel,
  Windows, and Linux jobs.
- [x] `package.json`, `src-tauri/Cargo.toml`, and
  `src-tauri/tauri.conf.json` all use version `0.5.0`.
- [x] Product name, bundle identifier, icons, GPL-3.0-or-later license, install guide, and
  release operator guide exist.
- [x] Source-build, contribution, security, conduct, and branding policies are
  documented; pull requests have an automated test/build workflow.
- [x] The release workflow stages private draft assets and explicitly prevents
  publishing paid official binaries to the public repository.
- [x] Verified locally on 2026-08-11: 16 frontend tests pass, 20 Rust tests
  pass, and the production frontend builds.
- [x] Verified locally on Apple Silicon on 2026-08-11: Tauri produced
  `Alfred_0.1.0_aarch64.dmg`, and `hdiutil verify` passed. This artifact is
  unsigned and is only proof that local packaging works.
- [x] Verified in GitHub Actions on 2026-08-13: run
  [31695713076](https://github.com/Nirzhuk/alfred-workflows/actions/runs/31695713076)
  built all advertised platforms, signed/notarized/stapled both macOS DMGs,
  and passed clean-runner installer smoke tests for native Apple Silicon,
  native Intel, and unsigned-beta Windows.

## P0 — required before publishing

### 1. Put the release source on GitHub

Verified on 2026-08-13: the repository has a `main` branch, an `origin` remote,
GitHub Actions enabled, and default workflow permissions set to read/write.

- [x] Review the current staged, modified, and untracked files; exclude local
  scratch/generated files from the release commit.
- [x] Create the release commit on `main` and push it to the final GitHub
  repository.
- [x] Confirm the repository owner/name and add the `origin` remote.
- [x] In GitHub, set Actions workflow permissions to **Read and write**.
- [x] Confirm `.github/workflows/release.yml` is visible under **Actions →
  release**.

### 2. Decide the v0.5.0 product scope

- [x] Distribution decision: official maintainer-built binaries are paid;
  source is GPL-3.0-or-later and may be compiled without an Alfred purchase.
- [x] Replace the old Free/Pro usage-limit plans with **two one-time products**:
  **Alfred License** (one named user, not seat-based) and **Alfred Teams**
  (one-time per claimed seat), where every claimed seat gets its own license key
  and downloads. Both grant paid features permanently plus **one year of
  updates**. Superseded the four-product annual/lifetime/seat model on
  2026-08-20; see `plans/release-money/007-two-product-perpetual-model.md`.
- [ ] Create the Polar products: **Alfred License** (standard one-time) and
  **Alfred Teams** (seat-based, one-time per seat). Create **two** license-key
  benefits (`individual`, `teams`), each with a three-activation limit and a
  **one-year key expiry**, and attach one shared File Downloads benefit to both
  products so every claimed Teams seat receives its own key and downloads.
  Prices live in Polar's dashboard only — do not restate them in this
  repository or compile them into Alfred.
- [ ] Bake `ALFRED_RELEASE_DATE` (ISO `YYYY-MM-DD`, supplied by the release
  workflow) into distribution builds and assert it in the acceptance manifest.
  Unset means a source build and must never lock anything.
- [ ] Complete legal review before claiming paid permission is required for
  commercial use. GPL licensing itself permits commercial source use; the
  current paid boundary is official builds, Polar-hosted downloads, hosted
  features, and support.
- [ ] Freeze the features included in v0.5.0; connected apps (`008`–`017`), an
  automatic updater, and other planned work must not silently expand the first
  release.
- [ ] Confirm the final app name `Alfred`, bundle ID
  `com.nirzhuk.alfred`, minimum macOS version, and Windows 10/11 support.

### 3. Make the release workflow internally consistent

- [x] Replace the unsupported `includeUpdaterJson` action input. The current
  `tauri-apps/tauri-action@v1` input is `uploadUpdaterJson`.
- [x] Update policy for v0.5.0 is **manual downloads through Polar**. Automatic
  updates are deferred because they would require either public signed updater
  assets or an authenticated manifest/asset service.
- [x] The staging workflow uses `uploadUpdaterJson: false`, and no Tauri updater
  dependency, plugin configuration, or signing key is present.
- [x] The **Check for Updates** stub is replaced by **Download Latest Version…**
  in the app menu and the tray menu. It opens Polar's customer portal through
  the allow-listed `src/features/licensing/public-links.ts` seam and explains
  that customers sign in by email to reach their personal downloads. Alfred
  never fetches an installer URL itself. An unconfigured or source build shows
  build-from-source instructions instead of a broken link.
- [x] The release workflow fails the run when `package.json`,
  `src-tauri/Cargo.toml`, and `src-tauri/tauri.conf.json` disagree on the
  version, and when a required draft artifact is missing or duplicated.
- [x] The release workflow emits a text + JSON acceptance manifest with the
  version, source commit, filenames, sizes, architectures, signing status, and
  SHA-256 checksums, and no signing secret.
- [ ] Run the workflow once as an unsigned draft to prove all matrix jobs and
  artifact uploads work before adding signing complexity.
- [x] Pin/freeze the release ref and rerun the workflow from that exact commit.

### 4. Sign the public installers

#### macOS DMG — required

Verified on 2026-08-13: the local keychain contains a valid Developer ID
Application identity and all six required secret names exist in GitHub.

- [x] Enroll in/confirm the paid Apple Developer Program.
- [x] Create a **Developer ID Application** certificate and export its `.p12`.
- [x] Add `APPLE_CERTIFICATE`, `APPLE_CERTIFICATE_PASSWORD`,
  `KEYCHAIN_PASSWORD`, `APPLE_ID`, `APPLE_PASSWORD`, and `APPLE_TEAM_ID` as
  GitHub Actions secrets (see the
  [signing reference](../plans/release-money/reference-verified-installer-signing.md)).
- [x] Confirm both ARM64 and Intel `.app` bundles are signed.
- [x] Confirm both DMGs are notarized and stapled.
- [x] Verify a downloaded DMG on a clean Mac with `codesign`, `spctl`, and
  `xcrun stapler validate`.

#### Windows EXE — signing waived, ships as unsigned beta

**Decision (2026-08-13):** Windows Authenticode signing is waived because there
is no signing budget. Any Windows installer is an explicitly unsigned beta and
must be labeled as such wherever it is offered. Users should expect an
unknown-publisher/SmartScreen warning. This release does not claim to be a
warning-free Windows release.

- [x] Accept an unsigned Windows beta and document the expected warning.

- [ ] Obtain a trusted Windows code-signing certificate.
- [ ] Configure the certificate thumbprint, SHA-256 digest, and timestamp URL
  in Tauri/CI.
- [ ] Confirm the NSIS installer and installed executable are Authenticode
  signed with `signtool verify /pa`.
- [ ] Confirm a fresh download does not show an unknown-publisher warning.

An unsigned Windows beta is technically installable, but shipping it must be
an explicit beta decision because SmartScreen will warn users.

### 5. Produce and inspect the draft release

- [x] Confirm `0.5.0` is still the intended version and keep it identical in
  all three version files.
- [x] Run **Actions → release → Run workflow** from the frozen release commit.
- [x] Require these assets in the draft release:
  - [x] Apple Silicon `.dmg`
  - [x] Intel `.dmg`
  - [x] Windows NSIS `-setup.exe`
- [x] Inspect the optional `.msi`, `.AppImage`, `.deb`, and `.rpm` assets; do
  not let failures in advertised platforms pass unnoticed.
- [x] Generate SHA-256 checksums for the required installers — the workflow's
  `acceptance-manifest` job produces them from the downloaded draft bytes.
- [ ] Publish that checksum manifest beside the Polar downloads.
- [x] Check that filenames include the product, version, architecture where
  relevant, and an unambiguous installer extension.

### 6. Smoke-test the actual downloaded artifacts

Do this with files downloaded from the draft release, not local build output.

The automated Windows gate installs the downloaded NSIS asset, launches the
installed executable twice, and uninstalls it. The Start-menu-specific item
below remains open until the shortcut itself is exercised manually.

- [x] Clean Apple Silicon Mac: install from DMG, first launch, quit/relaunch,
  and uninstall.
- [x] Clean Intel Mac: repeat the same flow.
- [ ] Clean Windows 10 or 11 x64 VM/device: install from NSIS EXE, launch from
  Start, quit/relaunch, and uninstall.
- [ ] Verify each supported agent CLI is detected when Alfred is launched
  from Finder/Start rather than a terminal.
- [ ] Run one real workflow end to end and confirm output selection, history,
  and SQLite persistence survive restart.
- [ ] Exercise tray/menu-bar behavior, a schedule, a file trigger, and the
  loopback webhook while the app is running.
- [ ] Verify failure UX for a missing CLI and an unauthenticated CLI; either
  complete plan 007 or accept and document the current behavior.
- [ ] Confirm user data survives an upgrade install and uninstall behavior is
  understood/documented.

### 7. Deliver through Polar

- [ ] Write release notes with supported OS versions, supported agent CLIs,
  known limitations, the unsigned-beta Windows warning, the manual-update
  policy, and the fact that automations run only while Alfred is
  open/tray-running.
- [ ] Set the three public Polar URLs for the build and replace every
  `TODO(polar-url)` marker in `README.md`, `docs/install.md`, and this file:
  - `VITE_POLAR_DESKTOP_CHECKOUT_URL` — the single in-app checkout link
    (Alfred License). Alfred Teams is sold on the marketing website and has no
    in-app checkout entry point.
  - `VITE_POLAR_CUSTOMER_PORTAL_URL` — customer portal
- [ ] Complete one Alfred License checkout/activation and one Alfred Teams
  checkout with at least two claimed seats.
- [ ] Verify Teams purchased/claimed/available seat counts, invitation
  acceptance, and seat removal in Polar. Record what Polar actually does when
  seats are added: under the current model that is a second one-time purchase,
  not a proration.
- [ ] Verify the three-device activation limit, the 7-day refresh, the 30-day
  offline tolerance, and immediate effect of a confirmed revocation.
- [ ] Upload the accepted draft artifacts, the SHA-256 checksum manifest, the
  GPL notices, and the corresponding-source link to Polar's File Downloads
  benefit. Add new files before disabling old ones; never publish the staging
  GitHub Release.
- [ ] Tag the exact release commit and provide its Corresponding Source beside
  the binary download at no additional charge.
- [ ] Verify an Alfred License purchase and a claimed Alfred Teams seat each
  download the exact accepted files, and that an unrelated or unclaimed customer
  cannot. A customer whose update year has lapsed **can** still download — that
  is expected behavior, not a defect.
- [ ] Verify an expired license key keeps entitlement (update window closed,
  purchase intact) while `revoked` and `disabled` end it immediately.
- [ ] Verify the public repository exposes source and documentation but no
  maintainer-signed paid installer.
- [ ] Define a support/bug-report URL and identify who will triage release
  issues.

## P1 — may follow the first paid release

- [ ] Keep the public Homebrew cask blocked unless an authenticated/private tap
  is approved; a public cask would bypass the Polar download boundary. See
  [deferred Homebrew distribution](../plans/release-money/deferred-homebrew-distribution.md).
- [ ] Decide whether an automatic updater is worth a separate product decision.
  It requires either public signed updater assets or a small authenticated
  update service. Do not reintroduce a general commerce backend for it.
- [ ] Improve authentication-error feedback (plan 007) if not pulled into P0.
- [x] Add automated clean-install/smoke coverage for release artifacts.
- [x] Linux packages are **best effort**, not a supported paid download. README,
  install guide, and release runbook say so. Promoting Linux to a supported paid
  download needs a separate operator decision and a matching update to the
  release workflow's required-artifact list.

## Release is done when

The release is complete only when Polar's File Downloads benefit contains a
tested, signed/notarized/stapled Apple Silicon DMG, a tested,
signed/notarized/stapled Intel DMG, and a tested, explicitly unsigned-beta
Windows x64 NSIS EXE; an Alfred License purchase and a claimed Alfred Teams
seat each reach those exact files while unauthorized customers cannot; checkout
and portal links work; the SHA-256 checksum manifest and the corresponding-source
link are published beside the binaries; the GitHub staging draft remains
private; and customer-facing notes describe the manual-update policy, the
unsigned-beta Windows warning, best-effort Linux, prerequisites, and known
limitations accurately.
