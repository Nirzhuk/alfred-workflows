# Plan 008: Gate pro features honestly, and keep self-built Alfred free

> **Executor instructions**: Do not start until Plan 007 has settled what
> "licensed" means. Read `plans/release-money/README.md`, Plan 007, and this
> file completely. This plan introduces the first entitlement surface in
> Alfred, so its shape matters more than its size. Stop on any STOP condition.

## Status

- **Priority**: P0
- **Effort**: L
- **Risk**: HIGH (first time payment affects what the app does)
- **Depends on**: 007
- **Category**: direction, security, product
- **Planned at**: commit `ba9ed57`, 2026-08-20

## Why this matters

Today no file outside `src/features/licensing` imports licensing at all. The
licensing stack validates a key and renders status; it gates nothing. "Pro
features" do not exist yet in any form.

This plan also reverses a recorded decision. `plans/release-money/README.md`
lists local feature gating as **rejected**, and Plan 002's migration row says
so explicitly. That rejection is superseded here. The reversal must be written
down, not quietly dropped, so a future reader does not think it was an
accident.

## The governing constraint

Alfred is GPL-3.0-or-later. Anyone may compile it, modify it, and redistribute
it. Therefore:

**A feature gate in Alfred can never be enforcement. It is a switch, and the
customer is buying the convenience of not flipping it themselves.**

Everything below follows from that sentence. The moat is signed and notarized
builds, Polar-hosted downloads, updates, and support — not capability. A user
who compiles Alfred gets everything, by design and by licence.

### Explicit non-goals

These are prohibited, not merely out of scope. They are futile against a GPL
codebase, hostile to the users this product is for, and would make the gate
dishonest:

- obfuscation, packing, or anti-debugging;
- binary integrity or tamper checks;
- phoning home to detect patched builds;
- hardware fingerprinting beyond the existing device activation;
- shipping a pro feature's code in a disabled state purely to frustrate
  someone who removes the check.

If a proposed change only makes sense as an attempt to stop a determined user,
it is out of bounds. Say so and stop.

## The mechanism: build configuration, not runtime detection

Alfred already distinguishes a distribution build from a source build at
compile time. `src-tauri/src/licensing/config.rs` reads the `ALFRED_POLAR_*`
values through `option_env!`; a build with no Polar organization configured
resolves to `notConfigured`. Reuse exactly that. Do not invent a second notion
of build identity.

| Build | How it is produced | Pro features |
| --- | --- | --- |
| **Source build** | clone and build; no Polar config in `.env` | **all unlocked**, no licence UI, no nagging |
| **Distribution build** | official signed release with Polar config baked in | require a licence in its update window |

Two properties this must have:

- A source build is a **first-class, fully functional** product. It must not
  nag, badge itself as unlicensed-in-a-bad-way, degrade, expire, or ask for a
  key. Plan 007's badge already renders `notConfigured` in a neutral tone;
  keep that.
- The switch is legible. Someone reading the source should find one obvious
  place that decides this, and should be able to build the unlocked app
  without patching anything — because that is the offer.

## Entitlement surface

Add exactly one entitlement authority. Not a boolean sprinkled through
components.

- One resolver that answers "is this capability available right now?" from:
  build kind (source vs distribution), licence state from Plan 007, and the
  in-window rule from Plan 007.
- Capabilities are named, enumerated values — not free-form strings, and not
  the licence state itself. UI asks about a capability; it never re-derives
  entitlement from `LicenseState`.
- Exactly one UI treatment for a locked capability, used everywhere, that
  explains what it is and how to unlock it without shaming or dark patterns.
- A locked capability must never destroy, hide, or corrupt existing user data.
  A workflow that already uses a pro feature must keep loading, keep its
  contents, and say plainly which step is locked. **Never silently drop a node
  from a saved graph.**

## The decision this plan cannot make

**Which capabilities are pro is not specified and must be approved before
implementation.** Do not guess, and do not pick features by how easy they are
to gate. Guidance for whoever decides:

- Gate things that cost the project money or that scale with professional use.
  Do not gate basic correctness, data access, or export.
- Never gate the ability to get your own data out. Export and history stay free
  in every build, or the product becomes hostage-taking.
- The free tier must remain genuinely useful, because a self-built Alfred is
  the same app and will be compared against it.
- Fewer, clearer pro capabilities beat a long thin list.

Record the approved list in this plan before writing code.

## Scope

**In scope**:

- one entitlement module in `src/features/licensing/**` plus its Rust
  counterpart if enforcement must exist below the UI;
- the shared locked-capability UI treatment;
- call sites for the approved capability list only;
- docs stating plainly that source builds are unlocked;
- this plan and the roadmap.

**Out of scope**:

- any prohibited technique in "Explicit non-goals";
- changing licence validation, the state machine, or the offline policy;
- a backend, telemetry, or usage metering;
- gating anything not on the approved list.

## Steps

### Step 1: Write down the reversal and the approved capability list

Update `plans/release-money/README.md` so the rejected-feature-gating entry is
explicitly superseded by this plan, with the reason. Record the approved pro
capability list here. Do not proceed without it.

**Verify**: a reader can see what changed, when, and why.

### Step 2: Build the entitlement resolver

One pure, fully unit-tested function from (build kind, licence state, window)
to a capability decision. Test the matrix exhaustively, including: source build
with no licence, distribution build with no licence, valid in-window licence,
expired-but-entitled licence, revoked licence, offline grace, and
`secureStorageUnavailable`.

The critical assertions: **a source build is never locked**, and **an expired
licence never loses a capability on the build it was bought for**.

**Verify**: `bun run check` passes; the matrix has no untested cell.

### Step 3: Add the one locked-capability treatment

Build the single shared UI for a locked capability. Accessible name, keyboard
reachable, both themes, honest copy, and a route to purchase that reuses the
existing Polar public-links allow-list rather than a new navigation path.

**Verify**: colocated tests per `.claude/rules/component-structure.md`; tokens
only, per `.claude/rules/design-system.md`.

### Step 4: Apply it to the approved capabilities only

Wire the resolver at the approved call sites. Every locked path must degrade
without data loss, and saved workflows using a locked capability must still
load and still be openable.

**Verify**: a test loads a workflow using a pro capability under a
distribution build with no licence and asserts nothing is dropped.

### Step 5: Make the offer honest in docs

State in `README.md` and `docs/open-source.md` that building from source gives
every feature at no cost, that payment buys signed builds, downloads, one year
of updates, and support, and that pro features are permanent once purchased.

**Verify**: `bun run verify:release-hygiene` all-PASS; no doc claims payment
is required to use Alfred.

## Done criteria

- [ ] The rejected-gating decision is explicitly superseded in writing.
- [ ] The approved pro capability list is recorded before any gating code.
- [ ] One entitlement resolver exists; no component re-derives entitlement.
- [ ] Source builds are unlocked and never nag, proven by test.
- [ ] An expired licence keeps every capability on its own build, proven by test.
- [ ] No locked path loses, hides, or drops user data.
- [ ] No prohibited anti-tamper technique exists anywhere in the diff.
- [ ] `bun run check` and `verify:release-hygiene` pass.

## STOP conditions

- Someone asks for obfuscation, tamper detection, or phone-home enforcement.
- A proposed gate would withhold a customer's own data or export.
- A gate would drop or corrupt part of a saved workflow.
- The pro capability list is still unapproved when implementation would start.
- Gating would make the source build second-class rather than equal.

## Maintenance notes

Every new pro capability is a product decision and a promise. Adding one that
was previously free retroactively takes something away from existing users;
that requires an explicit decision and a migration story, not a code review.
