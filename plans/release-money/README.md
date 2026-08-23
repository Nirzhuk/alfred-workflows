# Alfred Polar paid-release execution path

Reconciled on 2026-08-15 at commit `ecb94d6`; the product contract and the
execution order were rewritten on 2026-08-20 for the two-product perpetual
model. **Execution order is no longer numeric** — see "Execution order and
status" below. This is the canonical roadmap for selling Alfred's official desktop
distribution through Polar without operating an Alfred billing, account,
license, webhook, database, email, or download backend.

Read each plan completely before starting it. Run every verification gate,
honor its STOP conditions, and update the status table after completion.

Status values: `TODO`, `IN PROGRESS`, `DONE`, `BLOCKED (reason)`, or
`REJECTED (reason)`.

## Fixed product contract

This is the current contract. It replaced the four-product
annual/lifetime/seat model on 2026-08-20 via
[Plan 007](007-two-product-perpetual-model.md); the retired model is recorded
under "Historical" at the end of this section.

### Two products, both one-time

| Product | Sold as | Benefit class | Entitlement |
| --- | --- | --- | --- |
| **Alfred License** | one-time, one named user, not seat-based | `individual` | pro features forever + 1 year of updates |
| **Alfred Teams** | one-time **per claimed seat**, seat-based | `teams` | same, per claimed seat |

- Paying once unlocks every pro feature **permanently**. Nothing a customer
  paid for is ever taken away.
- The purchase includes updates released within **one year** of purchase.
- After that year the app keeps working exactly as it did. The customer simply
  stops being entitled to *newer builds'* pro features.
- `expired` means **entitled, update window closed** — it still proves a
  completed purchase. `revoked` and `disabled` mean **not entitled** and end
  access immediately. Never merge these three states.
- **Two benefits, not three.** The build reads exactly
  `ALFRED_POLAR_INDIVIDUAL_BENEFIT_ID`, `ALFRED_POLAR_TEAMS_BENEFIT_ID`, and
  `ALFRED_RELEASE_DATE`.
- `ALFRED_RELEASE_DATE` is supplied by the release workflow as ISO
  `YYYY-MM-DD`. **Unset means a source build and must never lock anything.**
- **One in-app checkout link** (Alfred License) plus the customer portal.
  **Alfred Teams is sold on the marketing website**; the app has no Teams
  checkout entry point.
- **A renewal is a fresh purchase of the same product.** There is no third
  product, so the two-benefit manifest stays valid.
  *(Coordinator default, pending owner confirmation — a separate renewal or
  upgrade SKU would break the two-benefit manifest and requires reopening
  Plan 007 Step 4.)*

### What payment buys, and what it does not

- Public GPL-3.0-or-later source and self-built Alfred remain independently
  usable. **Building Alfred from source is free and fully featured, forever.**
  Payment buys the official signed builds, Polar-hosted downloads, one year of
  updates, and support — explicitly **not capability**. See
  [Plan 008](008-pro-entitlement-and-source-freedom.md).
- A lapsed update window does **not** disable the build the customer has, does
  **not** remove a feature they already paid for, and does **not** touch
  workflows, memories, schedules, triggers, or any local data.
- Polar is the merchant of record and owns checkout, payment collection,
  tax/VAT handling, receipts, customer email authentication, billing
  self-service, seat invitations, benefit grants, license-key issuance, and
  customer download authorization.
- Alfred calls Polar's public customer-portal license-key endpoints directly.
  No Polar access token or webhook secret ships in the app.
- A license is refreshed after 7 days when possible and may use its last
  successful validation for at most 30 days during transient network failure.
  A confirmed `revoked` or `disabled` response takes effect immediately.
- The complete license key and activation ID are kept in the OS credential
  store. SQLite and React receive only masked, non-secret state.
- Polar's hosted portal owns device deactivation, billing, receipts, downloads,
  Teams members, and seats. Alfred does not duplicate those systems.
- v0.5.0 uses Polar-hosted manual downloads. A paid automatic Tauri updater is
  deferred because it would require an authenticated manifest/asset service;
  `uploadUpdaterJson` remains `false`.
- The one-year window is **enforceable client-side**: a build is in window when
  `ALFRED_RELEASE_DATE <= licenseUpdateDeadline`. The old warning against
  advertising an unenforceable window is answered rather than ignored — the
  mechanism now exists.
- Polar's File Downloads benefit stays **perpetual**, so an out-of-window
  customer can still download a newer build. It simply runs without pro
  features. **That must be stated to the customer, not discovered by them**
  (Plan 004 Step 2).

### Historical (retired 2026-08-20)

- Desktop License was sold to one named user as annual or lifetime.
- Company was sold monthly or annually by purchased seat quantity. Every
  claimed Company seat received its own license-key and download benefits;
  revoking an assignment freed it but did not lower the bill until the buyer
  also reduced purchased quantity.
- These names, the four-product table, the three benefit classes, and the
  subscription lifecycle no longer exist anywhere in the current model.

## Execution order and status

| Plan | Outcome | Priority | Effort | Depends on | Status |
| --- | --- | --- | --- | --- | --- |
| [001](archive/001-connect-desktop-polar-licensing.md) | Build the direct Polar license client with injected fixtures, secure storage, and a 30-day offline window | P0 | L | — | DONE (archived) |
| [002](archive/002-build-polar-license-settings.md) | Build License & Billing settings with injected checkout/portal configuration | P0 | M | 001 | DONE (archived) |
| [003](003-configure-polar-commerce.md) | Configure Polar sandbox for the two products, bind public IDs/URLs, and prove the real integration | P0 | M | 001, 002, **007** | BLOCKED — rewritten 2026-08-20 for the two-product model. Step 1's verifier/manifest shape is re-opened by 007 Step 4 (still three benefit classes; `verifier.ts` still asserts a key with no expiry). Steps 2–8 need the two approved prices, authenticated Polar sandbox access, and proof that Polar can issue a one-time product's key with a one-year expiry and sell a seat product one-time per seat |
| [004](004-publish-signed-polar-downloads.md) | Stage, verify, document, and deliver signed installers through Polar | P0 | M | 003; signing reference | IN PROGRESS — rewritten 2026-08-20; the pipeline half (version alignment, artifacts, checksums, updater guard, runbook) was already correct and is unchanged. Steps 1–2 re-open for the new copy and the out-of-window explanation; Step 3 gains `ALFRED_RELEASE_DATE` in the acceptance manifest; Steps 4–6 need signed artifacts and Polar sandbox downloads |
| [005](005-run-polar-paid-release-acceptance.md) | Pass the packaged Polar sandbox acceptance matrix | P0 | L | 004 | IN PROGRESS — rewritten 2026-08-20. Matrix D's offline boundaries stay automated (15 injected-clock tests, exact) with the `expired` row corrected; nine update-window rows (W1–W9) are new; matrix E inherits the signed-macOS and packaged-Windows credential-store smokes from [plans/008](../008-connected-apps-foundation.md). [Acceptance template](../../docs/release-acceptance/TEMPLATE-polar.md) updated. Matrices A–C and E–F need Polar sandbox, sandbox purchases, and clean macOS/Windows machines |
| [006](006-launch-polar-paid-release.md) | Configure production Polar, run live canaries, and open sales | P0 | M | 005 | TODO |
| [007](007-two-product-perpetual-model.md) | Move to two one-time products with permanent features and a one-year update window | P0 | L | 001, 002 | TODO — **blocks 003–006**; needs the two approved prices and the Polar one-time/expiring-key/one-time-per-seat confirmation. Plans 003/004/005 and the acceptance template are already rewritten to its model, so the remaining work is code plus operator input, not planning |
| [008](008-pro-entitlement-and-source-freedom.md) | Gate pro features in distribution builds only; keep self-built Alfred free and equal | P0 | L | 007 | TODO — **not release-blocking**; blocked on the approved pro capability list. See "008 does not block the launch" below |

**Sequencing changed on 2026-08-20, and again later the same day.**

Order is **007 → 003 → 004 → 005 → 006**, with **008 running in parallel and
landing whenever it is ready**.

007 comes first and is not negotiable: configuring Polar products (003),
writing customer-facing download copy (004), and running the acceptance matrix
(005) all encode the product model, so doing them against the old four-product
shape would mean redoing them.

### 008 does not block the launch

008 (pro-feature gating) was previously placed between 007 and 003. It has been
moved off the critical path.

The reason is the contract itself: payment buys **signed builds, Polar-hosted
downloads, support, and a year of updates** — explicitly *"not capability"*.
Everything sold is therefore deliverable with **zero gated features**. A launch
in which every feature is free in every build is a valid launch of exactly the
product described above; it just means the update window currently gates
nothing.

Consequences:

- 003, 004, 005, and 006 may proceed to completion without 008.
- 008 still depends on 007, because it consumes the entitlement resolver and
  the `expired` semantics 007 defines.
- 008 remains P0 as *product* work and is still blocked on the approved pro
  capability list. It is simply no longer a gate on taking money.
- Plan 005's update-window rows W1–W9 stay meaningful without 008: they prove
  the resolver's decision, and the "locked" outcome is verified against
  whatever capability set exists — including an empty one.

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
5. Plan 005 proves the Alfred License and Alfred Teams journeys, activation,
   offline behavior, the update window, revocation, portal, packaged-platform
   behavior, and packaged credential storage in the sandbox.
6. Plan 006 repeats the approved configuration in production, runs a small
   live-money canary, then opens sales gradually.
7. Plan 008 gates pro features in distribution builds only. It runs alongside
   003–006 rather than ahead of them; see "008 does not block the launch".

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
| `002-freemium-entitlement-enforcement.md` | Plan 001. **The "local feature gating is rejected" decision is superseded on 2026-08-20 by [Plan 008](008-pro-entitlement-and-source-freedom.md)**, which gates pro features in distribution builds only. It is an honest switch, never enforcement: under GPL a source build is unlocked by design, and anti-tamper techniques are explicitly prohibited. |
| `003-freemium-license-ux.md` | Plan 002 |
| `004-release-signing-secrets.md` | Signing reference + Plan 004 |
| `005-homebrew-cask.md` | Deferred Homebrew note |
| `006-in-app-updater-dmg-exe.md` | Plan 004; automatic updater deferred |
| `022-commercial-entitlement-update-gateway.md` | Rejected; replaced by Plans 001, 003, and 004 |
