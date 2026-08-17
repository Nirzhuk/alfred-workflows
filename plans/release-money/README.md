# Alfred Polar paid-release execution path

Reconciled on 2026-08-15 at commit `ecb94d6`. Execute these plans in numeric
order. This is the canonical roadmap for selling Alfred's official desktop
distribution through Polar without operating an Alfred billing, account,
license, webhook, database, email, or download backend.

Read each plan completely before starting it. Run every verification gate,
honor its STOP conditions, and update the status table after completion.

Status values: `TODO`, `IN PROGRESS`, `DONE`, `BLOCKED (reason)`, or
`REJECTED (reason)`.

## Fixed product contract

- Public GPL-3.0-or-later source and self-built Alfred remain independently
  usable. Payment buys the official signed builds, Polar-hosted downloads,
  support, and future hosted services; it does not disable local workflows or
  customer data.
- **Desktop License** is sold to one named user as annual or lifetime.
- **Company** is sold monthly or annually by purchased seat quantity. Every
  claimed Company seat receives its own Desktop license-key and download
  benefits through Polar; revoking an assignment frees it but does not lower
  the bill until the buyer also reduces purchased quantity.
- Polar is the merchant of record and owns checkout, payment collection,
  tax/VAT handling, receipts, subscription lifecycle, customer email
  authentication, billing self-service, seat invitations, benefit grants,
  license-key issuance, and customer download authorization.
- Alfred calls Polar's public customer-portal license-key endpoints directly.
  No Polar access token or webhook secret ships in the app.
- A license is refreshed after 7 days when possible and may use its last
  successful validation for at most 30 days during transient network failure.
  A confirmed `revoked` or `disabled` response takes effect immediately.
- The complete license key and activation ID are kept in the OS credential
  store. SQLite and React receive only masked, non-secret state.
- Polar's hosted portal owns device deactivation, billing, receipts, downloads,
  Company members, and seats. Alfred does not duplicate those systems.
- v0.5.0 uses Polar-hosted manual downloads. A paid automatic Tauri updater is
  deferred because it would require an authenticated manifest/asset service;
  `uploadUpdaterJson` remains `false`.
- Backendless lifetime purchases include future Polar-hosted Alfred releases.
  If that promise is unacceptable, remove the lifetime offer before launch;
  do not advertise an unenforceable limited update window.

## Execution order and status

| Plan | Outcome | Priority | Effort | Depends on | Status |
| --- | --- | --- | --- | --- | --- |
| [001](archive/001-connect-desktop-polar-licensing.md) | Build the direct Polar license client with injected fixtures, secure storage, and a 30-day offline window | P0 | L | — | DONE (archived) |
| [002](archive/002-build-polar-license-settings.md) | Build License & Billing settings with injected checkout/portal configuration | P0 | M | 001 | DONE (archived) |
| [003](003-configure-polar-commerce.md) | Configure Polar sandbox, bind public IDs/URLs, and prove the real integration | P0 | M | 001, 002 | BLOCKED — Step 1 verifier done; needs approved commercial policy and authenticated Polar sandbox access |
| [004](004-publish-signed-polar-downloads.md) | Stage, verify, document, and deliver signed installers through Polar | P0 | M | 003; signing reference | BLOCKED — Plan 003 is not DONE |
| [005](005-run-polar-paid-release-acceptance.md) | Pass the packaged Polar sandbox acceptance matrix | P0 | L | 004 | TODO |
| [006](006-launch-polar-paid-release.md) | Configure production Polar, run live canaries, and open sales | P0 | M | 005 | TODO |

## Why this order

1. Plan 001 is fully executable without external credentials. It builds the
   client, secure storage, safe state machine, and mock contract tests behind
   injectable public configuration.
2. Plan 002 is also fully local. It builds the UI and allow-listed navigation
   with fixture links and honest missing-configuration states.
3. Plan 003 is the first operator-assisted gate. It configures Polar only after
   the code is ready, then binds public IDs/URLs and proves the real sandbox
   integration. Missing dashboard access cannot invalidate Plans 001–002.
4. Plan 004 keeps the verified GitHub draft build/signing pipeline, then moves
   accepted installers and checksums into Polar's File Downloads benefit.
5. Plan 005 proves annual, lifetime, Company, offline, revocation, portal, and
   packaged-platform behavior in the sandbox.
6. Plan 006 repeats the approved configuration in production, runs a small
   live-money canary, then opens sales gradually.

## Execution readiness

- Plans 001–002 are fully executable in a normal repository worktree with no
  Polar account, production credential, price choice, purchase, or second OS.
- Plan 003 begins with executable verifier/configuration code, then pauses only
  for unavoidable Polar sandbox login and commercial-policy approval. Resume
  the same plan after the operator supplies those inputs.
- Plan 004 requires access to the existing GitHub release workflow and Polar
  sandbox downloads; Plan 005 additionally requires the named clean platform
  environments; Plan 006 is the only live-money plan.
- Missing external authorization is not a reason to block or weaken earlier
  plans. Complete all local steps, record the exact pending operator action,
  and resume without redesigning the architecture.

## Explicitly removed from the release path

- The custom Bun/Stripe license server and its SQLite database.
- Alfred-hosted accounts, magic links, email, recovery, organization roles,
  Company portal, webhook inbox, checkout claims, and Billing Portal proxy.
- A VPS, Caddy, server backups/restores, SMTP, private asset proxy, and server
  monitoring for commerce.
- Stripe Price IDs, Stripe SDKs, Stripe CLI smoke tests, and Stripe webhook
  configuration.
- Authenticated automatic updates for v0.5.0.

The existing `api-licenses/` implementation is abandoned reference work. Do
not deploy, extend, or integrate it. Removing it from disk or history is a
separate cleanup action and is not authorized by these plans.

## Reference and deferred work

- [Verified installer-signing baseline](reference-verified-installer-signing.md)
  records completed macOS signing/notarization and the accepted unsigned
  Windows beta exception.
- [Deferred Homebrew distribution](deferred-homebrew-distribution.md) remains
  outside the paid-release path because a public cask bypasses Polar downloads.
- A future automatic updater requires a separate product decision: either make
  signed updater assets public or approve a small authenticated update service.
  Do not quietly reintroduce a general commerce backend.

## Legacy plan migration

| Previous plan | Canonical destination |
| --- | --- |
| `001-polar-offline-licensing.md` | Plans 001–003 |
| `002-freemium-entitlement-enforcement.md` | Plan 001; local feature gating remains rejected |
| `003-freemium-license-ux.md` | Plan 002 |
| `004-release-signing-secrets.md` | Signing reference + Plan 004 |
| `005-homebrew-cask.md` | Deferred Homebrew note |
| `006-in-app-updater-dmg-exe.md` | Plan 004; automatic updater deferred |
| `022-commercial-entitlement-update-gateway.md` | Rejected; replaced by Plans 001, 003, and 004 |
