# Plan 004: Sign and notarize public DMG/EXE installers

> **Executor instructions**: Follow this plan step by step. Run every
> verification command and confirm the expected result before moving to the
> next step. If anything in the "STOP conditions" section occurs, stop and
> report — do not improvise. When done, update the status row for this plan in
> `plans/README.md`.
>
> **Drift check (run first)**: Compare SHA-256 of
> `.github/workflows/release.yml` and `docs/releasing.md` to the hashes in
> "Current state". If either differs, re-read both files and reconcile the
> release matrix, secret names, and verification steps before proceeding.

## Status

- **Priority**: P0
- **Effort**: M
- **Risk**: HIGH (external certificates, notarization, and CI state)
- **Depends on**: a committed GitHub repository with Actions enabled
- **Category**: dx
- **Planned at**: unversioned snapshot 2026-08-11

## Why this matters

The public release needs macOS DMGs that pass Gatekeeper without a bypass and
a Windows NSIS EXE with a verifiable publisher. Tauri updater signatures are a
separate trust system and belong to plan 006; they are not a completion
condition for operating-system signing.

## Current state

- `.github/workflows/release.yml` builds macOS ARM64 + Intel DMGs, Windows NSIS
  + MSI, and Linux packages into a draft GitHub Release. Apple certificate
  import is conditional. SHA-256 at plan time:
  `00fadd03259c1849654aec3e18b933e88736b1a927e5da76bc42a376b40e547c`.
- `docs/releasing.md` documents the Apple secret names. SHA-256 at plan time:
  `7eb254181923b41c21ea2b9b6e37e1fa14e860b046bb4b8d8166020c0f1ec55b`.
- The workspace has no Git `HEAD` and no `origin` remote, so CI cannot run yet.
- A local Apple Silicon DMG was built and verified on 2026-08-11, but it is
  unsigned. `security find-identity -v -p codesigning` reported no valid local
  identities.
- The workflow still contains the legacy `includeUpdaterJson` input. Plan 006
  replaces it when the updater is configured; `latest.json` is not required by
  this plan.

## Commands you will need

| Purpose | Command | Expected on success |
| --- | --- | --- |
| Local frontend | `bun run build:frontend` | exit 0 |
| Rust tests | `cargo test --manifest-path src-tauri/Cargo.toml` | all pass |
| Workflow syntax | `actionlint .github/workflows/release.yml` | no errors |
| List secret names | `gh secret list` | expected names are present |
| Inspect release | `gh release view v0.1.0 --json assets,isDraft,url` | draft and assets shown |

## Scope

**In scope**:

- GitHub repository/Actions prerequisites for the release workflow
- Developer ID signing and notarization for both macOS architectures
- Authenticode signing for the Windows NSIS artifact and any MSI retained in
  the public release
- One successful signed draft release and clean-machine verification
- Small release workflow/configuration changes required by the chosen signing
  providers

**Out of scope**:

- Tauri updater key generation, updater artifacts, or `latest.json` (plan 006)
- Homebrew cask creation (plan 005)
- Polar freemium plans 001–003
- Publishing the final release before smoke testing

## Steps

### Step 0: Establish the GitHub release source

1. Review staged, modified, and untracked files and exclude local scratch or
   generated output from the release commit.
2. Create a release commit on `main`, add the final GitHub repository as
   `origin`, and push it.
3. In GitHub, set Actions workflow permissions to **Read and write**.
4. Confirm **Actions → release → Run workflow** is available.

**Verify**:

```bash
git rev-parse --verify HEAD
git remote get-url origin
gh repo view --json nameWithOwner,url
```

### Step 1: Obtain Apple Developer materials

Obtain outside the repository:

1. Active paid Apple Developer Program membership
2. **Developer ID Application** certificate exported as a password-protected
   `.p12`
3. App-specific password for the notarization Apple ID
4. The 10-character Apple Team ID

Never add the `.p12`, its password, or notarization credentials to the
repository.

**Verify** on a Mac where the certificate is installed:

```bash
security find-identity -v -p codesigning
```

At least one `Developer ID Application` identity must be listed.

### Step 2: Configure macOS CI signing and notarization

Set repository secrets whose names match `.github/workflows/release.yml`:

- `APPLE_CERTIFICATE` — single-line base64 of the `.p12`
- `APPLE_CERTIFICATE_PASSWORD`
- `KEYCHAIN_PASSWORD` — strong random password for the ephemeral CI keychain
- `APPLE_ID`
- `APPLE_PASSWORD` — app-specific password
- `APPLE_TEAM_ID`

`gh secret list` only verifies names, not values. Validate values through a
signed CI run; never print secret contents.

**Verify**:

```bash
gh secret list
```

All six names listed above must be present.

### Step 3: Configure Windows Authenticode signing

Choose a current signing method supported by the certificate provider and
Tauri, such as a managed signing service/custom sign command or a certificate
available in the Windows runner certificate store. Do not assume the legacy
thumbprint-only OV flow applies to newly issued certificates.

For a certificate-store flow:

1. Store the base64-encoded `.pfx` and its password as GitHub secrets.
2. Import it only in the Windows job and delete temporary certificate files.
3. Configure `bundle.windows.certificateThumbprint`, `digestAlgorithm:
   "sha256"`, and the certificate provider's timestamp URL.

For a managed provider, configure Tauri's `signCommand` and the minimum scoped
provider credentials recommended by that provider.

**Verify on the Windows CI runner**:

```powershell
signtool verify /pa /v path\to\Agentflow-setup.exe
Get-AuthenticodeSignature path\to\Agentflow-setup.exe
```

If MSI remains in the public artifact matrix, verify it with `signtool` too.

The signature must be valid, timestamped, and identify the intended publisher.
An OV certificate may still need time or Microsoft review to build SmartScreen
reputation; do not equate “signed” with guaranteed immediate reputation.

### Step 4: Run and verify a signed draft release

1. Ensure the version matches in `package.json`, `src-tauri/Cargo.toml`, and
   `src-tauri/tauri.conf.json`.
2. Run **Actions → release → Run workflow** from the frozen release commit.
3. Confirm the draft contains ARM64 DMG, Intel DMG, and NSIS EXE. If MSI is
   still advertised, require it too. Inspect Linux jobs separately, but do not
   require updater JSON in this plan.
4. Download the assets from the draft release rather than testing local output.
5. Verify both macOS architectures and the Windows artifacts.

**Verify macOS artifacts**:

```bash
codesign --verify --deep --strict --verbose=2 /Applications/Agentflow.app
spctl --assess --type execute --verbose=4 /Applications/Agentflow.app
xcrun stapler validate path/to/Agentflow.dmg
```

Also install and launch the ARM64 DMG on Apple Silicon and the Intel DMG on an
Intel Mac or an equivalent clean test environment.

**Verify Windows artifacts** with the commands from Step 3, then install and
launch the NSIS EXE on a clean Windows 10/11 x64 machine.

## Done criteria

- [ ] GitHub repository, `origin`, Actions permissions, and release workflow exist
- [ ] All required Apple secret names exist in GitHub
- [ ] Both macOS DMGs are Developer ID signed, notarized, and stapled
- [ ] Windows NSIS EXE and every other published Windows installer have valid
  timestamped Authenticode signatures
- [ ] A CI run produced a signed draft release with both DMGs and the EXE
- [ ] Downloaded artifacts passed clean-machine launch checks
- [ ] `plans/README.md` status row for 004 is `DONE`

If the operator explicitly chooses an unsigned Windows beta, record that in
the release checklist and leave 004 `BLOCKED (Windows signing deferred)` rather
than marking a public-signing plan complete.

## STOP conditions

- GitHub repository/remote or Actions write permission is unavailable
- Apple Developer Program membership is missing or expired
- A suitable Windows signing method/certificate is unavailable for the public release
- CI fails notarization or signing twice with the same error after validating configuration
- Workflow secret names no longer match this plan
- Anyone requests committing private keys, `.p12`, or `.pfx` files

## Maintenance notes

- Rotate leaked credentials immediately.
- Renew platform certificates before expiration and retain timestamping.
- Updater keys are independent of Apple/Windows signing; losing the updater
  private key has different recovery consequences and is handled in plan 006.
- Plan 005 must consume the notarized GitHub Release DMGs, never local unsigned
  output.
