# Reconciliation audit: Plans 003, 004, and 005 against the two-product model

> **What this is**: an audit, not a rewrite. It maps every place Plans 003,
> 004, and 005 still encode the superseded FOUR-product model, states what in
> them survives untouched, enumerates the human inputs that block progress, and
> gives a readiness verdict per plan. It changes no plan. The rewrite is a
> separate, coordinator-approved action.
>
> **Audited at**: commit `5e62adf`, 2026-08-20, branch `main`, clean tree.
> **Sources read in order**: `README.md`,
> [007](007-two-product-perpetual-model.md),
> [008](008-pro-entitlement-and-source-freedom.md),
> [003](003-configure-polar-commerce.md),
> [004](004-publish-signed-polar-downloads.md),
> [005](005-run-polar-paid-release-acceptance.md).

## The change being reconciled against

| | Old (encoded in 003/004/005) | New (Plan 007, approved 2026-08-20) |
| --- | --- | --- |
| Products | 4 — Desktop annual, Desktop lifetime, Company monthly, Company annual | 2 — **Alfred License**, **Alfred Teams** |
| Billing shape | annual subscription, one-time, monthly-per-seat, yearly-per-seat | **both one-time**; Teams is one-time *per claimed seat* |
| License-key benefits | 3 (`desktopAnnual`, `desktopLifetime`, `companySeat`) | 2 |
| Key expiry | lifetime key must have **no** expiry | **every** key carries a one-year expiry (Polar owns the date) |
| Entitlement | subscription-shaped; `expired` = not licensed | perpetual features + 1-year update window; `expired` = **entitled, window closed** |
| Update window | "do not advertise an unenforceable window" | enforced client-side: `ALFRED_RELEASE_DATE <= licenseUpdateDeadline` |
| Checkout links | Desktop link + Company link | Desktop-equivalent only (Teams sold off-app — already true in code) |

Every row above is a drift axis. The sections below locate it by file and line.

---

## 1. Drift map

**Totals: 87 drift sites** — 003: 44 · 004: 9 · 005: 17 · code and operator
artifacts: 17. Counts are of distinct cited locations, not of word matches.

### 1.1 Plan 003 — `003-configure-polar-commerce.md` (44 sites)

#### The product/benefit table (the core of the drift)

| Line | Quoted text | Why it drifts |
| --- | --- | --- |
| 77 | `Create these sandbox products:` | Introduces the four-product table below. |
| 81 | `\| Desktop annual \| standard subscription \| yearly \| Desktop annual key + downloads \|` | Product 1 of 4; subscription billing no longer exists. |
| 82 | `\| Desktop lifetime \| standard one-time \| one payment \| Desktop lifetime key + downloads \|` | "lifetime" is retired; the surviving one-time product is Alfred License with a 1-year update window. |
| 83 | `\| Company monthly \| seat-based subscription \| monthly per seat \| Company seat key + downloads per claimed seat \|` | Monthly recurring seat billing is removed by 007. |
| 84 | `\| Company annual \| seat-based subscription \| yearly per seat \| ... \|` | Yearly recurring seat billing is removed by 007. |
| 86 | `Create three separate License Keys benefits so Alfred can identify the safe` | Benefit count is now two. |
| 89–91 | `- Desktop annual;` / `- Desktop lifetime;` / `- Company seat, shared by monthly and annual Company products.` | All three benefit names change; the "shared by monthly and annual" rationale disappears. |
| 93–94 | `Every key benefit uses a three-activation limit. Create one shared File` / `Downloads benefit for official installers and attach it to all four products.` | Three-activation limit **survives**; "all four products" does not. |
| 95–96 | `For seat-based products, both benefits must be granted to each claimed member,` / `not only to the billing customer.` | Still true for Teams, but must be re-proved against a **one-time** seat product, which 007 L97–98 flags as not obviously the same Polar object. |

#### Pricing and policy decisions

| Line | Quoted text | Why it drifts |
| --- | --- | --- |
| 100 | `- default currency and exact annual/lifetime/monthly/yearly prices;` | Four price points; the new model needs exactly two. |
| 103–104 | `- confirmation that Desktop annual receives new downloads only while its` / `  subscription benefit is active;` | There is no subscription. Under 007, Polar's File Downloads benefit stays **perpetual** and the window is enforced client-side. |
| 105–107 | `- confirmation that Desktop lifetime includes future Polar-hosted Alfred` / `  releases; Polar's one-time File Downloads benefit is perpetual, so a` / `  backendless limited update window is not an approved alternative;` | **Directly contradicted.** 007 approves exactly the limited update window this line forbids, via `ALFRED_RELEASE_DATE`. |
| 110 | `- Company minimum and maximum self-serve seat count.` | Reframes as Teams seat bounds on a one-time purchase, which has different resize semantics. |
| 112–114 | `Do not copy another product's prices. If perpetual future downloads make the` / `lifetime offer economically unacceptable, remove that offer before continuing` / `instead of publishing a window Polar cannot enforce without custom machinery.` | Superseded: 007 keeps the offer *and* the window because the window is now enforceable client-side. |

#### Counts embedded in commands, scope, and steps

| Line | Quoted text | Why it drifts |
| --- | --- | --- |
| 21 | `- **Risk**: HIGH (external billing and beta seat semantics)` | Risk framing assumes recurring seat billing beta; the new risk is one-time-per-seat behavior. |
| 40–41 | `license, seat, portal, and download services only if the sandbox proves the` / `exact product model. Company seat-based pricing is currently beta, so its` | "the exact product model" is now the two-product model. |
| 122 | `\| Sandbox verifier \| \`bun run verify:polar-sandbox\` \| all three configured benefit classes activate, validate, and deactivate; no secret output \|` | Expected output is now two benefit classes. |
| 135 | `- four sandbox purchase/benefit journeys;` | Two journeys. |
| 137 | `  three benefit IDs, checkout-link URLs, and portal URL;` | Two benefit IDs; one checkout link. |
| 169–170 | `Create a typed, non-secret manifest for the expected organization ID, three` / `benefit IDs, two checkout links, portal URL, and product/benefit labels.` | Two drifts in one sentence: benefit count (3→2) **and** checkout-link count. `scripts/polar/manifest.ts:57-59` already ships **one** link (`checkoutLinks.desktop`), so this line is already stale against shipped code, before 007. |
| 171 | `\`verify:polar-sandbox\` script that reads the three test license keys only from` | Two keys. |
| 173 | `endpoints, checks benefit allow-listing and three-activation behavior, and` | Three-activation behavior **survives**. |
| 183 | `Create or select Alfred's Polar organization in sandbox. Enable seat-based` | Still needed for Teams, but must be validated as one-time-per-seat (007 STOP L203). |
| 193 | `Create the four products and three License Keys benefits exactly as specified.` | Both counts wrong. |
| 194–195 | `Use recognizable Alfred prefixes, three activations, automatic subscription` / `revocation, and no arbitrary expiration on lifetime ownership.` | **Directly contradicted by 007 Step 4**: keys must now be issued *with* a one-year expiry. "automatic subscription revocation" has no subscription to hang off. |
| 198–199 | `**Verify**: dashboard inspection shows four products, three distinct benefit` / `IDs, and the exact attachment matrix; no product grants two license keys.` | Verification gate asserts the old counts; would fail a correct new configuration. |
| 201 | `### Step 4: Configure Company seats and portal ownership` | Whole step is "Company", now Teams. |
| 203–204 | `Enable seat management in Polar's hosted portal. Configure Company checkout so` / `the buyer can purchase a bounded quantity and receives a member/owner record.` | Seat-quantity billing semantics assume a recurring per-seat subscription. |
| 207–208 | `benefits automatically; the owner must assign and claim one purchased seat for` / `themselves through Polar's hosted flow.` | Survives as behavior, but unproven for a one-time seat product. |
| 211–212 | `Confirm that a billing owner can assign, resend, revoke, and resize seats and` / `that every claimed member—not merely the purchaser—receives the Company key` | "resize seats" on a one-time purchase is undefined — is it a second purchase? 007 L99 flags the equivalent renewal question and leaves it unapproved. |
| 215–218 | `**Verify**: one three-seat sandbox order creates the purchaser as owner without` / `granting benefits prematurely; ... A second email can claim another seat` | Gate is written for a recurring three-seat subscription order. |
| 222–223 | `Create a Desktop checkout link offering annual/lifetime choice and a Company` / `checkout link offering monthly/annual seat-based choice.` | Both link definitions drift. Also already stale against code: `docs/polar-operator-handoff.md:215` records the Company link as `⚫ **NOT USED BY THE APP**`. |
| 229–230 | `... correct product choices, currency, tax display,` / `quantity controls, terms, and return behavior.` | "product choices" now means no choice — two separate products, not an annual/lifetime toggle. |
| 234 | `Create one File Downloads benefit and attach it to all four products.` | Two products. |
| 236–237 | `and claimed Company members receive personal signed download access, while an` / `unclaimed invite and an unrelated email do not.` | Naming only; behavior survives for Teams. |
| 244 | `Run \`bun run verify:polar-sandbox\` against the three test key-benefit types.` | Two types. |
| 248–250 | `Cancel/end an annual subscription and revoke a Company member in sandbox.` / `Confirm their keys become non-granted according to Polar's documented timing.` / `Confirm the lifetime key remains granted.` | **Whole gate is obsolete.** There is no subscription to cancel. Under 007 an expired key stays *entitled*; "non-granted" is the wrong expectation. |
| 262 | `- Desktop annual, Desktop lifetime, and Company seat benefit IDs;` | Two benefit IDs, renamed. |
| 263 | `- Desktop and Company checkout-link URLs;` | One link. |
| 269–270 | `class, activate each benefit class, relaunch, refresh, open both checkout links` (L269) / `and the portal, then deactivate.` | "both checkout links" — one exists. |
| 273 | `class; all three activation classes and configured links work on one desktop;` | Two classes. |
| 278 | `- [ ] Pricing, tax, refund, lifetime-update, device, Company, and support policy are approved.` | "lifetime-update" and "Company" are retired terms. |
| 280 | `- [ ] Four sandbox products and the three-key/one-download benefit model exist.` | Both counts wrong. |
| 281 | `- [ ] Company purchase, buyer self-assignment/claim, member claim, seat revoke, and seat resize pass.` | "seat resize" is undefined on a one-time purchase. |
| 282 | `- [ ] Desktop annual/lifetime and Company checkout links show correct live sandbox data.` | Product names and link count. |
| 285 | `- [ ] Subscription end and Company revoke remove access; lifetime remains granted.` | No subscription; and under 007 a lapsed window does **not** remove access. |
| 294 | `- A claimed Company member cannot receive an independent license key and download benefit.` | Rename to Teams; behavior survives. |
| 297 | `- Lifetime or subscription revocation semantics contradict Alfred's published promise.` | The published promise changed. |
| 298 | `- The operator wants a limited lifetime-download window but will not remove the lifetime offer.` | **Inverted by 007.** The limited window is now the approved design. |
| 300 | `- Final price, refund, update, seat, or support policy is unresolved.` | Still a valid STOP; the *contents* change (two prices, not four). |
| 304–305 | `Polar's seat model is beta. Re-run the Company proof before changing portal` / `settings, confirmation URLs, benefits, or product pricing type.` | Survives as a maintenance rule; "Company" renames and the proof itself must be rebuilt for one-time seats. |

**Also missing from 003 (drift by omission)**: no step configures the one-year
license-key expiry required by 007 Step 4; no step records
`ALFRED_RELEASE_DATE`; no step re-verifies the two stale sandbox benefit IDs
that 007 L171–173 says the operator has already reset.

### 1.2 Plan 004 — `004-publish-signed-polar-downloads.md` (9 sites)

004 is the **least** model-coupled of the three. Its drift is concentrated in
customer-facing copy and in verification gates that name benefit classes.

| Line | Quoted text | Why it drifts |
| --- | --- | --- |
| 42–43 | `- Plan 003 creates one shared Polar File Downloads benefit attached to all` / `  Desktop products and claimed Company seats.` | "Desktop products" (plural, four) and "Company" both rename. |
| 91–98 | `Update customer and operator documentation so it consistently states:` … `- source/self-built Alfred remains usable under GPL;` | The doc-copy list has **no** line for: one-time purchase, permanent features, one-year update window, what lapsing does and does not do. 007 Step 5 and 008 Step 5 both require exactly those. Drift by omission at the point where copy is authored. |
| 100–101 | `Use live Polar checkout/portal destinations approved in Plan 003 without` / `embedding prices that can drift.` | Depends on 003's approved destinations, which are the four-product ones. |
| 114–120 | `### Step 2: Make the in-app update action truthful` … `Do not fetch a signed file URL inside Alfred.` | The action is still truthful, but 007 L79–82 adds a new obligation this step does not carry: an out-of-window customer **will** be handed newer files by Polar, and the app must explain that rather than let them discover it. |
| 133–134 | `produces a text/JSON acceptance manifest containing version, source commit,` / `filenames, sizes, architectures, and SHA-256 checksums—never signing secrets.` | **Missing field.** 007's maintenance note says `ALFRED_RELEASE_DATE` must be asserted in the acceptance manifest; a wrong date "silently grants or denies entitlement to real customers and will not fail any test." |
| 143 | `Linux remains source/best-effort unless the operator separately approves it as` | Model-independent, but still an unresolved operator decision (see §3). |
| 170–172 | `**Verify**: Desktop annual, Desktop lifetime, and a claimed Company sandbox` / `member each download the exact files; unrelated/unclaimed users cannot;` | Names three retired benefit classes; becomes License + a claimed Teams seat. |
| 200 | `- Polar sandbox E2E covers three benefit classes and unauthorized denial.` | Two classes. |
| 229–230 | `not. Re-verify downloads for every product/seat benefit after changing Polar` / `attachments.` | Rename only; rule survives. |

### 1.3 Plan 005 — `005-run-polar-paid-release-acceptance.md` (17 sites)

| Line | Quoted text | Why it drifts |
| --- | --- | --- |
| 33 | `- four Polar sandbox products;` | Two. |
| 34 | `- three license-key benefits with a three-device activation limit;` | Two benefits; the three-device limit **survives**. |
| 36 | `- Desktop and Company checkout links using Polar's confirmation page;` | One link. |
| 106 | `### A. Desktop purchase and hosted benefits` | Section title renames. |
| 108 | `Run Desktop annual and lifetime separately:` | Becomes a single Alfred License run. |
| 115–116 | `6. annual cancellation keeps access through the documented paid period, then` / `   revokes according to the approved policy;` | **Row deleted.** No subscription exists to cancel. |
| 117 | `7. lifetime remains granted and does not invent a renewal date;` | Reframes: *every* key now has an expiry date, and that date must **not** read as loss of entitlement. Opposite assertion. |
| 118 | `8. refund/revocation produces the approved key/download transition;` | Survives — refund/revoke still ends entitlement under 007. |
| 123 | `### B. Company monthly/annual seats` | Whole 10-row matrix is written for recurring per-seat billing. |
| 125 | `Run Company monthly and annual with at least three seats:` | Two runs collapse to one Teams run. |
| 135–137 | `   silently reducing the paid quantity;` / `7. reducing paid quantity cannot go below assigned seats;` / `8. adding/reducing seats applies Polar's displayed proration correctly;` | **Rows B6–B8 are subscription-only.** Proration and paid-quantity reduction have no meaning on a one-time per-seat purchase. |
| 138 | `9. canceled/past-due/refunded subscription transitions do not over-grant;` | `canceled`/`past-due` do not exist on a one-time product; `refunded` survives. |
| 146 | `For annual, lifetime, and Company keys:` | Two key classes. |
| 152 | `5. Refresh maps \`granted\`, \`revoked\`, \`disabled\`, invalid, and expired safely;` | **Semantic drift, not naming.** 007 §"What this means for `expired`" makes `expired` mean *entitled, window closed*. "Safely" now means the opposite of what the current code does — `src/features/licensing/view-model.ts:121-125` excludes `expired` from `LICENSED_BADGE_STATES`. |
| 170 | `- confirmed revoked/disabled/expired overrides grace immediately;` | **The single highest-risk row.** 007 L88–92: "Do not merge these three states." `expired` must be removed from this set; `revoked`/`disabled` stay. |
| 189–190 | `must agree on named-user Desktop, Company per claimed seat, device limit,` / `offline policy, manual updates, refund/cancellation terms, lifetime scope,` | Matrix F consistency contract names retired products and "lifetime scope". |
| 237–238 | `- [ ] Desktop annual/lifetime and Company monthly/annual complete end to end.` / `- [ ] Company buyer/member claims, revokes, and quantity changes behave correctly.` | Done criteria assert the four-product journeys. |
| 252 | `- Company benefit/seat behavior fails or the beta changes materially.` | Rename; STOP survives. |

**Missing from 005 (drift by omission)** — no matrix row exists for:

1. an out-of-window build (release date **after** the key deadline) running
   with pro features locked, all local data intact;
2. an in-window build keeping pro features **forever** after the window lapses;
3. exactly-at-deadline boundary behavior (007 Step 3 calls the off-by-one
   decisive for whether a paying customer keeps features);
4. an unset `ALFRED_RELEASE_DATE` (source build) never locking anything;
5. the source-build-is-unlocked assertion from 008 Step 2.

### 1.4 Code and operator artifacts the plans point at (17 sites)

#### `scripts/polar/` — **yes, it still assumes the old model**

`scripts/polar/verify-sandbox.ts` is a thin CLI wrapper; the model assumption
lives in the modules it imports. Precisely: it assumes **three benefit
classes**, which is the old four-product model's benefit shape.

| File:line | Quoted text | Why it drifts |
| --- | --- | --- |
| `scripts/polar/manifest.ts:10-14` | `export const BENEFIT_CLASSES = [` / `"desktopAnnual",` / `"desktopLifetime",` / `"companySeat",` | The single source of truth for the benefit set. Every other drift below flows from it. |
| `scripts/polar/manifest.ts:197-209` | `desktopAnnual: parseResource(` … `companySeat: parseOptionalResource(` | Parser requires the three-class shape. 007 Step 4 requires the manifest to **reject** a three-benefit shape. |
| `scripts/polar/manifest.ts:226-230` | `"organizationId and the three benefit IDs must all differ"` | Error copy states the old count. |
| `scripts/polar/verifier.ts:23-27` | `const CLASS_SLUG: Record<BenefitClass, string> = {` / `desktopAnnual: "desktop-annual",` / `desktopLifetime: "desktop-lifetime",` / `companySeat: "company-seat",` | Case names printed in verifier output carry the old product names into evidence. |
| `scripts/polar/verifier.ts:39` | `(kind === "desktopLifetime" && license.expires_at !== null)` | **Hard contradiction.** This asserts the lifetime key has *no* expiry. 007 Step 4 requires **every** key to carry a one-year expiry. Run unchanged against a correctly configured new sandbox, this line fails the verifier. |
| `scripts/polar/verifier.ts:158` | `for (const kind of BENEFIT_CLASSES) {` | Loops the three classes. |
| `scripts/polar/secrets.ts:11-15` | `desktopAnnual: "POLAR_TEST_DESKTOP_ANNUAL_KEY",` / `desktopLifetime: "POLAR_TEST_DESKTOP_LIFETIME_KEY",` / `companySeat: "POLAR_TEST_COMPANY_SEAT_KEY",` | Three secret env-var names to rename; partial-set detection (L66–71) counts against the old set. |
| `scripts/polar/secrets.ts:46-48` | `desktopAnnual: requiredKey(...)` / `desktopLifetime: ...` / `companySeat: ...` | Secret-file schema. |
| `scripts/polar/verify-sandbox.ts:18-24` | `"  sandbox values from Plan 003 Steps 3 and 5: the organization ID, the"` / `"  two Desktop benefit IDs, and the Desktop sandbox checkout link shaped"` | Operator help text names two *Desktop* benefits + optional Company seat. |
| `scripts/polar/verify-sandbox.ts:29-33` | `"  Supply the three sandbox TEST license keys one of two ways:"` / `'       object with "desktopAnnual", "desktopLifetime", "companySeat".'` | Help text hard-codes the three key names. |
| `scripts/polar/sandbox-manifest.json:6-17` | `"desktopAnnual": { "id": "69d283e8-fa0d-4d60-a474-5e3fee5cbe71", ...` / `"desktopLifetime": { "id": "caed58b2-...", ...` / `"companySeat": { "id": null, ... }` | Three-class shape **and** the two IDs 007 L171–173 declares stale after the operator reset the Polar products. |

#### `src-tauri/` and `.env`

| File:line | Quoted text | Why it drifts |
| --- | --- | --- |
| `src-tauri/build.rs:15-17` | `println!("cargo:rerun-if-env-changed=ALFRED_POLAR_DESKTOP_ANNUAL_BENEFIT_ID");` (+ `_LIFETIME_`, `_COMPANY_SEAT_`) | Three baked env keys → two. **And missing** `ALFRED_RELEASE_DATE`, which 007 Step 3 requires here. |
| `src-tauri/build.rs:35-37` | `"ALFRED_POLAR_DESKTOP_ANNUAL_BENEFIT_ID",` / `"ALFRED_POLAR_DESKTOP_LIFETIME_BENEFIT_ID",` / `"ALFRED_POLAR_COMPANY_SEAT_BENEFIT_ID",` | Same three keys in the `.env` allow-list. |
| `.env.example:25-28` | `ALFRED_POLAR_DESKTOP_ANNUAL_BENEFIT_ID=` / `ALFRED_POLAR_DESKTOP_LIFETIME_BENEFIT_ID=` / `# Optional: Polar manages seats natively. Blank is a complete configuration.` / `ALFRED_POLAR_COMPANY_SEAT_BENEFIT_ID=` | Three keys; no `ALFRED_RELEASE_DATE`. |
| `src-tauri/src/licensing/models.rs:10-12` | `DesktopAnnual,` / `DesktopLifetime,` / `CompanySeat,` | The `LicenseProduct` enum 007 Step 1 replaces. Consumers: `config.rs`, `service.rs`, `offline.rs`, `db/license.rs`, `acceptance.rs`. |
| `src/features/licensing/types.ts:3-5` | `\| "desktopAnnual"` / `\| "desktopLifetime"` / `\| "companySeat"` | Frontend wire contract. |
| `src/features/licensing/view-model.ts:16-20` | `// \`desktopAnnual\` is a legacy internal/benefit name. The customer-facing` / `desktopAnnual: "Desktop Lifetime",` / `desktopLifetime: "Desktop Lifetime",` / `companySeat: "Company Seat",` | Already a patch over the drift — two distinct classes deliberately render as the same label. Confirms the model has been sliding for a while. |
| `src/features/licensing/view-model.ts:121-125` | `const LICENSED_BADGE_STATES: ReadonlySet<LicenseState> = new Set([` / `"active", "offlineGrace", "needsOnline",` | `expired` is excluded, so a lapsed-window customer's build currently badges itself as a **free** build. 007 Step 2 calls this out by name. |

#### `docs/polar-operator-handoff.md` — the operator's actual checklist

Model-encoded throughout; it is the document an operator would follow into a
wrong Polar configuration. Highest-value lines:

- `:25-28` — four price rows: `Desktop annual price (per year)`,
  `Desktop lifetime price (one payment)`, `Company price per seat, monthly`,
  `Company price per seat, annual`.
- `:32-33` — `Desktop annual receives new downloads **only while the subscription benefit is active**`
  and `Desktop lifetime includes **all future** Polar-hosted releases`.
- `:40-43` — the **Lifetime warning** block, superseded by 007's client-side window.
- `:65` — `Build exactly this. Four products, three License Keys benefits, one File`.
- `:72-75` — the four-product build table.
- `:77-83` — `**License Keys benefits — three, not four**` plus the three benefit rows,
  including `Desktop lifetime key | **3** | **none** — never expires` (contradicts 007 Step 4).
- `:89` and `:93-98` — the attachment matrix, `(all four)`.
- `:105` — `- [ ] The lifetime benefit has **no** expiration set.` (contradicts 007 Step 4).
- `:107-121` — the whole Company-seats step, written for a three-seat recurring order.
- `:125-126` — `Create a **Desktop** checkout link offering the annual/lifetime choice` and the Company link.
- `:146` — `Attach the single File Downloads benefit to all four products.`
- `:168` / `:175` / `:177-180` — three test-key names; `all three benefit classes`;
  `Cancel an annual subscription in sandbox`; `The lifetime key **remains** granted throughout.`
- `:211-213` — the recorded stale benefit IDs `69d283e8-…` and `caed58b2-…`,
  marked ✅ but declared provisional by 007 L171–173.
- `:384-398` — STOP list repeating the lifetime-window inversion.

#### `docs/release-acceptance/TEMPLATE-polar.md` — the evidence form

- `:25` — example resource label `sandbox / Desktop Annual / license-key benefit`.
- `:64-65` — `Run the whole matrix twice, once for **Desktop annual** and once for **Desktop lifetime**.`
- `:72-73` — A6 annual cancellation, A7 lifetime-no-renewal-date checkboxes.
- `:79-87` — nine A rows all keyed `annual \| lifetime`.
- `:91-117` — the entire Matrix B section: title `Company monthly/annual seats`,
  ten checkboxes, and ten rows keyed `monthly \| annual`, including B7 paid-quantity
  floor and B8 proration.
- `:123` and `:137-144` — Matrix C run scope and eight rows keyed
  `annual \| lifetime \| company`.
- `:171` — D8 row `a_confirmed_restrictive_response_overrides_remaining_grace_immediately` /
  `restriction applies at once` — the row that must stop treating `expired` as restrictive.
- `:212`, `:217`, `:226`, `:231` — F2 `Company is licensed per claimed seat`,
  F7 `lifetime scope agrees, including which future releases it covers`, and their rows.

---

## 2. What survives

These are already correct and must **not** be rewritten. Rework here is pure
waste and risks regressing verified work.

### Fully model-independent — no change at all

| Area | Where | Status |
| --- | --- | --- |
| macOS Developer ID signing, notarization, stapling | `reference-verified-installer-signing.md`; 004 L36–37, L54; 005 E1 | Verified and passing. |
| Accepted unsigned-beta Windows exception | 004 L37, L98, L143; 005 E2 | A recorded product decision, unaffected by pricing. |
| SHA-256 checksum discipline and byte-identity between draft and Polar | 004 L55, L134, L167–172; 005 E3 | Correct as written. |
| Updater guard (`uploadUpdaterJson: false`, no updater plugin, no updater JSON) | 004 L38–39, L56, L121–122; 005 E7 | `bun run verify:release-hygiene` proves it (see §2.1). |
| Version alignment across `package.json` / `Cargo.toml` / `tauri.conf.json` | 004 Step 3 L130–131; 005 L204 | Model-independent CI gate. |
| Required-artifact list (ARM DMG, Intel DMG, Windows x64 NSIS) | 004 L137–141 | Unchanged by the product model. |
| Release runbook and rollback ordering (add before disable, never delete before rollback is proven) | 004 Step 6 L174–191 | Correct; only benefit *names* inside it change. |
| GPL corresponding-source adjacency to paid downloads | 004 L210, L222; 005 E5 | Reinforced, not weakened, by 008. |
| Release hygiene scans (architecture / secret / updater) | `scripts/release/verify-release-hygiene.ts`; 005 L74–75 | All PASS. |
| **Matrix D** — 7-day refresh, 30-day offline grace, exact boundaries, no-grace cases, network-failure-never-reads-revoked, local data always usable | 005 L162–172 minus L170; `src-tauri/src/licensing/acceptance.rs`; TEMPLATE `:150-176` | Fully automated with injected clocks. **One row (D8) needs `expired` removed from the restrictive set — every other row stands.** 007 Step 2 explicitly requires these boundaries not to regress. |
| Secure storage: OS credential store only, never SQLite/plaintext fallback | 005 C7–C8 L150–151; 003 L42 (README) | Correct and untouched by 007/008. |
| Three-device activation limit and replacement-after-deactivate | 003 L93, L108, L173, L245–246, L284; 005 C3–C4 | Per-key, not per-product. Survives verbatim. |
| No access token / webhook secret in the app; public customer-portal endpoints only | 003 L252–253, L266–267; 005 C-expected L158–159 | Architectural, unaffected. |
| Redacting verifier: no `Authorization` header, `finally` cleanup, field-name-only errors, refuses CLI arguments | `scripts/polar/verify-sandbox.ts:66-70`, `verifier.ts:120-140`, `secrets.ts:79-86` | Security design survives intact; only the class **set** it iterates changes. |
| Evidence redaction rules (no keys, activation IDs, emails, signed URLs, payment details) | 005 L47–54; TEMPLATE `:14-25` | Survives verbatim. |
| Required clean environments (Apple Silicon, Intel, Windows x64, two browser profiles, network controls) | 005 L56–64 | Survives verbatim. |
| Polar-as-merchant-of-record architecture; no Alfred backend | README L32–37; 003 L60–62; 004 L93 | The two-product change does not reintroduce a backend. |
| Manifest link allow-listing shared with the runtime opener and the Tauri `opener:allow-open-url` scope | `scripts/polar/manifest.ts:105-135` | Correct; only the number of links changes. |

### 2.1 `bun run verify:release-hygiene` — result

Run on `main` at `5e62adf`, clean tree, 2026-08-20:

```
$ bun run verify:release-hygiene
$ bun run ./scripts/release/verify-release-hygiene.ts
PASS architecture-scan
PASS secret-scan
PASS updater-scan
```

Exit code **0**. All three gates PASS.

**Caveat**: this is a *negative* scan — it proves no Stripe/CrabNebula/license-server
architecture claim, no leaked secret, and no enabled updater remain. It does
**not** check product-model consistency, so it will keep passing while 003/004/005
and `docs/polar-operator-handoff.md` describe four products. 007 Step 5 relies on
this command staying all-PASS *and* adds "no surface still mentions
annual/lifetime/subscription tiers" — that second half is currently unenforced by
any script. Worth adding a fourth scan when 007 lands.

---

## 3. Operator input list

Every external or human input that blocks progress. Ranked: rows 1–3 unblock
the most downstream work.

| # | Input | Blocks (plan / step) | What the operator must supply or decide | What stays blocked without it |
| --- | --- | --- | --- | --- |
| 1 | **Approved price for each of the two products** | 007 L96; 003 Step 2 (L181–189), Step 3 (L191–199), Step 5 (L220–230), done-criteria L278; 005 A1, F; `docs/polar-operator-handoff.md:25-28` | Currency, exact one-time price for **Alfred License**, exact one-time per-seat price for **Alfred Teams**, tax-inclusive vs tax-exclusive display | No Polar product can be created; no checkout link; no A-matrix run; no customer-facing copy in 004 Step 1. Cascades to 003 → 004 → 005 → 006 in full. |
| 2 | **Authenticated Polar sandbox access** | 003 Steps 2–8 (L181–274); 004 Step 5 (L159–172); 005 matrices A, B, C, E | Sign-in to the Alfred Polar sandbox org with recovery tested; confirmation the org can create the products; the account's fee plan and payout/review state | Everything operator-assisted. 003 stops at Step 1; 004 stops after Step 3; 005 can only run Matrix D. |
| 3 | **Confirmation of Polar's one-time + seat behavior** | 007 L96–98 and STOP L202–203; 003 Steps 3–4; 005 Matrix B | Prove in sandbox that (a) a **one-time** product can issue a license key with a **one-year expiry**, and (b) a seat-based product can be sold **one-time per seat** rather than recurring | 007 cannot be implemented, so 003/004/005 cannot be rewritten. If either is false, 007's STOP fires and the offer itself must change. This is the *highest-uncertainty* item. |
| 4 | **Approved pro-capability list** | 008 Step 1 (L134–138) and STOP L198; 005 new window rows | The named, enumerated set of capabilities gated in distribution builds — per 008's guidance: never gate export/history/data access, keep the free tier genuinely useful, prefer few and clear | 008 cannot start at all. Without it there is nothing for the update window to gate, so the whole two-product value proposition is unverifiable in 005. |
| 5 | **Renewal design decision** | 007 L99 | Is a renewal a fresh purchase or a discounted upgrade product? | Polar product count is unsettled (a third "renewal" product would break the two-benefit manifest 007 Step 4 mandates). Blocks 003 Step 3. |
| 6 | **Lapse copy, in the customer's words** | 007 L100–102; 004 Step 1 (L91–98), Step 2 (L114–120); 005 Matrix F | What the customer sees when the window lapses, and whether an out-of-window build locks silently or explains once on first run (007 recommends explain once) | 004's doc/copy step cannot be written; 005 Matrix F consistency check has no target text. |
| 7 | **macOS signing credentials** (Developer ID cert, notarization Apple ID / API key, team ID) | 004 Steps 3–4 (L128–157); 005 Matrix E (E1) | Working CI-accessible signing + notarization secrets on the release runner | No signed artifacts → no draft to smoke-test → no upload to Polar → no E-matrix. Already proven once, so this is custody/availability, not design. |
| 8 | **Clean macOS ARM64, macOS Intel, and Windows 10/11 x64 machines** | 005 L57–59, Matrix C and E; 004 Step 4 (L148–157) | Three genuinely clean environments (not the dev machine), plus network controls that block Polar **without** changing system time | 005 STOP L255 (`A supported clean platform is unavailable`) fires. Matrices C and E cannot run; 005 cannot reach GO. |
| 9 | **Two sandbox email identities + two private browser profiles** | 005 L60–61; 003 Step 4 (L215–218) | Two mailboxes the operator controls for buyer and Teams-member journeys | Teams seat-claim journeys (003 Step 4, 005 Matrix B) cannot be run. |
| 10 | **`ALFRED_RELEASE_DATE` source in the release pipeline** | 007 Step 3 (L156–166) and maintenance L209–213; 004 Step 3 | Decide where the date comes from (release tag, workflow input, freeze step) — explicitly **not** the local clock in a developer checkout | The update window cannot be enforced; 007 cannot complete; 004's acceptance manifest cannot assert it. A wrong value silently grants or denies entitlement and fails no test. |
| 11 | **Refund, cancellation, and support policy** | 003 L101–102, L109; 005 A8, Matrix F; handoff `:29-38` | Refund window, revocation policy, support address and response promise | 003 STOP L300 fires. Checkout terms and Matrix F consistency cannot be completed. Mostly model-independent — can be decided in parallel. |
| 12 | **Device-limit customer explanation** | 003 L108; 005 C3 | The customer-facing wording for the three-device limit | Copy gap only; does not block the sandbox work. Model-independent. |
| 13 | **Teams seat bounds** | 003 L110, L203–204 | Minimum and maximum self-serve seat count for a one-time Teams purchase, and whether adding seats later is a second purchase | 003 Step 4 checkout configuration cannot be finalized. Depends on row 3 resolving first. |
| 14 | **Linux paid-download approval** | 004 L143 | Whether Linux becomes a supported paid download or stays source/best-effort | 004 Step 3's required-artifact list stays provisional. Low impact; model-independent. |

---

## 4. Readiness verdict

### Plan 003 — **Needs operator input first** (and must wait for 007)

Both gates apply; the operator gate is the binding one, because even a
007-corrected 003 cannot proceed past Step 1 without sandbox access and prices.

- Step 1 (manifest + verifier + tests) is **DONE**, but its output shape is
  wrong for the new model — `manifest.ts:10-14`, `secrets.ts:11-15`, and
  `verifier.ts:39` all encode the three-class shape, and `verifier.ts:39`
  would actively **fail** a correctly configured new sandbox.
- Steps 2–8 are 100% operator-gated and 100% model-encoded. 44 drift sites.
- The two recorded sandbox benefit IDs are stale per 007 L171–173.

**Single next action**: obtain authenticated Polar sandbox access and the two
approved prices (operator-list rows 1–2). Do not touch Steps 2–8 until 007's
Step 4 has fixed the product/benefit table — configuring Polar now means
configuring it twice.

### Plan 004 — **Can start after 007 settles**, and its pipeline half is already correct

The most executable of the three. Its drift is 9 sites, mostly copy.

- Steps 1–3 are recorded DONE. Steps 1 and 2 are the model-coupled parts and
  need the new copy (one-time, permanent features, one-year updates, what
  lapsing does and does not do) plus the out-of-window download explanation
  from 007 L79–82.
- Step 3's machinery — version alignment, required artifacts, checksums,
  updater guard — survives untouched. It needs exactly **one** additive
  change: `ALFRED_RELEASE_DATE` in the acceptance manifest.
- Steps 4–6 are blocked on signing credentials, clean machines, and Polar
  sandbox — none of which 007 affects.

**Single next action**: once 007 Step 3 defines where `ALFRED_RELEASE_DATE`
comes from, add it to the CI acceptance manifest spec in 004 Step 3 (L128–134).
That is the one 004 change that is release-critical and currently invisible to
every test.

### Plan 005 — **Needs operator input first**; cannot start meaningfully

- Matrix D is **done, automated, and survives** — one row (D8, L170 /
  TEMPLATE `:171`) must stop treating `expired` as restrictive.
- Matrix A collapses from two runs to one; Matrix B loses rows B6–B8 entirely
  (proration and paid-quantity reduction do not exist on a one-time purchase).
- Matrix C row 5 and Matrix D row 8 carry the `expired` semantic inversion —
  the highest-risk drift in this audit, because getting it wrong revokes
  features a customer already paid for.
- Five new rows are missing entirely (out-of-window build, forever-features on
  the purchased build, exactly-at-deadline, unset release date, source build
  unlocked).
- Everything else waits on 003 configuring Polar and on the clean machines.

**Single next action**: hold. No acceptance run can start before 003 reconfigures
Polar. The one thing worth doing early is the coordinator decision on rewriting
matrices A/B and `docs/release-acceptance/TEMPLATE-polar.md` into the
two-product shape — including the five missing update-window rows — so the
rewrite is ready the moment 007 lands.

---

## 5. Coordinator summary

- **007's blast radius is larger than the roadmap implies.** It reaches
  `scripts/polar/` (5 files), `src-tauri/build.rs`, `.env.example`,
  `docs/polar-operator-handoff.md` (~20 lines), and
  `docs/release-acceptance/TEMPLATE-polar.md` (~40 lines) — not only the three
  plan files.
- **One code line will fail loudly**: `scripts/polar/verifier.ts:39` asserts
  the lifetime key has no expiry; 007 requires every key to have one.
- **One semantic change is dangerous and quiet**: `expired` moving from
  "not licensed" to "entitled, window closed". It is encoded in 005 L152 and
  L170, `view-model.ts:121-125`, `acceptance.rs` (3 sites), and TEMPLATE `:171`.
  Missing any of them takes away a feature a customer paid for — 007's fourth
  STOP condition.
- **Two things are already stale independent of 007**: 003 L169–170 says "two
  checkout links" while the code ships one, and 003 L222–223 defines a Company
  checkout link the handoff marks `NOT USED BY THE APP`.
- **004 is the cheapest plan to bring current** — 9 sites, one of them the
  genuinely important `ALFRED_RELEASE_DATE` manifest field.
- **Rows 1, 2, and 3 of the operator list unblock the most work.** Row 3
  (Polar's one-time + expiring-key + one-time-per-seat behavior) is the one
  that could invalidate the model itself rather than merely delay it.
