# Reference: verified installer-signing baseline

This is completed evidence, not an executable plan.

## Verified on 2026-08-13

- Frozen source: `c5fc81fb0ddb658001fb058e0c33a03c646ffb6a`.
- GitHub Actions run `31695713076` passed all four build jobs and all three
  clean-runner verification jobs.
- Apple Silicon and Intel `.app` bundles and DMGs were Developer ID signed,
  notarized, stapled, downloaded, installed, launched twice, and removed on
  clean hosted runners.
- The Windows NSIS installer was verified as unsigned, then installed,
  launched twice, and removed on a clean runner.
- The private GitHub draft contained the expected macOS, Windows, and Linux
  assets with GitHub-recorded SHA-256 digests.

## Accepted exception

Windows Authenticode signing is waived for the initial release because there
is no signing budget. Every download and release note must label Windows as an
unsigned beta and explain the unknown-publisher/SmartScreen warning. Do not
describe the Windows build as signed or warning-free.

## What this does not prove

- No accepted artifact has been uploaded to Polar's sandbox or production
  File Downloads benefit.
- No Desktop annual/lifetime or claimed Company member has completed a Polar
  download from the accepted files.
- No direct Polar activation, offline-window, revocation, or device-limit test
  has passed in a packaged build.
- Customer-facing checksums and corresponding-source links are not yet published.

Those remaining outcomes belong to
[`004-publish-signed-polar-downloads.md`](004-publish-signed-polar-downloads.md)
and the acceptance/launch plans that follow it. Automatic Tauri updater
artifacts are deliberately deferred for the backendless v0.5.0 release.
