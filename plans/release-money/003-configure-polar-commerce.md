# Plan 003: Configure, bind, and prove Alfred's Polar commerce model

> **Executor instructions**: Complete Plans 001–002 first, and read
> [Plan 007](007-two-product-perpetual-model.md) completely before touching
> Polar — 007 defines the product model this plan configures. This is an
> operator-assisted external configuration and integration task. Use Polar's
> sandbox and current official documentation. Do not create or deploy an Alfred
> backend. Record only non-secret identifiers and sanitized evidence. Pause for
> the operator if authenticated dashboard access or a commercial policy choice
> is unavailable; the preceding code plans remain complete. Stop on any STOP
> condition and update this plan's row in
> `plans/release-money/README.md` when done.
>
> **Drift check (run first)**:
> `git diff --stat ecb94d6..HEAD -- src src-tauri scripts package.json bun.lock plans/release-money docs README.md`
> Reconcile any changed product or distribution decision before configuring
> Polar.

## Status

- **Priority**: P0
- **Effort**: M
- **Risk**: HIGH (external billing, and one-time-per-seat semantics on a beta
  Polar seat model)
- **Depends on**: `archive/001-connect-desktop-polar-licensing.md`,
  `archive/002-build-polar-license-settings.md` (both DONE, archived), and
  [007](007-two-product-perpetual-model.md)
- **Category**: direction, migration, release
- **Planned at**: commit `ecb94d6`, 2026-08-15
- **Rewritten at**: 2026-08-20 against the two-product perpetual model
  ([007](007-two-product-perpetual-model.md)), from the drift map in
  [RECONCILIATION-003-004-005.md](RECONCILIATION-003-004-005.md).
- **Execution**: Step 1 completed on isolated branch
  `codex/003-polar-sandbox` at `2ef8cdb`. Operator-readiness work followed on
  `polar/003-operator-ready`: the verifier reads keys from a secret runner or
  the ignored local file and refuses arguments, configuration failures name the
  field and the fix, the manifest accepts only link shapes the desktop build is
  allowed to open, the public values bind from the single reviewed `.env`
  through `src-tauri/build.rs` and Vite, and
  [`docs/polar-operator-handoff.md`](../../docs/polar-operator-handoff.md)
  walks Steps 2–7. **Step 1's output shape is now wrong for the two-product
  model** and is re-opened by 007 Step 4: `scripts/polar/manifest.ts`,
  `scripts/polar/secrets.ts`, and `scripts/polar/verifier.ts` still encode
  three benefit classes, and `verifier.ts` still asserts a key with no expiry.
  Steps 2–8 remain blocked pending approved commercial policy and
  authenticated Polar sandbox access.

## Why this matters

Polar can replace Alfred's custom checkout, tax, customer-account, email,
license, seat, portal, and download services only if the sandbox proves the
exact product model — which is now the two-product perpetual model in Plan 007.
Two properties of that model are **unproven against Polar** and are release
gates, not assumptions:

1. a **one-time** product can issue a license key carrying a **one-year
   expiry** (007 Step 4 depends on Polar owning that date);
2. a **seat-based** product can be sold **one-time per seat** rather than
   recurring. Polar's seat-based pricing is beta, so its member benefit
   behavior on a one-time purchase must be observed, not assumed.

If either is false, 007's STOP conditions fire and the offer changes before
this plan continues.

## Current state

- Alfred's public source remains GPL-3.0-or-later and independently usable.
- The release workflow already stages private GitHub draft installers; macOS
  signing/notarization has passed and Windows is an accepted unsigned beta.
- No Polar production catalog or approved public identifier mapping is
  recorded for the desktop.
- The operator has reset the Polar products, so any sandbox benefit ID
  currently recorded anywhere in this repository is **stale and provisional**
  (007 Step 4). Re-verify every ID against the new products.
- Plans 001–002 provide tested injectable configuration seams, a direct Polar
  license client, safe local state, and License & Billing UI. They use local
  fixtures until this plan supplies the real sandbox public values.
- A custom implementation of the abandoned commercial gateway exists under the
  workspace's `api-licenses/` directory, but it is abandoned by this roadmap.
  Do not deploy, extend, copy, or delete it in this plan.
- Polar documents public desktop-safe activate, validate, and deactivate
  license-key endpoints that do not require an access token.
- Polar's hosted customer portal handles email-code authentication, receipts,
  license keys, file downloads, activations, and seat management.
- The app ships **one** in-app checkout link (individual). Alfred Teams is sold
  on the marketing website and has no in-app checkout entry point; this already
  matches `scripts/polar/manifest.ts`, which exposes a single link.

Re-check these primary references at execution time:

- <https://polar.sh/docs/features/checkout/links>
- <https://polar.sh/docs/features/benefits/license-keys>
- <https://polar.sh/docs/features/benefits/file-downloads>
- <https://polar.sh/docs/features/seat-based-pricing>
- <https://polar.sh/docs/features/customer-portal/introduction>
- <https://polar.sh/docs/api-reference/customer-portal/license-keys/activate>
- <https://polar.sh/docs/api-reference/customer-portal/license-keys/validate>
- <https://polar.sh/docs/api-reference/customer-portal/license-keys/deactivate>

## Approved Polar resource model

Two products. Both are **one-time** purchases. Both grant pro features
permanently plus **one year of updates**.

| Alfred offer | Polar product | Pricing | Benefits |
| --- | --- | --- | --- |
| **Alfred License** | standard one-time | one payment, one named user, not seat-based | Individual license key + downloads |
| **Alfred Teams** | seat-based, one-time per claimed seat | one payment per seat | Teams license key + downloads per claimed seat |

Create **two** separate License Keys benefits so Alfred can identify the safe
product class from `benefit_id`:

- **individual** — granted by Alfred License;
- **teams** — granted by Alfred Teams, to each claimed member.

Every key benefit uses a **three-activation limit**, and every key is issued
with a **one-year expiry** so Polar owns the update deadline that 007's
in-window rule compares against. Create one shared File Downloads benefit for
official installers and attach it to **both** products. For the seat-based
product, both benefits must be granted to each claimed member, not only to the
billing customer.

There is no third product. A renewal is a **fresh purchase of the same
product**, which is what keeps the two-benefit manifest in 007 Step 4 valid.

> **Coordinator default, pending owner confirmation** — renewal as a fresh
> purchase (rather than a discounted upgrade product) is the coordinator's
> working assumption so this plan can be written. If the owner instead approves
> a distinct renewal product, the two-benefit manifest and this table both
> change, and 007 Step 4 must be reopened before configuration starts.

### Environment surface

Exactly these values reach the build, per 007 Step 3 and Step 4:

| Variable | Meaning |
| --- | --- |
| `ALFRED_POLAR_INDIVIDUAL_BENEFIT_ID` | Alfred License key benefit |
| `ALFRED_POLAR_TEAMS_BENEFIT_ID` | Alfred Teams key benefit |
| `ALFRED_RELEASE_DATE` | ISO `YYYY-MM-DD`, supplied by the release workflow |

`ALFRED_RELEASE_DATE` is supplied by the release workflow as ISO
`YYYY-MM-DD`. **Unset means "source build" and must never lock anything.** It
is never read from the local clock in a developer checkout.

## Decisions that must be approved before configuration

Each item below is blocking. Nothing in Steps 2–8 may proceed on a guess; where
this plan needs a value it does not have, it carries an explicit
**OPERATOR INPUT REQUIRED** placeholder rather than an invented one.

- **Default currency**: `USD` — confirmed by the owner 2026-08-20.
- **Alfred License price (one payment)**: `9.99 USD` — confirmed 2026-08-20.
- **Alfred Teams price (one payment per seat)**: `9.99 USD` — confirmed 2026-08-20.
- **Tax display**: prices are shown **tax-inclusive** — confirmed 2026-08-20.
- refund and cancellation policy;
- **confirmation that Polar can issue a one-time product's license key with a
  one-year expiry** — this is the mechanism 007 depends on;
- **confirmation that a seat-based product can be sold one-time per seat**
  rather than recurring;
- confirmation that the shared File Downloads benefit stays **perpetual** on
  both products. It does, and that is correct: Polar keeps serving downloads
  after the window lapses, and the app enforces the window client-side by
  comparing `ALFRED_RELEASE_DATE` against the key's deadline. An out-of-window
  customer can still download a newer build; it simply runs without pro
  features, and the app must say so rather than let them discover it;
- three-device limit and customer-facing explanation;
- support address and response promise;
- Teams minimum and maximum self-serve seat count, and whether adding seats
  later is a second purchase;
- the customer-facing lapse copy (see Plan 004 Step 1), which the owner must
  approve before any checkout description is published.

Do not copy another product's prices.

## Commands you will need

| Purpose | Command | Expected on success |
| --- | --- | --- |
| Full repository check | `bun run check` | frontend tests/build and Rust tests pass |
| Verifier tests | `bun test scripts/polar` | manifest/parser/redaction/mock tests pass; the manifest **rejects** a three-benefit shape |
| Sandbox verifier | `bun run verify:polar-sandbox` | both configured benefit classes activate, validate, and deactivate; no secret output |
| Release hygiene | `bun run verify:release-hygiene` | all-PASS |
| Secret scan | `rg -n '(POLAR_ACCESS_TOKEN\|polar.*secret\|POLAR_TEST_.*_KEY)' src src-tauri scripts --glob '!**/*.test.*'` | environment-variable names only; no values or fixtures resembling real keys |

The sandbox verifier reads test license keys from ignored local environment or
the operator's secret runner. Never place a key directly in a shell command,
plan, screenshot, CI log, or committed fixture.

## Scope

**In scope**:

- Polar sandbox organization, products, prices, benefits, checkout link, and
  customer-portal settings;
- **two** sandbox purchase/benefit journeys (Alfred License, Alfred Teams);
- a private operator handoff containing the public Polar organization ID, the
  **two** benefit IDs, the individual checkout-link URL, and the portal URL;
- binding those public sandbox values to Alfred's reviewed build
  configuration and running an end-to-end desktop smoke;
- the public configuration files/seams produced under
  `src-tauri/src/licensing/**` and `src/features/licensing/**` by Plans 001–002;
- `scripts/polar/**` and a package script for a redacting, public-endpoint-only
  sandbox verifier;
- focused verifier tests and package/lock changes only if required;
- sanitized completion evidence in this plan and its index row.

**Out of scope**:

- Polar production/live mode or real payments;
- any Polar organization access token in Alfred or documentation;
- custom webhooks, API, database, accounts, email, or portal;
- desktop behavior changes beyond binding the public configuration seams from
  Plans 001–002;
- which features are "pro" and how they are gated — that is
  [Plan 008](008-pro-entitlement-and-source-freedom.md);
- an in-app Teams checkout entry point — Teams is sold on the marketing
  website;
- release artifact upload (Plan 004);
- deleting the abandoned commercial-gateway implementation.

## Git workflow

- Branch: `codex/003-polar-sandbox`.
- Commit the verifier/config seam separately from operator evidence.
- Do not commit dashboard exports, test keys, customer data, signed download
  URLs, screenshots containing PII, or any Polar access token.
- Do not push, enable live mode, or publish checkout links unless instructed.

## Steps

### Step 1: Add the safe sandbox manifest and verifier

Create a typed, non-secret manifest for the expected organization ID, the
**two** benefit IDs, the **one** individual checkout link, the portal URL, and
product/benefit labels. Add a `verify:polar-sandbox` script that reads the
**two** test license keys only from ignored environment/secret input, calls the
public activate/validate/deactivate endpoints, checks benefit allow-listing and
three-activation behavior, and prints case names plus pass/fail only.

The manifest must **reject** a three-benefit shape, so a stale configuration
fails loudly instead of half-binding. The verifier must not assert that any key
lacks an expiry: under the new model **every** key carries a one-year expiry,
and a key with no expiry is the failure case.

Unit-test the verifier through a local mock. It must redact request/response
bodies, never send `Authorization`, and clean up activations in `finally`.

**Verify**: `bun test scripts/polar` passes without network or Polar access; a
three-benefit manifest is rejected; a key with an expiry is accepted.

### Step 2: Create the sandbox organization and approve policy

Create or select Alfred's Polar organization in sandbox. Enable seat-based
pricing. Record the organization owner, recovery method, payout/account-review
requirements, and current fee plan privately. Approve every pricing/support
decision above — including both **OPERATOR INPUT REQUIRED** prices — before
making a public checkout link.

**Verify**: the operator can sign in with recovery tested; sandbox is visibly
selected; no live product or payment is created.

### Step 3: Create products and license benefits

Create the **two** products and **two** License Keys benefits exactly as
specified. Use recognizable Alfred prefixes and three activations. Issue every
key with a **one-year expiry** so the update deadline comes from Polar rather
than from Alfred's arithmetic. Attach the correct license benefit to each
product.

Before creating anything, prove the two open Polar questions in the sandbox:
that a one-time product can carry an expiring license key, and that the
seat-based product can be sold one-time per seat. If either fails, STOP — 007's
offer must change rather than this plan adapting around the gap.

**Verify**: dashboard inspection shows **two** products, **two** distinct
benefit IDs, and the exact attachment matrix; no product grants two license
keys; every issued key shows a one-year expiry date.

### Step 4: Configure Teams seats and portal ownership

Enable seat management in Polar's hosted portal. Configure Teams checkout so
the buyer can purchase a bounded quantity **as a one-time payment per seat**
and receives a member/owner record. Use Polar's default confirmation page for
the first release. Current Polar documentation says the purchaser becomes the
team owner but does not receive benefits automatically; the owner must assign
and claim one purchased seat for themselves through Polar's hosted flow. A
custom success URL is not allowed unless the full hosted claim flow is
re-tested.

Confirm that a billing owner can assign, resend, and revoke seats, and that
every claimed member — not merely the purchaser — receives the Teams key and
downloads benefits.

Seat **resize** has no defined meaning on a one-time purchase. Record what
Polar actually does when the owner adds seats: under the coordinator default
above, adding seats is a **second purchase of the same product**, not a
proration. Confirm this in sandbox rather than assuming it.

- **OPERATOR INPUT REQUIRED — Teams minimum self-serve seat count**: `<count>`
- **OPERATOR INPUT REQUIRED — Teams maximum self-serve seat count**: `<count>`

**Verify**: one three-seat sandbox order creates the purchaser as owner without
granting benefits prematurely; the owner can assign and claim one seat for
themselves, leaving two available seats. A second email can claim another seat
and see its own benefits. Adding a fourth seat behaves as a recorded, observed
transaction, not an assumed one.

### Step 5: Create the checkout link and hosted portal entry points

Create **one** checkout link for **Alfred License**. That is the only checkout
destination the app opens. Alfred Teams is sold on the marketing website and
gets no in-app checkout entry point; do not add one, and do not bind a second
link into the manifest.

Leave the success URL on Polar's hosted confirmation page. Enable only approved
discount and billing options. Confirm Polar's transactional email links reach
the customer portal, so Alfred does not need a pre-authenticated portal-link
backend.

- **Alfred License sandbox checkout link URL**:
  `https://sandbox-api.polar.sh/v1/checkout-links/polar_cl_VlQMEEhbMnyjMsDRJqRcbWaCA2FhibeBAWuQd2eQUQ2/redirect`
  — bound in `scripts/polar/sandbox-manifest.json`.
- **Hosted customer-portal URL**: `https://sandbox.polar.sh/nirzhuk/portal`
  — bound in `scripts/polar/sandbox-manifest.json`.

**Verify**: opening the persistent checkout link in a private browser creates a
new sandbox checkout with the correct single product, currency, tax display,
terms, and return behavior. There is no annual/lifetime toggle to test — the
link sells exactly one product.

### Step 6: Configure the shared downloads benefit

Create one File Downloads benefit and attach it to **both** products. Upload a
small non-production fixture for sandbox proof. Confirm standard purchasers and
claimed Teams members receive personal signed download access, while an
unclaimed invite and an unrelated email do not.

The benefit is **perpetual by design**, and the update window is enforced
client-side. Record explicitly that an out-of-window customer can still
download newer files from Polar — that is expected, and Plan 004 Step 2 owns
explaining it to the customer.

**Verify**: authorized portal downloads succeed; unauthorized access cannot
obtain the file URL or bytes.

### Step 7: Prove license activation lifecycle without credentials

Run `bun run verify:polar-sandbox` against the **two** test key-benefit types.
Use temporary device labels/activation IDs. Verify a fourth activation fails
after three active allocations and succeeds after one is deactivated.

Then prove the entitlement transitions that actually exist under the new model:

1. **Expiry**: let (or set) a sandbox key pass its expiry. Confirm Polar
   reports it as expired and that Alfred treats it as **entitled, update window
   closed** — not as a loss of access. This is the single most dangerous
   semantic in the plan set.
2. **Revocation**: revoke a Teams member in sandbox. Confirm their key becomes
   non-granted according to Polar's documented timing, and that they lose
   entitlement immediately.
3. **Refund**: confirm a refunded purchase produces the approved key and
   download transition.

There is no subscription to cancel, and no "lifetime key remains granted" case:
every key now carries an expiry, and expiry does not remove entitlement.

**Verify**: every request succeeds without `Authorization` or an organization
access token; sanitized evidence records only status, benefit class, and test
case result — not full keys, emails, URLs, or customer payloads.

### Step 8: Bind the public configuration and run desktop integration smoke

Bind these public values to the configuration seams created by Plans 001–002
through repository/release variables or a reviewed public configuration file:

- Polar organization ID;
- `ALFRED_POLAR_INDIVIDUAL_BENEFIT_ID` and `ALFRED_POLAR_TEAMS_BENEFIT_ID`;
- the Alfred License checkout-link URL;
- hosted customer-portal URL;
- `ALFRED_RELEASE_DATE`, supplied by the release workflow as ISO
  `YYYY-MM-DD`. Leave it unset for a source build; an unset value must never
  lock anything.

Product IDs, price IDs, access tokens, webhook secrets, and customer IDs are
not needed in Alfred. Build a sandbox-configured Alfred, activate each benefit
class, relaunch, refresh, open the checkout link and the portal, then
deactivate. Keep macOS/Windows packaged breadth for Plan 005; this plan needs
one available desktop target.

**Verify**: a reviewer maps each of the two benefit IDs to exactly one Alfred
product class; both activation classes and the configured links work on one
desktop; `ALFRED_RELEASE_DATE` is baked in and visible to the in-window rule;
`bun run check` passes; secret scanning shows no Polar access token.

## Done criteria

- [ ] Pricing, tax, refund, update-window, device, Teams-seat, and support policy are approved, with no `OPERATOR INPUT REQUIRED` placeholder left in this plan.
- [ ] The redacting sandbox verifier and its local mock tests pass, and the manifest rejects a three-benefit shape.
- [ ] **Two** sandbox products and the two-key/one-download benefit model exist.
- [ ] Every issued license key carries a one-year expiry.
- [ ] Polar is proven to support a one-time product with an expiring key, and a one-time-per-seat seat product.
- [ ] Teams purchase, buyer self-assignment/claim, member claim, and seat revoke pass; adding seats behaves as an observed, recorded transaction.
- [ ] The Alfred License checkout link shows correct live sandbox data; no in-app Teams checkout entry point exists.
- [ ] Authorized customers and claimed members receive hosted downloads.
- [ ] Public activate/validate/deactivate and the three-device limit pass.
- [ ] An expired key keeps entitlement; revoke and refund remove it.
- [ ] The public configuration handoff contains no credential.
- [ ] A sandbox-configured Alfred completes activation, refresh, link opening, and deactivation, with `ALFRED_RELEASE_DATE` bound.
- [ ] `bun run check`, `bun run verify:release-hygiene`, and the exact secret scan pass.
- [ ] The roadmap row is `DONE`.

## STOP conditions

- Polar cannot issue a **one-time** product's license key with a **one-year
  expiry**.
- Polar's seat-based product cannot be sold **one-time per seat**.
- Polar seat-based pricing is unavailable or unsuitable for the launch account.
- A claimed Teams member cannot receive an independent license key and download
  benefit.
- The billing purchaser cannot self-claim a seat through the chosen hosted flow.
- Public key endpoints require a Polar access token or expose a privileged
  operation.
- Polar's expiry or revocation semantics contradict Alfred's published promise
  — in particular, if an expired key cannot be distinguished from a revoked
  one, because that would take away features a customer already paid for.
- Customer download files are anonymously accessible.
- Final price, refund, update-window, seat, or support policy is unresolved.
- A third product is proposed (for example a separate renewal SKU) without
  first reopening 007 Step 4, because it breaks the two-benefit manifest.

## Maintenance notes

Polar's seat model is beta. Re-run the Teams proof before changing portal
settings, confirmation URLs, benefits, or product pricing type. Keep the
private identifier handoff current, but never turn it into a credential file.

Every release must set `ALFRED_RELEASE_DATE` correctly. A wrong date silently
grants or denies entitlement to real customers and will not fail any test —
Plan 004 Step 3 asserts it in the acceptance manifest for exactly that reason.
