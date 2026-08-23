# Plan 007: Move to two products with perpetual features and a one-year update window

> **Executor instructions**: This plan changes the product model that Plans
> 001–006 encoded. Read `plans/release-money/README.md` first, then this file
> completely. It is a state-machine and data-model change, not a UI change.
> Plan 008 (pro-feature entitlement) depends on it and must not start until the
> meaning of "licensed" here is settled. Stop on any STOP condition.
>
> **Drift check (run first)**:
> `git diff --stat ba9ed57..HEAD -- src-tauri/src/licensing src/features/licensing scripts/polar plans/release-money`

## Status

- **Priority**: P0
- **Effort**: L
- **Risk**: HIGH (changes what a paying customer is entitled to)
- **Depends on**: 001–002 (done), 003 seam (done)
- **Blocks**: 008, and any launch
- **Category**: direction, migration
- **Planned at**: commit `ba9ed57`, 2026-08-20

## Why this matters

The shipped model has four products across three benefit classes
(`desktopAnnual`, `desktopLifetime`, `companySeat`). The approved model is two
products, and the entitlement is perpetual rather than subscription-shaped.
Leaving the old shape in place would make Alfred label a customer's license
with a plan they never bought.

More importantly, this plan resolves a contradiction. The current roadmap says:

> Backendless lifetime purchases include future Polar-hosted Alfred releases.
> **Do not advertise an unenforceable limited update window.**

The new model *is* a limited update window. It becomes honest and enforceable
only through the mechanism in "The update window" below. If that mechanism is
rejected, the offer must change — not the warning.

## Approved product model

Two products. Both are one-time purchases.

| Product | Sold as | Entitlement |
| --- | --- | --- |
| **Alfred License** | one-time, one named user, not seat-based | pro features forever + 1 year of updates |
| **Alfred Teams** | one-time per seat, seat-based | same, per claimed seat |

- Paying once unlocks every pro feature **permanently**. Nothing a customer
  paid for is ever taken away.
- The purchase includes updates released within **one year** of purchase.
- After that year the app keeps working exactly as it did. The customer simply
  stops being entitled to *newer builds'* pro features. This is the standard
  perpetual-fallback model.
- A refunded, revoked, or disabled license is different from a lapsed update
  window and **does** end entitlement.

## The update window — the only enforceable design

Polar's File Downloads benefit is perpetual and cannot express a one-year
window. There is no Alfred backend. So the window cannot be enforced at
purchase or download time. It is enforced by comparing two dates the client
already trusts:

1. **`ALFRED_RELEASE_DATE`** — the release date of *this build*, baked in at
   compile time by `src-tauri/build.rs`, exactly like the existing
   `ALFRED_POLAR_*` values. It is not read at runtime and cannot drift.
2. **The license update deadline** — from Polar's license-key validation
   response. Issue keys with a one-year expiry so Polar owns the date.

The rule is one line: **a build is in-window when
`ALFRED_RELEASE_DATE <= licenseUpdateDeadline`.**

Consequences, all of which are correct and must be preserved:

- An in-window build with a valid key: pro features on.
- The customer's existing install keeps pro features **forever**, because its
  release date never changes and was inside the window when purchased.
- A build released after the deadline runs fine, keeps all local data and
  workflows, but its pro features are locked until the customer renews.
- Downloading is not the boundary; running a newer build is. Polar will still
  hand an out-of-window customer the new files. The app must explain this
  clearly on first run rather than letting them feel tricked.

### What this means for `expired`

`expired` currently reads as "no longer licensed". Under the new model a Polar
key that has passed its expiry still proves a completed purchase, so `expired`
must become **"entitled, update window closed"**, not a loss of access. This is
the single most important behavioral change in the plan.

`revoked` and `disabled` keep their current meaning and still end entitlement
immediately. Do not merge these three states.

## Decisions that must be approved before implementation

- The exact price of each product, and whether Teams is per-seat one-time or
  per-seat recurring. "One-time per seat" and Polar's seat-based product are
  not obviously the same thing — confirm in the sandbox before building.
- Whether a renewal is a fresh purchase or a discounted upgrade product.
- What the customer sees when their window lapses, in the customer's words.
- Whether an out-of-window build locks pro features silently or on an
  explicit, dismissible first-run explanation. (Recommended: explain once.)

## Scope

**In scope**:

- `src-tauri/src/licensing/models.rs` — the `LicenseProduct` enum;
- `src-tauri/src/licensing/service.rs` — state derivation, `expired` semantics;
- `src-tauri/src/licensing/config.rs` and `src-tauri/build.rs` — the two
  benefit IDs and `ALFRED_RELEASE_DATE`;
- `src-tauri/src/db/license.rs` and any migration for stored product values;
- `src/features/licensing/**` — types, view-model, labels, badge copy;
- `scripts/polar/**` — manifest shape and verifier expectations;
- `.env.example`, `docs/polar-operator-handoff.md`, this plan, the roadmap.

**Out of scope**:

- Which features are "pro" and how they are gated — that is Plan 008.
- Any Alfred backend, webhook, or licence server.
- An automatic updater.
- Anti-tamper, obfuscation, or integrity checking. See Plan 008.

## Steps

### Step 1: Reduce the product model to two

Replace `desktopAnnual | desktopLifetime | companySeat` with the two approved
classes plus `none`. Keep the wire format explicit and versioned; do not let a
stored legacy value silently deserialize into the wrong class.

Write the migration so an existing row holding `desktopAnnual` or
`desktopLifetime` maps to the individual licence and `companySeat` maps to
Teams. There are no production customers yet, so this is cheap now and
expensive after launch.

**Verify**: `cargo test` covers every legacy → new mapping and rejects an
unknown value rather than defaulting it.

### Step 2: Split entitlement from the update window

Introduce an explicit update deadline on the license status and thread it
through to the frontend contract. Keep it redacted-safe: a date is fine, a key
is not.

Change `expired` so it reports entitlement intact and the window closed. Audit
every consumer of `LicenseState` for the old assumption — including the badge
from the last release, whose `LICENSED_BADGE_STATES` set currently excludes
`expired`.

**Verify**: `src-tauri/src/licensing/acceptance.rs` gains cases proving an
expired key keeps entitlement, and that `revoked`/`disabled` still remove it.
The existing 7-day/30-day offline boundaries must not regress.

### Step 3: Bake and use the release date

Add `ALFRED_RELEASE_DATE` to `build.rs` alongside the `ALFRED_POLAR_*` values,
sourced from the release pipeline, not from the local clock at build time in a
developer checkout. An unset value means "source build" and must never lock
anything.

Implement the in-window comparison as one pure, unit-tested function. Do not
scatter date maths through the UI.

**Verify**: tests for in-window, exactly-at-deadline, out-of-window, and unset
release date. Off-by-one here decides whether a paying customer keeps features.

### Step 4: Rebind Polar to two benefits

Reduce the manifest and `.env` surface to two benefit IDs. The operator has
reset the Polar products, so the old sandbox IDs
(`69d283e8…`, `caed58b2…`) are stale — treat any value currently bound as
provisional and re-verify against the new products.

Configure Polar to issue license keys with a one-year expiry so the deadline
comes from Polar, not from Alfred's arithmetic.

**Verify**: `bun test scripts/polar` passes offline; the manifest rejects a
three-benefit shape; the handoff doc lists exactly the values now required.

### Step 5: Make every customer-facing surface agree

README, install/open-source/releasing docs, checkout copy, settings, and the
title-bar tag must all state: one-time purchase, permanent features, one year
of updates, what lapsing does and does not do, and that source builds are free.

**Verify**: `bun run verify:release-hygiene` stays all-PASS and no surface
still mentions annual/lifetime/subscription tiers.

## Done criteria

- [ ] Exactly two products and two benefit classes exist in code, manifest, and Polar.
- [ ] Legacy product values migrate deterministically; unknown values are rejected.
- [ ] `expired` preserves entitlement; `revoked`/`disabled` do not.
- [ ] The in-window rule is one tested pure function; unset release date never locks.
- [ ] Offline 7/30-day behavior is unchanged and still exact.
- [ ] Every customer-facing surface states the same promise.
- [ ] `bun run check` and `verify:release-hygiene` pass.

## STOP conditions

- Polar cannot issue a one-time product with an expiring license key.
- Polar's seat-based product cannot be sold one-time per seat.
- The client-side window rule is rejected, and no other enforcement point is
  approved — in that case the one-year promise must be removed from the offer,
  not advertised unenforceably.
- Any design appears that removes a feature the customer already paid for.

## Maintenance notes

Every release must set `ALFRED_RELEASE_DATE` correctly. A wrong date silently
grants or denies entitlement to real customers and will not fail any test.
Treat it as release-critical, and assert it in the acceptance manifest.
