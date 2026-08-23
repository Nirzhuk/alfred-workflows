# Plan 005: Pass the Polar paid-release acceptance matrix

> **Executor instructions**: Complete Plan 004, read
> [Plan 007](007-two-product-perpetual-model.md) for what "licensed" means, and
> test the exact packaged release candidate against Polar sandbox. Do not waive
> failed commerce, licensing, download, signing, or platform scenarios. Record
> sanitized evidence for every row, stop on any STOP condition, and update the
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
- **Rewritten at**: 2026-08-20 against the two-product perpetual model
  ([007](007-two-product-perpetual-model.md)), from the drift map in
  [RECONCILIATION-003-004-005.md](RECONCILIATION-003-004-005.md). Matrix D's
  offline boundaries are automated and survive unchanged except one row; the
  update-window rows are new; matrix E now also owns the signed-macOS and
  packaged-Windows credential-store smoke tests moved out of
  [plans/008-connected-apps-foundation.md](../008-connected-apps-foundation.md).

## Why this matters

Backendless reduces operations, but it moves trust into Polar configuration
and packaged-client behavior. Acceptance must prove the hosted checkout,
portal, benefits, keys, seats, downloads, local secure storage, offline
window, update window, and official artifacts as one customer journey before
real money is enabled.

One row in this matrix is more dangerous than the rest. Under the two-product
model an **expired** license key means *entitled, update window closed* — it
still proves a completed purchase. Getting that backwards revokes features a
customer already paid for, which is a STOP condition in 007 and the reason
matrix D row 8 and matrix C row 5 are called out explicitly below.

## Expected release-candidate system

- **two** Polar sandbox products, both one-time: **Alfred License** and
  **Alfred Teams**;
- **two** license-key benefits (individual, teams) with a three-device
  activation limit, every key issued with a **one-year expiry**;
- one shared File Downloads benefit, perpetual, attached to both products;
- **one** in-app checkout link (Alfred License) using Polar's confirmation
  page — Teams is sold on the marketing website and has no in-app entry point;
- hosted customer portal for email authentication, billing, keys, devices,
  receipts, downloads, members, and seats;
- direct desktop activate/validate/deactivate with keychain-only credentials;
- deterministic 7-day refresh / 30-day offline behavior;
- `ALFRED_RELEASE_DATE` baked into the candidate as ISO `YYYY-MM-DD`, supplied
  by the release workflow, and asserted in the acceptance manifest;
- the client-side in-window rule
  `ALFRED_RELEASE_DATE <= licenseUpdateDeadline`;
- signed/notarized macOS DMGs and explicit unsigned-beta Windows NSIS;
- manual updates through Polar; no Alfred commercial backend or automatic updater.

If any item is false, return to its prerequisite instead of adapting the
matrix around the gap.

## Evidence format

Create a dated acceptance report at `docs/release-acceptance/<date>-polar.md`
from [`TEMPLATE-polar.md`](../../docs/release-acceptance/TEMPLATE-polar.md).
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
- automated injected-clock tests for exact day boundaries;
- automated injected-date tests for the update-window rule — the release date
  is compile-time, so window cases are proven by injection, never by editing a
  binary or moving the system clock.

There is deliberately **no Linux environment in this plan**. The packaged-Linux
Secret Service smoke test stays in
[plans/008-connected-apps-foundation.md](../008-connected-apps-foundation.md)
for exactly that reason — see "Credential-store smoke ownership" below.

## Commands you will need

| Purpose | Command | Expected on success |
| --- | --- | --- |
| Public repo | `bun run check` | all desktop checks pass |
| Workflow lint | `actionlint .github/workflows/release.yml` | no errors |
| Release hygiene | `bun run verify:release-hygiene` | all-PASS |
| Rust acceptance | `bun run test:rust` | injected-clock and injected-date cases pass |
| macOS artifact | `codesign --verify --deep --strict <app>` and `xcrun stapler validate <dmg>` | exit 0 |
| Artifact hash | `shasum -a 256 <download>` | equals acceptance manifest and Polar copy |
| Tier scan | `rg -ni 'annual\|lifetime\|subscription' README.md docs src src-tauri` | every hit is either a third-party service's own subscription (Claude, Cursor, OpenAI) or an explicitly labeled historical note. **No hit offers an Alfred annual, lifetime, or subscription tier.** This is a review list, not a zero-match gate |
| Secret scan | `rg -n '(POLAR_ACCESS_TOKEN\|polar.*secret\|licenseKey\|license_key)' src src-tauri/src` | no Polar secret; key handling is ephemeral/keychain-only |

## Scope

**In scope**:

- Polar sandbox purchases, members, seats, benefits, and files;
- the exact downloaded release-candidate packages;
- the signed-macOS and packaged-Windows credential-store smoke tests inherited
  from Plan 008;
- acceptance report and fixes strictly required to pass a failed scenario;
- plan/index status updates.

**Out of scope**:

- live payments, production Polar configuration, or real customer data;
- new product tiers, features, trials, or discounts;
- the packaged-Linux Secret Service smoke test (stays in Plan 008);
- which features are "pro" — that list belongs to
  [Plan 008 (pro entitlement)](008-pro-entitlement-and-source-freedom.md);
- a custom backend, webhook, portal, or automatic updater;
- weakening acceptance to convert a failure into a pass.

## Credential-store smoke ownership

`plans/008-connected-apps-foundation.md` left an open question: three packaged
credential-store smoke tests could not run from a development checkout, and two
of them needed exactly the environments this plan already mandates.

**Decision (coordinator, 2026-08-20)**:

| Smoke test | Owner | Why |
| --- | --- | --- |
| Signed/notarized macOS package: create/read/overwrite/delete credential + restart persistence | **This plan, matrix E** | Matrix E already requires clean Apple Silicon and Intel macOS machines and signed/notarized DMGs. The setup is identical. |
| Packaged Windows build: Credential Manager smoke + restart | **This plan, matrix E** | Matrix E already requires a clean Windows 10/11 x64 machine. |
| Packaged Linux build: Secret Service smoke + restart | **Plan 008, unchanged** | This plan has **no** Linux environment. Folding it in would silently widen the hardware requirements of a release-blocking plan, and a requirement nobody can meet is a requirement that gets waived. |

Nothing is lost: Plan 008 keeps the Linux row and now points at this plan for
the other two. The connected-apps credential store is the same `keyring`-backed
store licensing uses, so proving it under a signed macOS identity and a
packaged Windows identity here covers both features on those two platforms.

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

### A. Alfred License purchase and hosted benefits

One product, one run. There is no annual/lifetime choice to test.

1. the checkout link shows the correct product, one-time price, tax treatment,
   and terms;
2. sandbox purchase completes on Polar's hosted confirmation page;
3. receipt email reaches the buyer and its portal link authenticates by code;
4. portal shows exactly one correct key and official download benefit;
5. downloaded installers/checksums equal the accepted candidate;
6. the issued key carries a **one-year expiry**, and every surface presents
   that date as an **update deadline**, never as the end of the purchase or of
   the customer's access;
7. refund/revocation produces the approved key/download transition and **does**
   end entitlement;
8. Alfred consumes no checkout success URL and no webhook.

Expected: Polar alone owns payment proof and hosted benefit access; Alfred
does not consume checkout success URLs or webhooks. There is no subscription
to cancel, so no cancellation row exists.

### B. Alfred Teams seats

One run, one-time per seat, at least three seats:

1. checkout quantity and total are correct, and are presented as a **one-time
   payment per seat**, not a recurring charge;
2. the buyer becomes owner, then explicitly assigns and claims one seat for
   themselves through Polar's hosted portal; no benefit is granted merely for
   being the billing purchaser;
3. assigning a second email sends an invitation and remains pending until claim;
4. claim grants that member an independent Teams key and downloads;
5. a third available seat can be assigned and claimed;
6. revoking a member removes their benefits and frees the assignment;
7. **adding seats behaves as an observed, recorded transaction.** Under the
   coordinator default (renewal and expansion are fresh purchases of the same
   product) this is a second purchase, not a proration. Record what Polar
   actually does; do not assume. If it prorates or converts to a recurring
   charge, that is a STOP, not a variation;
8. a refunded purchase does not over-grant, and ends entitlement for the seats
   it covered;
9. unclaimed and unrelated users receive no key or file access;
10. each claimed member's key carries its own one-year expiry.

Expected: Polar's portal is sufficient for buyer/member/seat operations and
each claimed member has an independent entitlement.

There are no proration, paid-quantity-floor, or `canceled`/`past-due`
transition rows: none of those states exist on a one-time purchase.

### C. Desktop activation and secure storage

For the **individual** and **teams** keys:

1. activate on device 1 and validate;
2. quit/relaunch and load cached status without network delay;
3. activate through device 3; device 4 receives the device-limit state;
4. deactivate an old activation in Polar, then activate a replacement;
5. Refresh maps `granted`, `revoked`, `disabled`, and invalid safely, and maps
   **`expired` to "entitled, update window closed"** — a licensed state, not a
   loss of access. `expired` must appear in the licensed badge states;
   `revoked` and `disabled` must not. Do not merge the three;
6. Deactivate this device calls Polar before clearing local credentials;
7. locked/unavailable keychain never falls back to SQLite/plaintext;
8. logs, SQLite, DOM-after-submit, URLs, and evidence contain no full key or activation ID.

Expected: the public organization/benefit IDs and `ALFRED_RELEASE_DATE` are the
only Polar-related values compiled into Alfred; no access token is present.

### D. Offline, restrictive-state, and update-window behavior

Use automated injected-clock tests for exact time boundaries, automated
injected-date tests for the update window, and packaged smoke for network
behavior. Never change the system clock, and never hand-edit a binary's baked
release date.

**Offline and restrictive state** (existing, automated, unchanged except D8):

- cached active before day 7;
- refresh due at day 7;
- transient timeout/DNS/429/5xx yields offline grace through day 30;
- day 30 boundary and after-day-30 `needsOnline` are exact;
- never-validated or unknown-benefit keys receive no grace;
- **confirmed `revoked`/`disabled` overrides grace immediately — `expired` does
  not.** `expired` is an entitled state and must not be treated as restrictive.
  This row previously grouped all three together; that grouping is the single
  highest-risk defect the two-product model introduces;
- network failure alone never displays revoked;
- all local workflows, memories, schedules, triggers, and data remain usable.

**Update window** (new):

| # | Case | Expected |
| --- | --- | --- |
| W1 | **In-window build**: `ALFRED_RELEASE_DATE` is before the key's update deadline, key `granted` | pro features on |
| W2 | **Exactly at the deadline**: `ALFRED_RELEASE_DATE == licenseUpdateDeadline` | **in window** — pro features on. The rule is `<=`. Off-by-one here decides whether a paying customer keeps features |
| W3 | **Out-of-window build**: release date after the deadline | the app **runs**, every workflow, memory, schedule, trigger, and file stays intact and usable, and only pro features are locked. Nothing is deleted, hidden, or downgraded |
| W4 | **Out-of-window explains itself once**: first run of an out-of-window build | one dismissible explanation, then silence. Not a silent lock, not a repeated nag, and never a block on local data |
| W5 | **Expired key keeps entitlement**: key past its Polar expiry, on a build that was in-window when purchased | still licensed, pro features still on, **permanently**. An expiry date is not a loss of purchase |
| W6 | **Revoked does not**: confirmed `revoked` response | entitlement ends immediately, on any build |
| W7 | **Disabled does not**: confirmed `disabled` response | entitlement ends immediately, on any build |
| W8 | **Unset release date**: `ALFRED_RELEASE_DATE` absent (source build) | **never locks anything.** No window comparison runs at all |
| W9 | **Source build is unlocked**: a build with no `ALFRED_POLAR_*` configuration | every feature available, no licence prompt, no nag. Building from source is free and fully featured, forever (Plan 008) |

W1, W2, W5, W6, W7, W8, and W9 are automated in
`src-tauri/src/licensing/acceptance.rs` with injected dates. W3 and W4 need a
packaged out-of-window candidate: build the candidate a second time with an
`ALFRED_RELEASE_DATE` deliberately past the sandbox key's deadline, and run it
on a machine that already holds real local data.

### E. Official distribution, update truthfulness, and packaged credential storage

- required DMGs verify signing/notarization/stapling;
- Windows NSIS is labeled unsigned beta everywhere;
- Polar copies match private draft checksums byte-for-byte;
- unrelated/unclaimed accounts cannot download;
- a customer whose update window has closed **can** still download newer files
  — this is expected behavior, not a defect, and the app explains it (see W4);
- customer-facing download pages link corresponding source and license notices;
- **Download latest version** opens Polar's portal;
- the acceptance manifest records `ALFRED_RELEASE_DATE` and it matches the
  value actually baked into the tested artifacts;
- no public GitHub binary, public cask, updater JSON, updater plugin, or
  automatic-update promise exists;
- **signed/notarized macOS package credential-store smoke** (moved from Plan
  008): on the signed, notarized, stapled build, create / read / overwrite /
  delete a connected-app credential in the macOS Keychain, then restart the app
  and confirm the credential persists and is still readable under the
  production app identity;
- **packaged Windows build credential-store smoke** (moved from Plan 008): the
  same create / read / overwrite / delete / restart cycle against Windows
  Credential Manager on the packaged build.

Both credential-store rows exist because development and production app
identities can receive different access, so a dev-checkout pass proves nothing
about the shipped bundle. The packaged **Linux** Secret Service equivalent is
**not** in this plan — it remains Plan 008's, because this plan has no Linux
environment.

### F. Product, legal, and support consistency

Check README, install/open-source/releasing docs, settings, Polar checkout,
portal/product descriptions, receipts, release notes, and support pages. They
must agree on:

1. **Alfred License** is sold to one named user, one-time;
2. **Alfred Teams** is licensed one-time per claimed seat;
3. pro features are **permanent** once purchased;
4. the purchase includes **one year of updates**;
5. **what lapsing does**: newer builds' pro features stay locked until the
   customer buys again;
6. **what lapsing does not do**: it never disables the build they have, never
   removes a paid feature, and never touches local data;
7. refunded / revoked / disabled **does** end entitlement, and is different
   from a lapsed window;
8. the three-device limit;
9. the 7-day refresh / 30-day offline policy;
10. manual updates through Polar;
11. refund and cancellation terms;
12. the Windows build is beta;
13. GPL source rights, and that **building from source is free and fully
    featured, forever**;
14. Teams is purchased on the marketing website, not in the app.

No surface may claim payment restricts commercial use of GPL source, and no
surface may still offer an Alfred annual, lifetime, or subscription tier. The
tier scan in "Commands you will need" narrows where to look — it legitimately
matches third-party subscriptions and labeled history, so read every hit rather
than counting them. The rest is read by a human, because a wrong promise can be
phrased without any of those three words.

## Steps

### Step 1: Freeze the candidate and sandbox configuration

Record exact source commit/version, `ALFRED_RELEASE_DATE`, artifact
filenames/checksums, Polar product/benefit labels, portal settings, and the
checkout-link label. Prevent configuration drift during acceptance.

**Verify**: all identifiers/checksums appear in the report without secrets, the
three version files agree, and the manifest's `ALFRED_RELEASE_DATE` matches the
value baked into the artifacts.

### Step 2: Execute matrices A–B

Run the Alfred License and Alfred Teams purchase/benefit/seat journeys. File a
defect for every mismatch and do not reuse corrupted test state.

**Verify**: every A–B row has reproducible PASS evidence.

### Step 3: Execute matrices C–D

Run packaged activation/secure-storage tests on required platforms, the exact
offline-state automation, and the update-window cases — automated for W1, W2,
W5–W9, and a packaged out-of-window candidate for W3 and W4.

**Verify**: every C–D row passes; `expired` reads as entitled in both C5 and
the D restrictive-state row; secret inspection is clean.

### Step 4: Execute matrices E–F and full regression

Verify artifacts, authorization, manual update UX, GPL/source links, the two
packaged credential-store smokes, and customer-facing consistency, then re-run
the full repository checks.

**Verify**: every E–F row passes; `actionlint`, `bun run check`, and
`bun run verify:release-hygiene` pass; the tier scan finds no annual, lifetime,
or subscription offer.

### Step 5: Sign the launch recommendation

List known limitations. Record GO only if no P0/P1 defect or missing required
evidence remains. A NO-GO leaves this plan `BLOCKED` with defect references.

**Verify**: the dated report and checksum are recorded and approved.

## Done criteria

- [ ] Every A–F scenario has reproducible PASS evidence.
- [ ] Alfred License and Alfred Teams both complete end to end as one-time purchases.
- [ ] Teams buyer/member claims and revokes behave correctly, and seat addition is recorded as observed.
- [ ] Three-device, secure-store, status, and deactivate behavior pass.
- [ ] Exact 7/30-day behavior and restrictive-state precedence pass.
- [ ] `expired` keeps entitlement everywhere it is consumed; `revoked`/`disabled` end it immediately.
- [ ] Every update-window case W1–W9 passes, including exactly-at-deadline and unset release date.
- [ ] An out-of-window build runs, keeps all local data, and explains itself once.
- [ ] The signed macOS and packaged Windows credential-store smokes pass.
- [ ] Required packaged artifacts and Polar copies match and are correctly labeled.
- [ ] The acceptance manifest's `ALFRED_RELEASE_DATE` matches the tested artifacts.
- [ ] No obsolete backend or updater architecture remains active.
- [ ] No surface offers an annual, lifetime, or subscription tier.
- [ ] `actionlint`, `bun run check`, and `bun run verify:release-hygiene` pass on the frozen candidate.
- [ ] No unresolved P0/P1 defect remains and GO is signed.
- [ ] The roadmap row is `DONE`.

## STOP conditions

- Polar sandbox configuration differs from intended production semantics.
- Any credential, customer PII, payment detail, or signed download URL appears in evidence/logs.
- A test requires a real payment method or production credential.
- Teams benefit/seat behavior fails, the seat beta changes materially, or adding
  seats turns out to prorate or bill recurringly.
- A restrictive license state still gains offline grace.
- **`expired` removes entitlement anywhere** — this takes away a feature the
  customer already paid for and is 007's fourth STOP condition.
- **An out-of-window build hides, drops, or refuses access to local data.**
- `ALFRED_RELEASE_DATE` is missing, malformed, or does not match the artifacts.
- A paid artifact is public, mismatched, unsigned where signing is promised, or missing source.
- A supported clean platform is unavailable.
- The candidate/config changes without resetting affected evidence.

## Maintenance notes

Keep this matrix as the release regression suite. Every Polar product, benefit,
portal, or checkout-link change invalidates the affected rows even when Alfred
source did not change.

Two rows need re-checking on every single release even when nothing else moved:
the `ALFRED_RELEASE_DATE` assertion in matrix E, and the exactly-at-deadline
case W2. Both are silent when wrong.
