# Alfred v0.1.0 release TODO

**Status:** Not ready to publish  
**Primary deliverables:** signed and notarized macOS `.dmg` installers and an
explicitly unsigned-beta Windows NSIS `.exe` installer delivered through the
paid official download channel. The public GitHub repository distributes
source code only.

This is the launch gate. The detailed implementation notes remain in
[plan 004](../plans/004-release-signing-secrets.md),
[plan 005](../plans/005-homebrew-cask.md), and
[plan 006](../plans/006-in-app-updater-dmg-exe.md).

## What already works

- [x] Tauri is configured to bundle macOS DMG, Windows NSIS/MSI, and Linux
  packages.
- [x] The manual GitHub Actions release workflow has macOS ARM64, macOS Intel,
  Windows, and Linux jobs.
- [x] `package.json`, `src-tauri/Cargo.toml`, and
  `src-tauri/tauri.conf.json` all use version `0.1.0`.
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

## P0 — required before publishing

### 1. Put the release source on GitHub

Verified on 2026-08-13: the repository has a `main` branch, an `origin` remote,
GitHub Actions enabled, and default workflow permissions set to read/write.

- [x] Review the current staged, modified, and untracked files; exclude local
  scratch/generated files from the release commit.
- [ ] Create the release commit on `main` and push it to the final GitHub
  repository.
- [x] Confirm the repository owner/name and add the `origin` remote.
- [x] In GitHub, set Actions workflow permissions to **Read and write**.
- [x] Confirm `.github/workflows/release.yml` is visible under **Actions →
  release**.

### 2. Decide the v0.1.0 product scope

- [x] Distribution decision: official maintainer-built binaries are paid;
  source is GPL-3.0-or-later and may be compiled without an Alfred purchase.
- [ ] Reconcile freemium/license plans `001`–`003` with free source builds
  before executing them. GPL licensing does not itself enforce payment and
  users may modify the source, including entitlement code.
- [ ] Freeze the features included in v0.1.0; connected apps (`008`–`017`),
  authenticated updates, and other planned work should not silently expand the
  first release.
- [ ] Confirm the final app name `Alfred`, bundle ID
  `com.nirzhuk.alfred`, minimum macOS version, and Windows 10/11 support.

### 3. Make the release workflow internally consistent

- [x] Replace the unsupported `includeUpdaterJson` action input. The current
  `tauri-apps/tauri-action@v1` input is `uploadUpdaterJson`.
- [ ] Decide the update policy for v0.1.0:
  - If automatic updates ship, complete plan 006, configure the updater public
    key and endpoint, generate signed updater artifacts, and keep
    `uploadUpdaterJson: true`.
  - If updates are deferred, keep `uploadUpdaterJson: false` and remove or
    replace the current **Check for Updates** stub so the shipped UI does not
    promise a feature that is unavailable.
- [x] The staging workflow currently uses `uploadUpdaterJson: false`; an
  authenticated paid update service is deferred.
- [ ] Run the workflow once as an unsigned draft to prove all matrix jobs and
  artifact uploads work before adding signing complexity.
- [ ] Pin/freeze the release ref and rerun the workflow from that exact commit.

### 4. Sign the public installers

#### macOS DMG — required

Verified on 2026-08-13: the local keychain contains a valid Developer ID
Application identity and all six required secret names exist in GitHub.

- [x] Enroll in/confirm the paid Apple Developer Program.
- [x] Create a **Developer ID Application** certificate and export its `.p12`.
- [x] Add `APPLE_CERTIFICATE`, `APPLE_CERTIFICATE_PASSWORD`,
  `KEYCHAIN_PASSWORD`, `APPLE_ID`, `APPLE_PASSWORD`, and `APPLE_TEAM_ID` as
  GitHub Actions secrets (plan 004).
- [ ] Confirm both ARM64 and Intel `.app` bundles are signed.
- [ ] Confirm both DMGs are notarized and stapled.
- [ ] Verify a downloaded DMG on a clean Mac with `codesign`, `spctl`, and
  `xcrun stapler validate`.

#### Windows EXE — required for a warning-free public release

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

- [ ] Confirm `0.1.0` is still the intended version and keep it identical in
  all three version files.
- [ ] Run **Actions → release → Run workflow** from the frozen release commit.
- [ ] Require these assets in the draft release:
  - [ ] Apple Silicon `.dmg`
  - [ ] Intel `.dmg`
  - [ ] Windows NSIS `-setup.exe`
- [ ] Inspect the optional `.msi`, `.AppImage`, `.deb`, and `.rpm` assets; do
  not let failures in advertised platforms pass unnoticed.
- [ ] Generate and publish SHA-256 checksums for the downloadable installers.
- [ ] Check that filenames include the product, version, architecture where
  relevant, and an unambiguous installer extension.

### 6. Smoke-test the actual downloaded artifacts

Do this with files downloaded from the draft release, not local build output.

- [ ] Clean Apple Silicon Mac: install from DMG, first launch, quit/relaunch,
  and uninstall.
- [ ] Clean Intel Mac: repeat the same flow.
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

### 7. Deliver through the paid channel

- [ ] Write release notes with supported OS versions, supported agent CLIs,
  known limitations, and the fact that automations run only while Alfred is
  open/tray-running.
- [ ] Configure the official storefront/download service and add its purchase
  link to the README/install guide.
- [ ] Upload accepted draft artifacts and SHA-256 checksums to that service;
  never publish the staging GitHub Release.
- [ ] Tag the exact release commit and provide its Corresponding Source beside
  the binary download at no additional charge.
- [ ] Complete a real purchase/download and verify entitlement, every customer
  asset, and the checksum instructions.
- [ ] Verify the public repository exposes source and documentation but no
  maintainer-signed paid installer.
- [ ] Define a support/bug-report URL and identify who will triage release
  issues.

## P1 — may follow the first paid release

- [ ] Design authenticated signed updates that preserve paid download access
  before implementing plan 006.
- [ ] Improve authentication-error feedback (plan 007) if not pulled into P0.
- [ ] Add automated clean-install/smoke coverage for release artifacts.
- [ ] Decide whether Linux packages are fully supported or best-effort, and
  align README/release wording with that decision.

## Release is done when

The release is complete only when the paid channel contains a tested,
signed/notarized ARM64 DMG, a tested, signed/notarized Intel DMG, and a tested,
explicitly unsigned-beta Windows NSIS EXE; purchase and download links work;
checksums are published; the GitHub staging draft remains private; and
customer-facing notes describe prerequisites and known limitations accurately.
