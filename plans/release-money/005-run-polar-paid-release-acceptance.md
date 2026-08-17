# Plan 005: Pass the Polar paid-release acceptance matrix

> **Executor instructions**: Complete Plan 004 and test the exact packaged
> release candidate against Polar sandbox. Do not waive failed commerce,
> licensing, download, signing, or platform scenarios. Record sanitized
> evidence for every row, stop on any STOP condition, and update the
> release-money index.
>
> **Drift check (run first)**: compare the desktop commit/version, Polar
> sandbox product/benefit/link configuration, uploaded artifact checksums, and
> this matrix to Plans 001–004 completion evidence. Re-run an affected
> prerequisite if any item changed.

## Status

- **Priority**: P0
- **Effort**: L
- **Risk**: HIGH (final pre-live customer validation)
- **Depends on**: `004-publish-signed-polar-downloads.md`
- **Category**: tests, security, release
- **Planned at**: commit `ecb94d6`, 2026-08-15

## Why this matters

Backendless reduces operations, but it moves trust into Polar configuration
and packaged-client behavior. Acceptance must prove the hosted checkout,
portal, benefits, keys, seats, downloads, local secure storage, offline
window, and official artifacts as one customer journey before real money is
enabled.

## Expected release-candidate system

- four Polar sandbox products;
- three license-key benefits with a three-device activation limit;
- one shared File Downloads benefit;
- Desktop and Company checkout links using Polar's confirmation page;
- hosted customer portal for email authentication, billing, keys, devices,
  receipts, downloads, members, and seats;
- direct desktop activate/validate/deactivate with keychain-only credentials;
- deterministic 7-day refresh / 30-day offline behavior;
- signed/notarized macOS DMGs and explicit unsigned-beta Windows NSIS;
- manual updates through Polar; no Alfred commercial backend or automatic updater.

If any item is false, return to its prerequisite instead of adapting the
matrix around the gap.

## Evidence format

Create a dated acceptance report at `docs/release-acceptance/<date>-polar.md`.
For each scenario record the build commit/version, platform/architecture,
Polar sandbox resource label (not a full customer payload), artifact checksum,
expected/observed result, evidence link, tester, and UTC time. Never attach
full license keys, activation IDs, emails, one-time codes, personal download
URLs, payment details, customer objects, or signing credentials.

## Required environments

- clean Apple Silicon macOS machine/runner;
- clean Intel macOS machine/runner;
- clean Windows 10 or 11 x64 VM/device;
- two private browser profiles and at least two sandbox email identities for
  buyer/member journeys;
- network controls that can block Polar without changing system time;
- automated injected-clock tests for exact day boundaries.

## Commands you will need

| Purpose | Command | Expected on success |
| --- | --- | --- |
| Public repo | `bun run check` | all desktop checks pass |
| Workflow lint | `actionlint .github/workflows/release.yml` | no errors |
| macOS artifact | `codesign --verify --deep --strict <app>` and `xcrun stapler validate <dmg>` | exit 0 |
| Artifact hash | `shasum -a 256 <download>` | equals acceptance manifest and Polar copy |
| Architecture scan | `rg -n 'Stripe|stripe|CrabNebula|license-server|authenticated updater' README.md docs src src-tauri` | no matches in active product/source documentation |
| Secret scan | `rg -n '(POLAR_ACCESS_TOKEN|polar.*secret|licenseKey|license_key)' src src-tauri/src` | no Polar secret; key handling is ephemeral/keychain-only |

## Scope

**In scope**:

- Polar sandbox purchases, subscriptions, members, seats, benefits, and files;
- the exact downloaded release-candidate packages;
- acceptance report and fixes strictly required to pass a failed scenario;
- plan/index status updates.

**Out of scope**:

- live payments, production Polar configuration, or real customer data;
- new product tiers, features, trials, or discounts;
- a custom backend, webhook, portal, or automatic updater;
- weakening acceptance to convert a failure into a pass.

## Git workflow

- Test an immutable release-candidate commit/version; record both before the
  first acceptance case.
- Put acceptance-report edits on `codex/005-polar-release-acceptance` or the
  repository's active release-candidate branch without rewriting the tested commit.
- Any source/config fix creates a new candidate and invalidates the affected
  evidence; rerun that scenario plus its regression dependencies.
- Do not publish live checkout links, production files, or a public GitHub
  binary release in this plan.

## Acceptance matrix

### A. Desktop purchase and hosted benefits

Run Desktop annual and lifetime separately:

1. checkout link shows the correct product, price, tax treatment, and terms;
2. sandbox purchase completes on Polar's hosted confirmation page;
3. receipt email reaches the buyer and its portal link authenticates by code;
4. portal shows exactly one correct key and official download benefit;
5. downloaded installers/checksums equal the accepted candidate;
6. annual cancellation keeps access through the documented paid period, then
   revokes according to the approved policy;
7. lifetime remains granted and does not invent a renewal date;
8. refund/revocation produces the approved key/download transition.

Expected: Polar alone owns payment proof and hosted benefit access; Alfred
does not consume checkout success URLs or webhooks.

### B. Company monthly/annual seats

Run Company monthly and annual with at least three seats:

1. checkout quantity and total are correct;
2. the buyer becomes owner, then explicitly assigns and claims one seat for
   themselves through Polar's hosted portal; no benefit is granted merely for
   being the billing purchaser;
3. assigning a second email sends an invitation and remains pending until claim;
4. claim grants that member an independent Company key and downloads;
5. a third available seat can be assigned and claimed;
6. revoking a member removes their benefits and frees the assignment without
   silently reducing the paid quantity;
7. reducing paid quantity cannot go below assigned seats;
8. adding/reducing seats applies Polar's displayed proration correctly;
9. canceled/past-due/refunded subscription transitions do not over-grant;
10. unclaimed and unrelated users receive no key or file access.

Expected: Polar's portal is sufficient for buyer/member/seat operations and
each claimed member has its own Desktop entitlement.

### C. Desktop activation and secure storage

For annual, lifetime, and Company keys:

1. activate on device 1 and validate;
2. quit/relaunch and load cached status without network delay;
3. activate through device 3; device 4 receives the device-limit state;
4. deactivate an old activation in Polar, then activate a replacement;
5. Refresh maps `granted`, `revoked`, `disabled`, invalid, and expired safely;
6. Deactivate this device calls Polar before clearing local credentials;
7. locked/unavailable keychain never falls back to SQLite/plaintext;
8. logs, SQLite, DOM-after-submit, URLs, and evidence contain no full key or activation ID.

Expected: the public organization/benefit IDs are the only Polar identifiers
compiled into Alfred; no access token is present.

### D. Offline and restrictive-state behavior

Use automated injected-clock tests for exact boundaries and packaged smoke for
network behavior:

- cached active before day 7;
- refresh due at day 7;
- transient timeout/DNS/429/5xx yields offline grace through day 30;
- day 30 boundary and after-day-30 `needsOnline` are exact;
- never-validated or unknown-benefit keys receive no grace;
- confirmed revoked/disabled/expired overrides grace immediately;
- network failure alone never displays revoked;
- all local workflows, memories, schedules, triggers, and data remain usable.

### E. Official distribution and update truthfulness

- required DMGs verify signing/notarization/stapling;
- Windows NSIS is labeled unsigned beta everywhere;
- Polar copies match private draft checksums byte-for-byte;
- unrelated/unclaimed accounts cannot download;
- customer-facing download pages link corresponding source and license notices;
- **Download latest version** opens Polar's portal;
- no public GitHub binary, public cask, updater JSON, updater plugin, or
  automatic-update promise exists.

### F. Product, legal, and support consistency

Check README, install/open-source/releasing docs, settings, Polar checkout,
portal/product descriptions, receipts, release notes, and support pages. They
must agree on named-user Desktop, Company per claimed seat, device limit,
offline policy, manual updates, refund/cancellation terms, lifetime scope,
Windows beta, and GPL source rights.

No surface may claim payment restricts commercial use of GPL source.

## Steps

### Step 1: Freeze the candidate and sandbox configuration

Record exact source commit/version, artifact filenames/checksums, Polar
product/benefit labels, portal settings, and checkout-link labels. Prevent
configuration drift during acceptance.

**Verify**: all identifiers/checksums appear in the report without secrets and
the three version files agree.

### Step 2: Execute matrices A–B

Run all individual and Company purchase/benefit/seat journeys. File a defect
for every mismatch and do not reuse corrupted test state.

**Verify**: every A–B row has reproducible PASS evidence.

### Step 3: Execute matrices C–D

Run packaged activation/secure-storage tests on required platforms plus exact
offline-state automation.

**Verify**: every C–D row passes; secret inspection is clean.

### Step 4: Execute matrices E–F and full regression

Verify artifacts, authorization, manual update UX, GPL/source links, and
customer-facing consistency, then re-run the full repository checks.

**Verify**: every E–F row passes; `actionlint` and `bun run check` pass.

### Step 5: Sign the launch recommendation

List known limitations. Record GO only if no P0/P1 defect or missing required
evidence remains. A NO-GO leaves this plan `BLOCKED` with defect references.

**Verify**: the dated report and checksum are recorded and approved.

## Done criteria

- [ ] Every A–F scenario has reproducible PASS evidence.
- [ ] Desktop annual/lifetime and Company monthly/annual complete end to end.
- [ ] Company buyer/member claims, revokes, and quantity changes behave correctly.
- [ ] Three-device, secure-store, status, and deactivate behavior pass.
- [ ] Exact 7/30-day behavior and restrictive-state precedence pass.
- [ ] Required packaged artifacts and Polar copies match and are correctly labeled.
- [ ] No obsolete backend/Stripe/updater architecture remains active.
- [ ] `actionlint` and `bun run check` pass on the frozen candidate.
- [ ] No unresolved P0/P1 defect remains and GO is signed.
- [ ] The roadmap row is `DONE`.

## STOP conditions

- Polar sandbox configuration differs from intended production semantics.
- Any credential, customer PII, payment detail, or signed download URL appears in evidence/logs.
- A test requires a real payment method or production credential.
- Company benefit/seat behavior fails or the beta changes materially.
- A restrictive license state still gains offline grace.
- A paid artifact is public, mismatched, unsigned where signing is promised, or missing source.
- A supported clean platform is unavailable.
- The candidate/config changes without resetting affected evidence.

## Maintenance notes

Keep this matrix as the release regression suite. Every Polar product, benefit,
portal, or checkout-link change invalidates the affected rows even when Alfred
source did not change.
