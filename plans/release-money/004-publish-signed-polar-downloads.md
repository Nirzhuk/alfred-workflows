# Plan 004: Publish verified official installers through Polar downloads

> **Executor instructions**: Complete Plan 003, read
> [Plan 007](007-two-product-perpetual-model.md) for the product model this
> plan's copy must state, and read the signing
> reference. Keep GitHub releases as private maintainer staging drafts and use
> Polar's File Downloads benefit as the customer channel. Do not enable a
> Tauri automatic updater or live sales in this plan. Follow every gate, stop
> on a STOP condition, and update the release-money index when done.
>
> **Drift check (run first)**:
> `git diff --stat ecb94d6..HEAD -- .github/workflows/release.yml src src-tauri docs README.md plans/release-money`
> Reconcile the current build matrix, version files, signing decisions, menu
> behavior, and Polar benefit configuration before editing.

## Status

- **Priority**: P0
- **Effort**: M
- **Risk**: HIGH (official binary distribution and signing custody)
- **Depends on**: `003-configure-polar-commerce.md` and
  `reference-verified-installer-signing.md`
- **Category**: security, release infrastructure, docs
- **Planned at**: commit `ecb94d6`, 2026-08-15
- **Rewritten at**: 2026-08-20 against the two-product perpetual model
  ([007](007-two-product-perpetual-model.md)), from the drift map in
  [RECONCILIATION-003-004-005.md](RECONCILIATION-003-004-005.md). The pipeline
  half of this plan (version alignment, required artifacts, checksums, updater
  guard, runbook, rollback) was already correct and is unchanged. The changes
  are customer-facing copy, the benefit-class names in the gates, and one
  additive release-critical field: `ALFRED_RELEASE_DATE` in the acceptance
  manifest.

## Why this matters

Polar can authorize and host Alfred's installers without an Alfred asset
server. The release pipeline must still prove the exact binaries before they
are uploaded, publish checksums and corresponding source, avoid exposing paid
assets through public GitHub, and tell users honestly that v0.5.0 updates are
manual through Polar's portal.

## Current state

- `.github/workflows/release.yml` manually builds macOS ARM64/Intel, Linux, and
  Windows artifacts into a private draft GitHub Release.
- Both macOS DMGs have passed Developer ID signing, notarization, stapling, and
  clean-install smoke tests. Windows is an explicitly unsigned beta.
- The workflow correctly uses `uploadUpdaterJson: false`.
- `src-tauri/tauri.conf.json` has no updater plugin configuration.
- The app currently contains a “Check for Updates” stub that says automatic
  updates are not configured.
- Plan 003 creates one shared Polar File Downloads benefit attached to both
  products — Alfred License and Alfred Teams — and granted to each claimed
  Teams seat.
- That benefit is **perpetual**. Polar will keep serving newer files to a
  customer whose one-year update window has closed. That is expected: the
  window is enforced client-side by comparing the build's
  `ALFRED_RELEASE_DATE` against the license key's deadline. Step 2 owns saying
  so to the customer.
- Polar gives authorized customers personal signed download URLs. Those URLs
  are never compiled into Alfred or republished.

## Commands you will need

| Purpose | Command | Expected on success |
| --- | --- | --- |
| Full local check | `bun run check` | all gates pass |
| Workflow lint | `actionlint .github/workflows/release.yml` | no errors |
| Inspect draft | `gh release view v<VERSION> --json assets,isDraft,url` | draft true and required artifacts listed |
| macOS verify | `codesign --verify --deep --strict <app>` and `xcrun stapler validate <dmg>` | exit 0 |
| Hash | `shasum -a 256 <installer>` | one recorded SHA-256 per advertised artifact |
| Updater guard | `rg -n 'uploadUpdaterJson|createUpdaterArtifacts|plugins.*updater' .github src-tauri` | `uploadUpdaterJson: false`; no enabled updater config |

## Scope

**In scope**:

- `.github/workflows/release.yml` and focused release scripts;
- `src/features/workflow/components/workflow-canvas/workflow-canvas.tsx` or
  the current update-menu handler;
- `src-tauri/capabilities/default.json` only for approved Polar portal opening;
- `README.md`, `docs/install.md`, `docs/open-source.md`, `docs/releasing.md`,
  and `docs/release-todo.md`;
- Polar sandbox File Downloads content and customer-visible product copy;
- this plan and the release-money index status.

**Out of scope**:

- `api-licenses/` or any server deployment;
- Polar webhooks, access tokens in Alfred, or a custom portal;
- Tauri updater plugin, updater JSON, private manifest service, or CrabNebula;
- live Polar products/payments (Plan 006);
- public GitHub binary releases, Homebrew, or public CDN mirrors;
- Windows signing beyond the accepted beta limitation.

## Git workflow

- Branch: `codex/004-polar-download-release`.
- Use imperative commits such as `Route official downloads through Polar`.
- Do not publish a GitHub release, live Polar product, or production file in
  this plan.

## Steps

### Step 1: Replace obsolete Stripe/backend release documentation

Update customer and operator documentation so it consistently states:

- Polar is merchant of record and hosts checkout, portal, keys, seats, and downloads;
- **there are two products, both one-time purchases**: **Alfred License** (one
  named user, not seat-based) and **Alfred Teams** (one-time per claimed seat);
- **paying once unlocks every pro feature permanently** — nothing a customer
  paid for is ever taken away;
- **the purchase includes one year of updates**, counted from purchase;
- **what lapsing does**: after the year, builds released later run fine and keep
  all local data and workflows, but their pro features stay locked until the
  customer buys again;
- **what lapsing does not do**: it does not disable the build the customer
  already has, does not remove a feature they already paid for, does not touch
  workflows, memories, schedules, triggers, or any local data, and does not
  make the app stop working;
- a refunded, revoked, or disabled license **is** different from a lapsed
  update window and does end entitlement;
- **building from source is free and fully featured, forever** — payment buys
  signed builds, Polar-hosted downloads, one year of updates, and support, not
  capability (see [Plan 008](008-pro-entitlement-and-source-freedom.md));
- official builds update manually through Polar for v0.5.0;
- source/self-built Alfred remains usable under GPL;
- no external payment-gateway IDs, Alfred gateway, third-party binary-hosting
  origin, account service, webhook, email service, server database, or backend
  backup is required;
- the Windows build is an unsigned beta with expected SmartScreen warnings.

Alfred Teams is sold on the marketing website. The app has **one** in-app
checkout entry point, for Alfred License, plus the customer portal. Do not
document an in-app Teams purchase path.

Use live Polar checkout/portal destinations approved in Plan 003 without
embedding prices that can drift. Remove links to the rejected commercial
gateway plans.

**OPERATOR INPUT REQUIRED — lapse notification copy.** The exact customer-facing
wording for a lapsed update window is drafted in Step 2 below and marked
`DRAFT — needs owner approval`. Do not publish it, or any paraphrase of it, to
a customer surface until the owner approves the wording.

**Verify**:

```bash
rg -n 'Stripe|stripe|CrabNebula|license server|Alfred gateway|authenticated updater' \
  README.md docs/install.md docs/open-source.md docs/releasing.md docs/release-todo.md
```

Expected: no active architecture claim remains; any historical mention is
explicitly labeled rejected/legacy.

### Step 2: Make the in-app update action truthful

Replace “Check for Updates” automatic-update behavior with **Download latest
version** or equivalent. Open the fixed Polar customer-portal destination in
the system browser and explain that customers sign in by email to obtain their
personal downloads. Do not fetch a signed file URL inside Alfred.

Source/unconfigured builds may show the public build instructions instead.
Keep Tauri updater dependencies/configuration absent and
`uploadUpdaterJson: false`.

#### The out-of-window case must be explained, not discovered

Polar's File Downloads benefit is perpetual, so a customer whose update window
has closed **will still be handed newer files**. Downloading is not the
boundary; running a newer build is. If the app says nothing, that customer
installs a new version, finds pro features locked, and reasonably concludes
they were tricked.

So: an out-of-window build must explain itself **once**, on first run, in a
dismissible message — not lock silently, and not nag repeatedly. (007 Step 3
recommends explain-once; the resolver that decides in-window vs out-of-window
belongs to 007, and the locked-capability treatment belongs to Plan 008. This
step owns only the download action and the message text.)

**DRAFT — needs owner approval.** Proposed first-run wording for an
out-of-window build. This is customer-facing copy and must not ship until the
owner approves it:

> **This update is outside your update year.**
> Your purchase included one year of updates, and this build was released after
> that year ended. Alfred is running normally and every file, workflow,
> schedule, and memory on this machine is untouched.
> The version you bought keeps all of its pro features, permanently — you can
> go on using it for as long as you like. In *this* newer build, pro features
> stay locked until you buy another year.
> [Keep using this build] [Get the version I bought] [Buy another year]

The three actions and the sentence order are part of the draft: the
reassurance ("nothing was taken away") must come before the offer, or the
message reads as a paywall rather than an explanation.

**Verify**: frontend tests cover official/unconfigured destinations;
`bun run check` passes; no updater plugin is enabled; the out-of-window message
appears at most once per build and never blocks access to local data.

### Step 3: Make the private draft workflow produce an acceptance manifest

Keep the existing manual, private GitHub draft. Ensure CI checks exact version
alignment across `package.json`, `src-tauri/Cargo.toml`, and
`src-tauri/tauri.conf.json`, rejects missing/duplicate required artifacts, and
produces a text/JSON acceptance manifest containing version, source commit,
filenames, sizes, architectures, SHA-256 checksums, and **`ALFRED_RELEASE_DATE`
as the exact ISO `YYYY-MM-DD` value baked into these artifacts** — never
signing secrets.

`ALFRED_RELEASE_DATE` is release-critical and otherwise invisible. It is
supplied by the release workflow, never read from a local clock, and an unset
value means "source build" and must never lock anything. A wrong value silently
grants or denies entitlement to real customers and **will not fail any test**,
which is why the manifest has to assert it: the manifest is the only place a
human reviews it before customers do. CI must fail the run if the value is
absent from a distribution build or is not a valid ISO date.

Required paid artifacts for v0.5.0:

- signed/notarized/stapled Apple Silicon DMG;
- signed/notarized/stapled Intel DMG;
- explicit unsigned-beta Windows x64 NSIS installer.

Linux remains source/best-effort unless the operator separately approves it as
a supported paid download.

- **OPERATOR INPUT REQUIRED — Linux paid-download approval**: `<yes | no>`.
  Until this is answered, the required-artifact list above is the complete
  list and Linux ships as source/best-effort.

**Verify**: `actionlint` passes; a draft run contains exactly the required
artifacts plus one matching acceptance manifest and no public release.

### Step 4: Smoke-test the downloaded draft artifacts

Download from the draft—not local build output—and run the existing clean
install/launch/relaunch/uninstall gates. Manually exercise Windows Start-menu
launch, CLI detection outside a terminal, one real workflow, persistence,
tray, schedule, trigger, and upgrade behavior. Verify macOS signatures and
checksums from the downloaded bytes.

**Verify**: record platform, architecture, source commit, artifact checksum,
and pass/fail without credentials. Every advertised platform passes.

### Step 5: Upload the exact accepted files to Polar sandbox

Upload only the accepted installer files, checksum manifest, release notes,
license notices, and a prominent corresponding-source link to the shared Polar
File Downloads benefit. Start manually through the dashboard for v0.5.0; do
not add a permanent Polar access token to CI merely to automate one release.

Add new files before disabling old current-release files. Never delete an old
file until replacement and rollback access are verified. Confirm the uploaded
bytes match the draft checksums.

**Verify**: an Alfred License purchaser and a claimed Alfred Teams sandbox
member each download the exact files; unrelated/unclaimed users cannot; local
hashes match after download. A customer whose update window has closed **can**
still download — confirm that, rather than treating it as a defect.

### Step 6: Document the repeatable release runbook

Document this order:

1. freeze version/source commit;
2. build private GitHub draft;
3. verify signing, install behavior, and checksums;
4. upload accepted files and source link to Polar;
5. verify every benefit class;
6. enable the new files/links;
7. retain or disable old files according to rollback policy;
8. keep the GitHub draft private or remove it under the retention policy.

Include a rollback that disables the new Polar files/checkout links without
deleting customer purchase history or local data.

**Verify**: a second operator can follow the sandbox runbook from the document
without using a backend or receiving a Polar access token in Alfred.

## Test plan

- Workflow checks validate version alignment, required filenames,
  architectures, checksums, draft-only publication, and absence of updater
  JSON.
- Packaged smoke tests use the exact downloaded DMG/EXE artifacts.
- Frontend tests cover the manual download action and allow-listed destination.
- Polar sandbox E2E covers both benefit classes (individual, teams) and unauthorized denial.

## Done criteria

- [ ] Active docs contain only the Polar/backendless release architecture.
- [ ] Every customer-facing surface states: two one-time products, permanent
      pro features, one year of updates, what lapsing does and does not do, and
      that source builds are free and fully featured forever.
- [ ] No surface mentions annual, lifetime, or subscription tiers.
- [ ] The app opens Polar downloads instead of promising automatic updates.
- [ ] An out-of-window build explains itself once and never blocks local data.
- [ ] The lapse copy is either owner-approved or still marked `DRAFT` and unpublished.
- [ ] Private CI draft produces the exact required artifact/checksum manifest.
- [ ] The acceptance manifest asserts `ALFRED_RELEASE_DATE`, and CI fails a
      distribution build that is missing it or has a malformed date.
- [ ] Both macOS DMGs and Windows NSIS pass downloaded-artifact smoke tests.
- [ ] Polar sandbox hosts byte-identical accepted files for both paid benefit classes.
- [ ] Unauthorized and unclaimed users cannot download official files.
- [ ] Corresponding source and GPL notices are adjacent to paid downloads.
- [ ] No Polar access token, customer URL, or signing private key ships in Alfred.
- [ ] `actionlint` and `bun run check` pass.
- [ ] The roadmap row is `DONE`.

## STOP conditions

- A GitHub release or official binary becomes public.
- Polar cannot restrict file access to purchasers/claimed seat members.
- Uploaded bytes differ from the accepted checksums.
- macOS signing/notarization or required packaged smoke fails.
- Windows is marketed as signed or warning-free.
- GPL corresponding source cannot be provided beside the binary offer.
- The release requires an automatic updater or custom asset backend for v0.5.0.
- `ALFRED_RELEASE_DATE` is missing, malformed, or wrong on a distribution build.
- Lapse copy is published to a customer surface without owner approval.
- A verification gate fails twice after a scoped correction.

## Maintenance notes

Polar is the authorization boundary for files; Tauri/local license state is
not. Re-verify downloads for both the individual and Teams seat benefits after
changing Polar attachments. Re-check `ALFRED_RELEASE_DATE` on every release:
it is the only release input that can quietly change what a paying customer is
entitled to. Automate uploads later only if the API supports the same staged,
checksum-verified, rollback-safe flow.
