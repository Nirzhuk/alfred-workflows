# Plan 002: Build Polar-backed License & Billing settings

> **Executor instructions**: Complete Plan 001 first. Keep all secret handling
> in Rust and all billing, device recovery, seats, receipts, and downloads in
> Polar's hosted portal. Follow every verification gate, stop on a STOP
> condition, and update the release-money index when done.
>
> **Drift check (run first)**:
> `git diff --stat ecb94d6..HEAD -- src/App.css src/features/settings src/features/licensing src-tauri/capabilities`
> Reconcile settings composition, shared controls, safe license DTOs, and
> opener permissions if any in-scope path changed.

## Status

- **Priority**: P0
- **Effort**: M
- **Risk**: MED (sensitive input and external navigation)
- **Depends on**: `001-connect-desktop-polar-licensing.md`
- **Category**: direction, security, tests
- **Planned at**: commit `ecb94d6`, 2026-08-15

## Why this matters

Customers need one calm place to activate Alfred, understand its last verified
state, and reach the Polar pages that actually own billing and benefits. The
screen must not manufacture renewal, role, organization, or seat data that
Polar's public license endpoint does not expose, and a network problem must
never be described as revocation or data loss.

## Current state

- `src/features/settings/components/settings-page/settings-sections.ts`
  defines current sections but no license section.
- `settings-page.tsx`, `settings-sidebar.tsx`, shared `settings-card` /
  `settings-row` controls, and `src/App.css` define the established settings
  layout.
- `docs/design-system.md` requires semantic variables, existing controls,
  keyboard access, and theme-safe styling.
- Plan 001 adds safe commands and DTOs for activate, refresh, deactivate, and
  local status. It intentionally exposes no stored key or activation ID.
- Polar's hosted portal authenticates customers by email code and owns license
  keys, activation deactivation, billing, receipts, downloads, and Company
  seats. Alfred only needs the approved public portal and checkout links.

## Required experience

Add a settings destination titled **License & Billing** containing:

- activation form for license key and editable device label;
- current product: Desktop annual, Desktop lifetime, or Company seat;
- status, masked key, last successful check, next check, offline deadline, and
  expiration only when supplied;
- Refresh and Deactivate this device actions;
- Buy Desktop, Buy for a Company, and Open Polar customer portal actions;
- explicit states for offline grace, needs online validation, expired,
  revoked, disabled, device limit, secure storage unavailable, Polar
  unavailable, and licensing not configured.

Use “Manage billing, devices, seats & downloads” for the portal action. Do not
show organization name, member role, seat counts, renewal date, or
cancel-at-period-end unless a future safe API provides them.

## Commands you will need

| Purpose | Command | Expected on success |
| --- | --- | --- |
| Frontend tests | `bun test` | all frontend tests pass |
| Frontend build | `bun run build:frontend` | TypeScript and Vite exit 0 |
| Full check | `bun run check` | all repository gates pass |
| Secret scan | `rg -n 'licenseKey|license_key' src/features --glob '!**/*.test.*'` | ephemeral form/command input only; no store, URL, or log |
| Provider scan | `rg -n 'Stripe|stripe' src src-tauri/src` | no commercial Stripe integration |

## Scope

**In scope**:

- `src/features/settings/components/settings-sidebar/**`;
- `src/features/settings/components/settings-page/**`;
- `src/features/licensing/**`;
- `src/App.css`;
- `src-tauri/capabilities/default.json` only for tightly allow-listed opener behavior;
- focused frontend tests;
- this plan and the release-money index status.

**Out of scope**:

- changing Plan 001's entitlement evaluator or credential storage;
- rendering raw Polar responses or customer data;
- custom account, Company member, seat, device-list, receipt, download, or billing UI;
- embedding Polar checkout in the Tauri webview;
- automatic updates or local-feature gates;
- analytics, trials, coupons, or additional pricing tiers.

## Git workflow

- Branch: `codex/002-polar-license-settings`.
- Use imperative commits such as `Add Polar License and Billing settings`.
- Do not push or open a PR unless instructed.

## Steps

### Step 1: Add navigation and a focused non-persisted UI store

Add `license-billing` to the settings section ID, label, navigation group, and
icon. Create a hook/store that loads `get_license_status`, allows one mutation
at a time, refreshes safe state after commands, and maps stable error codes to
customer copy. Do not persist the activation form value in Zustand,
localStorage, sessionStorage, URL state, analytics, or logs.

**Verify**: `bun run build:frontend` passes with no type/exhaustiveness errors.

### Step 2: Build activation and product status cards

Keep the entered key only in component state; allow paste and password-manager
behavior, submit it once, and clear it in `finally`. Use an ordinary device
label that the user can recognize in Polar, defaulting to a non-identifying
platform label rather than a hardware fingerprint.

Render product and effective state separately. Format Polar expiration and
local validation timestamps in the user's locale with accessible exact
values. Omit absent data rather than guessing.

**Verify**: component tests cover unlicensed activation, all three products,
loading, success, error, secret clearing, and absent expiration.

### Step 3: Add safe lifecycle actions and messages

Implement Refresh and Deactivate with disabled/busy states and accessible live
regions. Deactivation requires confirmation and makes clear that it frees this
device activation but does not cancel billing. A failed remote deactivation
must preserve existing state and offer retry.

Use distinct guidance:

- `offlineGrace`: show the exact deadline and say Alfred will retry;
- `needsOnline`: reconnect to validate official access;
- `expired`: open Polar to renew or review the purchase;
- `revoked` / `disabled`: open Polar or contact support;
- `deviceLimit`: deactivate an old device in Polar's portal, then retry;
- secure storage unavailable: unlock the OS credential store;
- not configured: official licensing is unavailable in this build, while
  local Alfred remains usable.

**Verify**: view-model/DOM tests assert every state has a heading, explanation,
action, and no workflow/data-loss claim.

### Step 4: Add allow-listed Polar checkout and portal navigation

Consume typed, injected public checkout/portal configuration. Tests use fixed
safe Polar URL fixtures; Plan 003 binds the real approved URLs. Open them with
Tauri's opener in the system browser. Allow only exact configured HTTPS Polar
hosts/paths; reject a runtime URL supplied by React or a license response.
Buttons:

- **Buy Desktop** → Desktop checkout link;
- **Buy for a Company** → Company checkout link;
- **Manage billing, devices, seats & downloads** → hosted customer portal.

Do not create pre-authenticated portal sessions. Polar will email the customer
a one-time code.

**Verify**: tests prove each action resolves only its configured destination;
wrong scheme, host, or unexpected redirect input is rejected.

### Step 5: Complete accessibility and regression checks

Confirm keyboard navigation, visible focus, form labels, status announcements,
confirmation focus return, high contrast in both themes, reduced motion, and
no overflow at the minimum 960×640 app size. Make error copy useful without
showing raw Polar status bodies.

**Verify**: `bun run check` passes.

## Test plan

- Unit-test a pure view-model mapping for every product/state/error pair.
- Component-test activation input clearing, busy guards, deactivation
  confirmation, external navigation, date formatting, and missing config.
- Use Plan 001's safe fixtures only. Never snapshot or assert a complete key,
  activation ID, customer record, or checkout session.

## Done criteria

- [ ] License & Billing is keyboard accessible from settings.
- [ ] Activation, Refresh, Deactivate, and cached Status are usable.
- [ ] Product, expiration, validation, and offline dates are accurate and conditional.
- [ ] Every restricted/transient state has distinct, truthful guidance.
- [ ] Checkout and portal actions open only configured, allow-listed Polar destinations.
- [ ] Billing, devices, Company seats, receipts, and downloads stay in Polar's portal.
- [ ] Raw credentials are absent from persisted frontend state, DOM after submit, URLs, and logs.
- [ ] `bun run check` passes.
- [ ] The roadmap row is `DONE`.

## STOP conditions

- Plan 001's safe commands/DTOs are incomplete.
- A full saved key or activation ID must be returned to React.
- Renewal, Company role, organization, or seat state would need to be guessed.
- A Polar URL cannot be fixed and allow-listed at build time.
- The design requires embedded checkout or privileged portal sessions.
- A verification gate fails twice after a scoped correction.

## Maintenance notes

Keep this UI narrower than Polar's portal. If Polar adds safe public lifecycle
fields, extend the Rust DTO and evaluator first; do not parse provider payloads
or duplicate billing logic in React.
