# Releasing Alfred

Before cutting a public release, complete the
[v0.5.0 release TODO](release-todo.md). This document is the operator runbook;
the TODO is the launch gate and artifact acceptance checklist.

Alfred's source is public under GPL-3.0-or-later, but official maintainer-built
installers are a paid product. Release automation therefore stages binaries in
a **draft** GitHub Release for maintainers only. Never publish that draft to the
public repository. After validation, upload the artifacts to the configured
paid storefront/download service.

Distribution boundary:

| Channel | Artifact | How users update |
| --- | --- | --- |
| Desktop License | Official signed/notarized desktop installers for one named user | Authenticated download and Tauri updater through the Alfred gateway |
| Company/Enterprise member seat | Same official desktop entitlement plus organization/hosted access | Authenticated member download and the same Tauri updater |
| Public source repository | Source only | Pull/clone a newer version and rebuild |
| Draft GitHub Release | Private staging for maintainers | Never user-facing; do not publish |

Commercial products follow the same inclusion rule as Cap: every paid Company
or Enterprise member seat includes the Desktop License. A standalone Desktop
License is sold per named user with annual/lifetime options; Company is billed
per active member with monthly/annual options. Prices and Stripe Price IDs are
server-side catalog data.

Under Alfred's current GPL-3.0-or-later license, these products control the
official signed distribution, authenticated updates, hosted organization
features, and support. They do not remove the GPL right to build or use the
source commercially. Do not publish commercial-use restriction copy unless a
separate dual-license/EULA and contributor-rights review is complete.

A public Homebrew cask or public GitHub Release asset would expose the paid
official binary without purchase, so neither is part of this model. Plan 022
defines the reviewed commercial entitlement/update gateway: Stripe is
server-only, CrabNebula stores release assets/channels, and Alfred returns an
authorized Tauri manifest plus a short-lived asset URL.

The updater has two layers, by design:

- Tauri's updater plugin runs in Alfred and verifies the signed artifact before
  installing it.
- The Alfred gateway authenticates the device entitlement and asks CrabNebula
  for the matching channel/target asset. A license check does not replace
  Tauri signature verification.

## Stage and deliver a release

1. Bump `version` in lockstep:
   - `package.json`
   - `src-tauri/Cargo.toml`
   - `src-tauri/tauri.conf.json`
2. Push/merge that commit to `main` (or whichever branch you want to build).
3. In GitHub: **Actions → release → Run workflow** → choose that branch → **Run workflow**.
   Builds never start from a push; only this manual trigger stages draft installers.
4. Wait for [`.github/workflows/release.yml`](../.github/workflows/release.yml)
   to finish. Its release gates download the exact draft DMGs and NSIS EXE,
   verify signing/notarization or the documented Windows-signing waiver, and
   exercise clean install, two launches, and uninstall on native hosted runners.
5. Open the draft GitHub Release and verify every artifact from that exact
   commit. **Do not publish the draft.**
6. Generate SHA-256 checksums, smoke-test the downloaded draft artifacts, and
   upload only the accepted files and checksums to the paid download service.
   For updater-enabled releases, upload the signed updater archives to the
   selected CrabNebula channel and publish only after the gateway smoke test.
7. Publish or link the exact Corresponding Source for the release at no extra
   charge. Tag the exact source commit used by CI and place a prominent source
   link beside the paid binary download, as required for GPL binary distribution.
8. Verify a standalone Desktop purchase and a Company member seat both receive
   the correct download/update entitlement. Verify seat removal revokes new
   hosted/update access without deleting local data, then complete a clean
   install through the customer-facing channel before announcing the release.
9. Keep the GitHub draft private as a maintainer record, or delete it after the
   storefront upload according to the retention policy. Never convert it to a
   public release.

## GitHub settings

Settings → Actions → General → Workflow permissions → **Read and write permissions**.

## Secrets (add before a public ship)

The macOS release jobs intentionally fail when these values are absent. The
Windows artifacts remain unsigned by the explicit beta decision below.

### macOS (Developer ID + notarization)

| Secret | Value |
| --- | --- |
| `APPLE_CERTIFICATE` | Base64-encoded `.p12` export of **Developer ID Application** |
| `APPLE_CERTIFICATE_PASSWORD` | Password for that `.p12` |
| `KEYCHAIN_PASSWORD` | Any strong password for the ephemeral CI keychain |
| `APPLE_ID` | Apple ID email used for notarization |
| `APPLE_PASSWORD` | App-specific password (not the account password) |
| `APPLE_TEAM_ID` | 10-character Team ID |

Export the cert locally:

```bash
base64 -i DeveloperID.p12 | pbcopy   # macOS
```

### Windows Authenticode

Use a current certificate-provider flow supported by Tauri: a managed signing
service/custom sign command, or a certificate imported into the Windows runner.
For a certificate-store flow, configure `certificateThumbprint`, SHA-256 digest,
and the provider's timestamp URL, then add the matching CI secrets.

For the 2026-08-13 release decision, Windows signing is explicitly waived due
to budget. Every NSIS/MSI artifact must be labeled **unsigned beta** wherever it
is offered, with an unknown-publisher/SmartScreen warning. Do not describe this
as a warning-free Windows release.

### Tauri updater signatures (for DMG/EXE in-app updates)

Generate once and store securely — losing the private key permanently breaks
updates for already-installed apps:

```bash
bunx tauri signer generate -w ~/.tauri/alfred.key
```

| Secret | Value |
| --- | --- |
| `TAURI_SIGNING_PRIVATE_KEY` | Contents (or path content) of the private key |
| `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` | Key password if you set one |

Embed the **public** key in `tauri.conf.json` when implementing the updater
plugin (`plans/006-in-app-updater-dmg-exe.md`). The update manifest and download
URLs must enforce the same paid-distribution boundary as the storefront. The
production endpoint is the authenticated Alfred gateway backed by CrabNebula,
not a public GitHub `latest.json`. Until Plans 022 and 006 are complete, keep
`uploadUpdaterJson: false` in the release workflow and require manual customer
downloads.

The desktop sends a short-lived entitlement/update token in the updater request
headers. The gateway returns `204` for no update, `200` with Tauri's dynamic
manifest for an entitled device, and `401/403` for a missing or revoked
entitlement. Stripe and CrabNebula API credentials never ship in Alfred.

## Local smoke build

```bash
bun install
bun run build
```

Produces installers for the **current** OS only under
`src-tauri/target/release/bundle/`.
