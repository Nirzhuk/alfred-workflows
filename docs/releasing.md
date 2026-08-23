# Releasing Alfred

Before cutting a release, complete the [v0.5.0 release TODO](release-todo.md).
This document is the operator runbook; the TODO is the launch gate and artifact
acceptance checklist. The canonical plan set is
[`plans/release-money/`](../plans/release-money/README.md).

Alfred's source is public under GPL-3.0-or-later, but official maintainer-built
installers are a paid product. Release automation therefore stages binaries in a
**private draft** GitHub Release for maintainers only. Never publish that draft.
After verification, upload the accepted artifacts to Polar's File Downloads
benefit, which is the only customer-facing channel.

## Distribution boundary

[Polar](https://polar.sh) is the merchant of record. Polar owns checkout,
payment collection, tax/VAT, receipts, customer email authentication, billing
self-service, seat invitations, license-key issuance, and download
authorization. Alfred operates **no** payment gateway, account
service, license server, webhook receiver, email service, server database, or
server backup. No Polar access token or webhook secret ships in the app.

| Channel | Artifact | How users get a newer version |
| --- | --- | --- |
| Alfred License (one named user, one-time) | Official installers hosted by Polar | Sign in to Polar's customer portal by email and download manually |
| Alfred Teams (one-time per claimed seat) | Same installers; every claimed seat gets its own license key and downloads | Same manual Polar portal download |
| Public source repository | Source only | Pull/clone a newer version and rebuild |
| Private draft GitHub Release | Maintainer staging | Never user-facing; do not publish |

Prices live in Polar's dashboard, not in this repository and not in the desktop
binary. Do not restate them here — they drift. Both products are one-time
purchases, so there is no billing interval to state.

Each purchase includes **one year of updates**, enforced client-side by
comparing the build's baked `ALFRED_RELEASE_DATE` against the license key's
deadline. Polar's File Downloads benefit is perpetual, so a customer whose year
has lapsed **can still download newer builds** — those builds run with all
local data intact and only their paid features locked. That is expected, and
the app explains it on first run rather than letting the customer discover it.

`ALFRED_RELEASE_DATE` is release-critical. The release workflow supplies it as
an ISO `YYYY-MM-DD` value; an unset value means a source build and never locks
anything. A wrong value silently grants or denies entitlement to real customers
and fails no test, so the acceptance manifest asserts it on every release.

Under Alfred's GPL-3.0-or-later license, these products control the official
signed distribution, hosted organization features, and support. They do not
remove the GPL right to build or use the source commercially, and payment never
disables local workflows or customer data. Do not publish commercial-use
restriction copy unless a separate dual-license/EULA and contributor-rights
review is complete.

A public Homebrew cask or public GitHub Release asset would expose the paid
official binary without purchase, so neither is part of this model. See
[deferred Homebrew distribution](../plans/release-money/deferred-homebrew-distribution.md).

## No automatic updater in v0.5.0

v0.5.0 ships **manual** downloads. There is no Tauri updater plugin, no updater
dependency, no updater public key, and no update manifest. The release workflow
keeps `uploadUpdaterJson: false`.

In the app, **Help → Download Latest Version…** (and the same tray/menu-bar
item) opens Polar's customer portal in the system browser through the
allow-listed `src/features/licensing/public-links.ts` seam. Alfred never
resolves or fetches a signed installer URL itself. A build with no configured
portal URL shows source build instructions instead of a broken link.

An automatic updater is deferred because it needs either public signed updater
assets or an authenticated manifest/asset service. That is a separate product
decision; do not quietly reintroduce a commerce or asset backend for it.

## Public Polar URLs the build needs

The frontend reads these at build time. They are public, non-secret links.
Until the operator fills them in, every `TODO(polar-url)` marker in
`README.md`, `docs/install.md`, and `docs/release-todo.md` stays unresolved and
the in-app action falls back to source build instructions.

The accepted form depends on `ALFRED_POLAR_ENVIRONMENT`, which the same `.env`
supplies to `build.rs` and to Vite.

| Variable | Destination | Accepted form (`production`) | Accepted form (`sandbox`) |
| --- | --- | --- | --- |
| `VITE_POLAR_DESKTOP_CHECKOUT_URL` | Desktop checkout link | `https://buy.polar.sh/polar_cl_…` | `https://sandbox-api.polar.sh/v1/checkout-links/polar_cl_…/redirect` |
| `VITE_POLAR_CUSTOMER_PORTAL_URL` | Customer portal | `https://polar.sh/<org-slug>/portal` | `https://sandbox.polar.sh/<org-slug>/portal` |

The per-environment allow-list in
`src/features/licensing/public-link-rules.ts` rejects any other host, path,
scheme, port, query string, or fragment — and rejects the *other* environment's
shapes, so a sandbox link can never ship to a paying customer.
`src-tauri/capabilities/default.json` independently restricts which URLs the app
may open. The sandbox values are recorded in
[`scripts/polar/sandbox-manifest.json`](../scripts/polar/sandbox-manifest.json)
and validated by `bun run verify:polar-sandbox`.

## Stage and deliver a release

1. Bump `version` in lockstep:
   - `package.json`
   - `src-tauri/Cargo.toml`
   - `src-tauri/tauri.conf.json`

   The release workflow's `verify-version` job fails the run if these three
   disagree, so fix a mismatch before dispatching.
2. Push/merge that commit to `main` (or whichever branch you want to build) and
   record the exact source commit — the acceptance manifest pins it.
3. In GitHub: **Actions → release → Run workflow** → choose that branch → **Run
   workflow**. Builds never start from a push; only this manual trigger stages
   draft installers.
4. Wait for [`.github/workflows/release.yml`](../.github/workflows/release.yml)
   to finish. Its gates download the exact draft DMGs and NSIS EXE, verify
   signing/notarization or the documented Windows-signing waiver, and exercise
   clean install, two launches, and uninstall on native hosted runners.
5. Download the run's `acceptance-manifest` artifact. It contains
   `acceptance-manifest.json` and `acceptance-manifest.txt` with the version,
   source commit, and every required artifact's filename, size, architecture,
   signing status, and SHA-256 checksum. It contains no signing secret. The job
   fails if a required artifact is missing or appears more than once.

   Required paid artifacts for v0.5.0:
   - `Alfred_<VERSION>_aarch64.dmg` — signed, notarized, stapled Apple Silicon
   - `Alfred_<VERSION>_x64.dmg` — signed, notarized, stapled Intel
   - `Alfred_<VERSION>_x64-setup.exe` — **unsigned beta** Windows x64 NSIS

   Linux `.AppImage`/`.deb`/`.rpm` and the Windows `.msi` stay best-effort. Do
   not promote them to supported paid downloads without a separate operator
   decision recorded in the release TODO.
6. Open the draft GitHub Release and confirm every artifact came from that exact
   commit. **Do not publish the draft.**
7. Smoke-test the downloaded draft artifacts (not local build output) on clean
   machines, then upload only the accepted files, the checksum manifest, the
   release notes, the GPL notices, and a prominent corresponding-source link to
   Polar's shared File Downloads benefit.

   Add the new files **before** disabling the previous release's files. Never
   delete an old file until the replacement and a rollback path are verified.
   Confirm the uploaded bytes hash to the checksums in the acceptance manifest.
8. Publish or link the exact Corresponding Source for the release at no extra
   charge. Tag the exact source commit used by CI and place a prominent source
   link beside the paid binary download, as required for GPL binary
   distribution.
9. Verify both benefit classes download the exact accepted files: an Alfred
   License purchase and a claimed Alfred Teams seat. Verify an unrelated or
   unclaimed customer cannot. Confirm seat removal revokes new download access
   without deleting local data. A customer whose update year has lapsed **can**
   still download — that is expected, not a defect.
10. Enable the new files and links, then retain or disable the old files
    according to the rollback policy.
11. Keep the GitHub draft private as a maintainer record, or delete it after the
    Polar upload according to the retention policy. Never convert it to a public
    release.

### Rollback

Disable the new Polar files and checkout links and re-enable the previous
release's files. Do not delete customer purchase history, license keys, or any
local Alfred data. Because updates are manual, no already-installed copy is
affected by a rollback.

## GitHub settings

Settings → Actions → General → Workflow permissions → **Read and write
permissions**.

## Secrets

The macOS release jobs intentionally fail when these values are absent. The
Windows artifacts remain unsigned by the explicit beta decision below. No Polar
credential belongs in CI: the v0.5.0 upload to Polar is done manually through
the dashboard.

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

### Windows Authenticode — waived

For the 2026-08-13 release decision, Windows signing is explicitly waived due to
budget. Every NSIS/MSI artifact must be labeled **unsigned beta** wherever it is
offered, with an unknown-publisher/SmartScreen warning. Never describe this as a
signed or warning-free Windows release. See
[the signing reference](../plans/release-money/reference-verified-installer-signing.md).

Signing it later needs a current certificate-provider flow supported by Tauri: a
managed signing service/custom sign command, or a certificate imported into the
Windows runner. For a certificate-store flow, configure `certificateThumbprint`,
SHA-256 digest, and the provider's timestamp URL, then add the matching CI
secrets and remove the unsigned-beta labeling from every customer-facing surface
in the same change.

## Legacy: rejected commercial gateway

An earlier design routed downloads and automatic updates through an
authenticated Alfred gateway backed by a third-party asset host, with a
server-side payment catalog. **That design is rejected and is not implemented.**
It appears here only so an old reference is not mistaken for current
architecture. The replacement is Polar as merchant of record plus manual
downloads, described above and in
[`plans/release-money/README.md`](../plans/release-money/README.md).

## Local smoke build

```bash
bun install
bun run build
```

Produces installers for the **current** OS only under
`src-tauri/target/release/bundle/`.
