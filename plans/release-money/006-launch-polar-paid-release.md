# Plan 006: Launch Alfred's Polar paid desktop release

> **Executor instructions**: Begin only after Plan 005 records an approved GO
> for the exact candidate and Polar sandbox configuration. This plan enables
> live money and customer access; obtain operator approval at every marked
> gate. Stop on a STOP condition and update the release-money index only after
> launch checks pass.
>
> **Drift check (run first)**: compare Plan 005's commit/version, artifact
> checksums, acceptance-report checksum, product/benefit matrix, prices,
> policies, and public URLs with the proposed production configuration. Any
> change requires re-running affected acceptance rows before launch.

## Status

- **Priority**: P0
- **Effort**: M
- **Risk**: HIGH (live money and irreversible customer distribution)
- **Depends on**: `005-run-polar-paid-release-acceptance.md`
- **Category**: release, direction, security
- **Planned at**: commit `ecb94d6`, 2026-08-15

## Why this matters

Launch is a controlled transition from proven sandbox configuration to Polar
live products, real customers, and official downloads. There is no server
deployment, database migration, webhook, SMTP, DNS, or backup rollout in this
architecture, but prices, benefit attachments, files, source obligations,
support, account review, and rollback controls still need exact production
verification.

## Launch contract

- Public source/self-built Alfred remains available under GPL-3.0-or-later.
- Official signed/notarized macOS installers and explicit unsigned-beta
  Windows NSIS are paid downloads through Polar.
- Desktop annual/lifetime and Company monthly/annual match the accepted
  sandbox product/benefit semantics.
- Every claimed Company seat receives its own license key and downloads.
- Alfred validates through Polar's public customer-portal endpoints and ships
  no Polar access token.
- Billing, tax, receipts, email authentication, subscriptions, keys, devices,
  seats, and file authorization remain in Polar.
- v0.5.0 updates are manual through Polar's portal. No public binary release,
  Homebrew cask, updater JSON, or automatic updater is enabled.
- Commercial state never disables local workflows or deletes local data.

## Commands you will need

| Purpose | Command | Expected on success |
| --- | --- | --- |
| Desktop checks | `bun run check` | all public repo gates pass |
| Workflow lint | `actionlint .github/workflows/release.yml` | no errors |
| Release assets | compare files to Plan 005 acceptance manifest | every name/size/SHA-256 matches |
| macOS verify | `codesign --verify --deep --strict <app>` and `xcrun stapler validate <dmg>` | exit 0 |
| Architecture scan | `rg -n 'Stripe|stripe|CrabNebula|license-server|authenticated updater' README.md docs src src-tauri` | no matches in active product/source documentation |
| Secret scan | inspect repository, bundles, logs, and CI output | no Polar/signing credential or full customer key |

## Scope

**In scope**:

- Polar account review and production organization settings;
- production products, prices, benefits, checkout links, portal, and files;
- exact accepted public build configuration and customer documentation;
- controlled live purchase canaries, gradual rollout, and launch monitoring;
- source tag/corresponding-source and release records;
- this plan and release-money index status.

**Out of scope**:

- new products, trials, discounts, features, or changed accepted binaries;
- any Alfred backend, webhook, database, email service, portal, or asset proxy;
- public GitHub binary releases, Homebrew, or automatic updates;
- hiding the Windows unsigned-beta warning;
- changing GPL rights or adding incompatible terms without legal/rightsholder review.

## Git workflow

- Tag the exact accepted source commit according to repository policy.
- Do not rebuild after acceptance. If a rebuild is unavoidable, return to Plan 005.
- Production Polar configuration, file enablement, and public checkout-link
  publication each require explicit operator approval.

## Steps

### Step 1: Complete business, account-review, legal, and support readiness

Complete Polar's merchant/account review and payout setup. Approve exact
prices/currencies, tax display, refund/cancellation policy, lifetime
update/support promise, privacy policy, terms, GPL corresponding-source
delivery, support address/SLA, incident owner, and subprocessor disclosure.

Confirm current Polar plan/fees and payout countries are acceptable. Prepare
copy for supported OS/architectures, agent CLI prerequisites, three-device
limit, offline window, manual updates, Company seat rules, and Windows unsigned beta.

**Verify**: Polar permits live sales/payouts and every planned checkout/download
surface has approved policies, source link, and support destination.

### Step 2: Recreate the accepted model in Polar production

Create production resources using Plan 003's exact matrix:

- Desktop annual and lifetime;
- Company monthly and annual seat-based;
- three distinct three-activation License Keys benefits;
- one shared File Downloads benefit;
- Desktop and Company checkout links using Polar's confirmation page;
- hosted portal seat management and email authentication.

Use production IDs/URLs in the official build configuration. No product/price
ID or access token belongs in Alfred. Have a second reviewer compare every
attachment and setting against the accepted sandbox record.

**Verify**: a configuration checklist has two-person approval; no live
checkout link is public yet.

### Step 3: Upload the exact accepted release files

Upload the byte-identical Plan 005 artifacts, checksum manifest, release notes,
license notices, and corresponding-source link to the production File
Downloads benefit. Verify local re-download hashes with an internal production
customer or Polar preview mechanism before public enablement.

Do not delete old files during a future release until replacement and rollback
have been proven. Keep the GitHub staging release private.

**Verify**: every filename, size, architecture, and SHA-256 matches Plan 005;
the public repository contains source but no official paid binary.

### Step 4: Build and recheck the official Polar-configured candidate

Build from the accepted source commit with production public organization ID,
benefit IDs, checkout links, and portal URL. Confirm the bundle contains no
Polar access token, customer identifier, full key, signing private key, Stripe
configuration, or abandoned backend URL. Re-run `bun run check`, packaged
keychain smoke, link allow-list tests, and macOS signature validation.

If public configuration changes the binary checksum relative to Plan 005,
record the new exact artifacts and rerun all affected packaged acceptance rows
before proceeding.

**Verify**: the production-configured candidate has signed approval and no
unreviewed delta.

### Step 5: Run controlled live-money canaries

With explicit approval, share live checkout links only with designated internal
canary buyers and use authorized company payment methods:

1. Desktop annual purchase → receipt/portal → key → download → activate/validate;
2. Desktop lifetime purchase → key/download → activate/validate;
3. Company monthly or annual purchase with at least two seats → buyer claim →
   member invitation/claim → independent key/download/activation;
4. customer portal payment, receipt, device, cancellation, and seat operations;
5. one policy-approved refund/cancellation/revocation transition.

Confirm Polar's tax/receipt/payout records and Alfred's resulting states. Do
not manually edit benefit grants to make a failed flow pass.

**Verify**: operator/finance signs a sanitized reconciliation of each payment,
benefit class, download, and activation; no support blocker remains.

### Step 6: Publish entry points gradually and monitor

Publish checkout links in stages: internal list, invited users, then README/
install docs and public launch surfaces. At each stage monitor checkout
completion, payment failures, refunds/disputes, portal access, benefit grants,
Company claims, license validation/device-limit errors, download failures, and
support volume in Polar and Alfred's privacy-safe local reports.

Define numerical stop thresholds before public launch. Rollback disables
checkout links/files or removes public entry points without deleting customer
history, valid downloaded applications, or local data.

**Verify**: one complete monitoring window passes at each stage and the named
operator can exercise rollback.

### Step 7: Close the release and establish the operating cadence

Record production resource labels/IDs privately, source tag, artifact
checksums, acceptance report, canary reconciliation, known limitations,
support/incident contacts, and rollback steps. Schedule:

- Polar payout/payment/refund review;
- failed benefit/download/support review;
- quarterly account-recovery/access review;
- license/seat sandbox regression after Polar product changes;
- the Plan 005 matrix for every official release.

Capture privacy-minimized baseline metrics: checkout-link visits if available,
paid conversions, Desktop vs Company, annual vs lifetime/monthly mix, claimed
seats, refunds, activation errors, and download/support failures.

**Verify**: launch record and owner schedule are complete; repository checks
remain green at the launched commit.

## Test plan

- Re-run the high-risk Plan 005 subset in production: all benefit classes,
  Company claim/revoke, device limit, restrictive state, unauthorized download,
  artifact checksums, and source link.
- Use marked internal accounts only; never use customer data for destructive tests.
- Exercise checkout/file rollback and confirm existing customers retain the
  policy-promised access.

## Done criteria

- [ ] Polar account review, payouts, legal, pricing, refund, source, and support readiness are approved.
- [ ] Production products/benefits/links/portal exactly match the accepted model.
- [ ] Official artifacts and source links match the accepted candidate.
- [ ] Production-configured Alfred contains public Polar IDs/URLs only and passes checks.
- [ ] Desktop annual, lifetime, and Company live canaries reconcile end to end.
- [ ] Gradual rollout completes without a stop-threshold breach.
- [ ] No public official binary or automatic updater exists.
- [ ] Baseline commercial metrics and operating owners are recorded without PII.
- [ ] The launch record is signed and the roadmap row is `DONE`.

## STOP conditions

- Plan 005 GO does not match the exact production candidate/configuration.
- Polar account review/payout, pricing, tax, refund, privacy/terms, GPL source, or support ownership is unresolved.
- A production benefit attachment differs from the accepted matrix.
- A build contains a Polar credential, customer data, signing private key, or obsolete backend URL.
- Official files are public, checksum-mismatched, or lack corresponding source.
- Company or license-key behavior fails during a live canary.
- Windows is marketed as signed/warning-free.
- A stop threshold, security issue, billing mismatch, or data-loss report fires during rollout.

## Maintenance notes

Provider configuration is production code even when it lives in a dashboard.
Require review and acceptance evidence for every product, benefit, portal, and
checkout-link change. Do not add a backend merely to reproduce a Polar feature.
