# Plan 003: Ship the freemium and license-management experience

> **Executor instructions**: Complete Plans 001 and 002 first. Read their
> resulting public TypeScript types before modifying UI state. Follow every
> verification gate and stop on any STOP condition. When done, update this
> plan's row in `plans/README.md`.
>
> **Drift check (run first)**: this plan was written against an unversioned
> workspace that was changing during inspection. Run `git rev-parse --short
> HEAD`. If it fails, compare the hashes/excerpts and inspect every current
> component named below. If Git exists, record HEAD and inspect in-scope paths.
> Do not restore older component layouts; adapt the plan to equivalent live
> symbols only when behavior still matches. Semantic mismatch is a STOP
> condition.

## Status

- **Priority**: P1
- **Effort**: L
- **Risk**: MED
- **Depends on**: `plans/001-polar-offline-licensing.md`,
  `plans/002-freemium-entitlement-enforcement.md`
- **Category**: direction, UX, tests
- **Planned at**: unversioned workspace snapshot, 2026-08-09

## Why this matters

The freemium boundary should feel like a natural scale limit, not punishment.
Users need clear usage, quiet offline behavior, contextual upgrade prompts, and
safe control of which three workflows remain runnable after a downgrade. The
app must open and run local models immediately even when Polar is unreachable.

## Current state

- `src/App.tsx` loads workflows and local services on mount; there is no license
  store or network-independent entitlement bootstrap.
- `SettingsPage` contains only General, Runs, and Data sections and has no plan
  or activation management.
- New-workflow actions converge on `useWorkflowStore.createWorkflow` from the
  native menu, title-bar plus button, and sidebar plus button.
- `MemoriesInspector` shows per-workflow combined memories and a `New note`
  action. Its `memories.length` is not the global owned-memory usage required
  by the product policy.
- `ScheduleModal` and `TriggersModal` save through the workflow store and show
  generic store errors.
- `@tauri-apps/plugin-opener` is already installed and initialized, but no UI
  currently opens checkout/customer-portal links.
- There is no frontend test script or test framework.

Observed startup (`src/App.tsx:18`):

```tsx
useEffect(() => {
  void loadWorkflows();
  void prepareNotifications();
  void installRunEventBridge();
}, [loadWorkflows]);
```

Observed settings shell (`settings-page.tsx:18`):

```tsx
<div className="settings-page-body">
  <section className="settings-section">
    <h2>General</h2>
    {/* ... */}
  </section>
</div>
```

Observed converged creation (`workflow-canvas.tsx`, `AppTitlebar` call site):

```tsx
<AppTitlebar
  // ...
  onNewTab={() => void createWorkflow()}
/>
```

Selected snapshot hashes:

```text
3bce624e69f9e47342747eed3b51cc9abf76daab94e3b9797064a3044e4299f2  package.json
4f7a1dc04fa855e62f97568c5450c9930a561488447c763c1a9a4ff8abde8354  src/App.tsx
b3537f4e3df807b1ff23a2e61057b1774baf5f3cafe28faa87e7569151623b08  src/features/workflow/store.ts
7b0e6a1fbf66334388c8b7947fb254f2cc8530625e23b8f7644238cfe32d069d  src/features/settings/components/settings-page/settings-page.tsx
5102f657c28b53446bbbeedd4a1e0be2f0344d90e48805209fe7eb5a645c72fa  src/features/workflow/components/workflow-canvas/workflow-canvas.tsx
5ca0bf7fb58ca31a7e7b511623fb89811fecb282af654c6703e9f3e6f5d72fab  src/features/workflow/components/memories-inspector/memories-inspector.tsx
59f486fcaaec872b84dd6b02bbc092b40dbb9008ff3728102c4c53e9f2d1bbc6  src/features/workflow/components/schedule-modal/schedule-modal.tsx
cc911bdd8377fa6565e70885af4494bc6b42b54a0db37878cdbec138fb8891f8  src/features/workflow/components/triggers-modal/triggers-modal.tsx
b324c234f79c458f5ff987e231d5fdd9e9a347cec05a98632d10e53bb18d8da3  src/App.css
```

Plans 001 and 002 are expected to add `src/features/licensing/` and change API
types. Reconcile those results instead of recreating parallel models.

## UX policy that must not drift

- Do not show an upgrade modal at launch.
- Do not watermark outputs, limit manual-run count, or hide providers/models.
- Upgrade prompts appear when the user reaches or attempts to exceed a limit.
- Display Free usage as `workflows / 3`, `owned memories / 25`, and
  `active automations / 1`.
- Do not use the current workflow's combined memory list as global usage.
- Pro offline grace is quiet until the final 9 days. Then show a small warning,
  not a blocking dialog.
- `needsRefresh` asks for one connection before Pro-only scale is restored, but
  the application and three Free workflows continue to work.
- Downgrade never deletes data. Over-limit workflows remain openable and
  editable; Run is disabled until one of the three Free slots is assigned.
- Existing over-limit memories remain editable and usable; only creating
  another owned memory is blocked.
- Plan-paused automations are visibly paused by the plan, not presented as
  broken or silently changed to disabled.

## Commands you will need

| Purpose | Command | Expected on success |
|---------|---------|---------------------|
| Install | `bun install` | exit 0 and lockfile updated only for declared test dependencies |
| Frontend tests | `bun run test` | all tests pass |
| Frontend build | `bun run build:frontend` | exit 0 |
| Rust regression | `cargo test --manifest-path src-tauri/Cargo.toml` | all tests pass |

## Scope

**In scope**:

- `package.json`
- `bun.lock`
- `vite.config.ts` or `vitest.config.ts` (choose one test configuration)
- `src/test/setup.ts` (new)
- `src/App.tsx`
- `src/App.css`
- `src/vite-env.d.ts`
- `src/menu.ts`
- `src/features/licensing/types.ts`
- `src/features/licensing/api.ts`
- `src/features/licensing/store.ts` (new)
- `src/features/licensing/config.ts` (new)
- `src/features/licensing/components/license-settings.tsx` (new)
- `src/features/licensing/components/upgrade-dialog.tsx` (new)
- focused tests under `src/features/licensing/**/*.test.ts(x)` (new)
- `src/features/settings/components/settings-page/settings-page.tsx`
- `src/features/workflow/store.ts`
- `src/features/workflow/components/workflow-canvas/workflow-canvas.tsx`
- `src/features/workflow/components/workflow-list-item/workflow-list-item.tsx`
- `src/features/workflow/components/app-title-bar/app-title-bar.tsx`
- `src/features/workflow/components/memories-inspector/memories-inspector.tsx`
- `src/features/workflow/components/schedule-modal/schedule-modal.tsx`
- `src/features/workflow/components/triggers-modal/triggers-modal.tsx`
- `plans/README.md` and this plan's status only

**Out of scope**:

- Changing backend limits or Polar state rules from Plans 001/002.
- A custom web checkout, website, backend, or authenticated portal session.
- App analytics, advertisements, watermarking, email capture, or forced login.
- Team plans, trials, coupons, pricing experiments, and localization.
- Export/import implementation; preserve space for it but do not invent it here.
- Redesigning the workflow editor or settings navigation beyond licensing needs.

## Git workflow

- Do not initialize Git in the current unversioned workspace.
- If Git exists at execution time, use branch
  `advisor/003-freemium-license-ux`.
- Do not push or open a PR unless instructed.

## Steps

### Step 1: Add focused frontend test infrastructure

Add Vitest, jsdom, React Testing Library, and jest-dom as development
dependencies. Add a `test` script that runs once in CI/non-watch mode. Keep the
configuration minimal and compatible with the current Vite React setup.

Create a test setup file for jest-dom matchers and browser API shims only when
needed. Do not add a second bundler or broad lint migration.

**Verify**:

```bash
bun run test
bun run build:frontend
```

Expected: both exit 0; an initial licensing type/parser smoke test passes.

### Step 2: Build one license store with non-blocking refresh

Create `useLicenseStore` around the APIs from Plans 001/002. It owns:

- current safe license snapshot;
- entitlement usage;
- loading flags separated into local-load, activation, and refresh;
- ordinary operational errors;
- a contextual `limitNotice` for known entitlement errors;
- actions to load local status, activate, refresh, deactivate, reload usage,
  set Free workflow slots, and dismiss a limit notice.

At app startup:

1. Load local status immediately without network and independently from
   `loadWorkflows`.
2. Do not await licensing before showing or using workflows.
3. If the local snapshot recommends refresh, start it in the background.
4. While the app remains open, check every 6 hours whether a refresh is due;
   the backend should avoid unnecessary validation.
5. A network failure leaves the offline snapshot and app functionality intact.

Never store the key again in `localStorage`; Plan 001's backend persistence is
the source of truth.

**Verify**:

```bash
bun run test -- licensing/store
```

Expected: tests prove local status renders before a pending refresh, transient
refresh failure retains offline Pro, and definitive invalid state updates Free.

### Step 3: Configure hosted Polar destinations without secrets

Use public build-time frontend variables:

- `VITE_POLAR_CHECKOUT_URL`
- `VITE_POLAR_CUSTOMER_PORTAL_URL`

Type them in `vite-env.d.ts`. Open links with
`@tauri-apps/plugin-opener`'s external URL API. Use Polar's static checkout and
default hosted customer portal; do not generate customer sessions because that
requires a privileged server-side token.

If a URL is absent in development, disable the corresponding button and show
`Licensing checkout is not configured in this build`; never crash or embed a
placeholder production URL.

**Verify**:

```bash
bun run test -- licensing/config
bun run build:frontend
```

Expected: missing and configured URL cases pass; build exits 0.

### Step 4: Add Plan and License settings

Add a first-class `Plan & license` section to the existing Settings page.
Reuse existing `settings-section`, `settings-card`, `settings-row`, `field`,
`primary`, `ghost`, `hint`, and `muted` patterns before adding new CSS.

Free state must show:

- `Free` badge and concise explanation;
- usage rows: workflows `/ 3`, owned memories `/ 25`, active automations `/ 1`;
- `Upgrade to Pro` opening hosted checkout;
- license-key input and `Activate license`;
- no requirement to create an Alfred account.

Pro state must show:

- `Pro`, `Pro — offline`, or refresh-needed wording;
- masked key only;
- last successful validation and offline-grace end when applicable;
- `Refresh license`, `Manage subscription`, and `Deactivate this device`;
- confirmation before deactivation.

Never render a full key returned from form state after activation. Clear the
input after success.

**Verify**:

```bash
bun run test -- license-settings
```

Expected: component tests cover Free, active Pro, quiet offline Pro, final-nine-
days warning, needs-refresh, activation error, and missing checkout config.

### Step 5: Present contextual limits without aggressive upselling

Make known structured entitlement errors from Plan 002 open one reusable
`UpgradeDialog`; unknown errors stay in the existing operational error surface.
The dialog copy must state the exact reached limit and offer `Not now` plus
`Upgrade to Pro`.

Route all workflow creation entry points through the existing store action so
native menu, title bar, and sidebar behave identically. Add small passive usage
text near the workflow list only for Free, such as `2 of 3 workflows`; do not
disable the button before the backend confirms the authoritative count unless
local usage is already loaded.

In Memories:

- show global owned usage, not `memories.length`;
- keep link, edit, delete, pin, and clear available at/over 25;
- when creating the 26th owned memory, show the contextual dialog;
- if already over limit after downgrade/migration, explain that existing
  memories remain safe and only new creation is paused.

In Schedule/Triggers:

- show `1 active automation on Free` near enable controls;
- display Plan 002's plan-paused IDs as `Paused on Free plan` while preserving
  the stored enabled state;
- allow disabling a stored automation so another can be enabled;
- keep trigger test runs available.

**Verify**:

```bash
bun run test -- upgrade-dialog
bun run build:frontend
```

Expected: tests cover each limit code and prove unknown failures do not become
upgrade prompts.

### Step 6: Add fair downgrade workflow selection

When Free usage contains more than three saved workflows:

- mark non-selected workflows with a subtle lock/Free-limit badge in the
  sidebar and title tabs;
- allow opening, editing, saving, and deleting them;
- disable Run and live automation controls with an explanation;
- add a Settings control to atomically choose up to three runnable workflows;
- do not close open tabs or change the active workflow automatically;
- refresh usage after deletion, selection, activation, or deactivation.

The Settings selector must prevent more than three checked workflows before
sending, and the backend remains authoritative. Copy should say `Choose the 3
workflows available on Free`, not imply data loss.

**Verify**:

```bash
bun run test -- free-workflow
```

Expected: tests prove over-limit workflows remain editable/visible, only three
are runnable, and selection changes do not delete or mutate workflow content.

### Step 7: Complete regression and offline scenarios

Add tests for the integration helpers and run all gates. Then perform the
manual desktop scenarios below; record results in this plan under a short
`Execution notes` section rather than creating a new document.

Manual scenarios:

1. Fresh Free install with no network: create/run three workflows and 25
   memories; the app never asks for a connection.
2. Attempt workflow 4, memory 26, and automation 2: contextual prompts, no data
   mutation.
3. Activate a sandbox Polar key, disconnect, restart: Pro loads immediately and
   local workflows/models run without waiting.
4. Simulate final nine days of grace: small warning, no blocking dialog.
5. Simulate grace exhausted: Free limits return, data remains.
6. Restore network and refresh: Pro features return without app restart.
7. Downgrade with more than three workflows, 25 memories, and one automation:
   choose slots; no deletion.
8. Invalid/revoked key: clear explanation and Free functionality remains.

**Verify**:

```bash
bun run test
bun run build:frontend
cargo test --manifest-path src-tauri/Cargo.toml
```

Expected: all commands exit 0.

## Test plan

- `src/features/licensing/store.test.ts`: local-first load, background refresh,
  transient errors, definitive invalidation, usage reload.
- `src/features/licensing/config.test.ts`: valid HTTPS URL, missing config,
  invalid/non-HTTPS URL rejection.
- `license-settings.test.tsx`: all license states and actions.
- `upgrade-dialog.test.tsx`: every stable limit code and accessible dismissal.
- A focused workflow entitlement UI test for three selected versus one locked
  workflow after downgrade.
- Prefer role/name assertions and behavior over large snapshots.
- Mock only the licensing API and opener boundary; do not mock the entire
  workflow store when a small real Zustand store can be reset per test.

## Done criteria

- [ ] `bun run test` exits 0.
- [ ] `bun run build:frontend` exits 0.
- [ ] `cargo test --manifest-path src-tauri/Cargo.toml` exits 0.
- [ ] Alfred launches and loads local workflows without waiting for Polar.
- [ ] Pro works offline for the policy window and local model execution never
      makes a license network request per run.
- [ ] Free usage displays 3 workflows, 25 owned memories, and 1 active
      automation.
- [ ] Manual runs, all providers/models/skills/nodes, and local history remain
      ungated.
- [ ] No launch popup, watermark, forced login, or destructive downgrade path
      exists.
- [ ] Full keys and customer PII never appear in rendered UI, errors, or logs.
- [ ] Missing Polar public URLs degrade gracefully in development.
- [ ] No files outside scope are modified.
- [ ] `plans/README.md` marks Plan 003 `DONE`.

## STOP conditions

Stop and report if:

- Plans 001/002 do not expose the expected safe snapshot, usage, structured
  errors, and slot-selection APIs.
- Opening Polar checkout or portal requires a privileged token or backend.
- The only way to show a limit is to duplicate policy constants independently
  from backend-returned limits.
- Downgrade UX would require deleting or rewriting local data.
- The current component structure differs enough that the named common store
  actions no longer converge all creation/run entry points.
- A verification command fails twice after a reasonable scoped correction.

## Maintenance notes

- Reviewers should test with the network disabled before launch, during
  activation, and after a successful activation.
- Keep copy about limits in one mapping keyed by stable entitlement error code.
- Future limits must be returned by backend usage; do not hard-code a second
  policy in React.
- The hosted customer portal is intentionally less branded in exchange for no
  backend and self-service billing compliance.
- If analytics are added later, make telemetry optional and never send workflow
  contents, prompts, memories, file paths, model outputs, or license keys.

