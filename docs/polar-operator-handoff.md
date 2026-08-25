# Polar operator handoff

Everything a human operator needs to configure Alfred's Polar **sandbox** and
hand four public values back to the repository, executing
[Plan 003](../plans/release-money/003-configure-polar-commerce.md) Steps 2–8 as
amended by
[Plan 007](../plans/release-money/007-two-product-perpetual-model.md).

Alfred's code is already finished and tested for this. Nothing here requires a
developer; nothing here should ever require pasting a credential into the
repository.

> **Sandbox only.** Do not enable live mode, take a real payment, or publish a
> checkout link until Plan 006 says so.

> **SUPERSEDED 2026-08-25 — read first.** The owner replaced the two-product
> paid model with a **supporter licence**: one one-time product, perks are
> `schedules` + `triggers`, entitlement permanent, and licence keys are issued
> **without expiry**. Any instruction below that mandates a one-year expiry,
> Teams seats, or an update window describes the retired model and awaits the
> Plan 003 re-plan. Where the two conflict, the verifier
> (`bun run verify:polar-sandbox`) is authoritative: it now **fails** a benefit
> configured with an expiry.
>
---

## 0. Approve the commercial policy first

Configuration cannot start until the business owner has decided and written
down each row. **Do not invent a value to unblock yourself** — an unresolved
row is a STOP condition (see the last section).

| Decision | Approved value | Approved by / date |
| --- | --- | --- |
| Default currency | | |
| Alfred License price (one payment, one named user) | | |
| Alfred Teams price (one payment **per seat**) | | |
| Tax display: inclusive or exclusive | | |
| Refund policy | | |
| Is a renewal a fresh purchase, or a discounted upgrade product? | | |
| What the customer is told when their update window lapses | | |
| Three-device limit: exact customer-facing wording | | |
| Support email address | | |
| Support response promise | | |
| Teams minimum self-serve seat count | | |
| Teams maximum self-serve seat count | | |

### What the customer is buying

Both products are **one-time purchases**, and both entitlements are the same
shape:

- paying once unlocks every pro feature **permanently** — nothing a customer
  paid for is ever taken away;
- the purchase includes updates released within **one year** of purchase;
- after that year the installed app keeps working exactly as it did. The
  customer simply stops being entitled to *newer builds'* pro features until
  they renew;
- a refunded, revoked, or disabled license is **different** from a lapsed
  update window, and does end entitlement.

> **How the one-year window is enforced.** Polar's File Downloads benefit is
> perpetual and Alfred has no backend, so the window is not enforced at
> purchase or download time. Alfred compares the release date baked into the
> build against the license key's expiry date. That is why **every** key must
> be issued with a one-year expiry: Polar owns the date, Alfred only reads it.
> An out-of-window customer will still be handed newer files by Polar; the app
> explains this rather than letting them discover it.

---

## 1. Step-by-step checklist

### Step 2 — Organization and account (Plan 003 Step 2)

- [ ] Sign in to Polar and switch to **sandbox**. Confirm the environment badge
      is visible on every screen before creating anything.
- [ ] Create or select Alfred's organization.
- [ ] Enable **seat-based pricing** for the organization (it is a beta feature
      and must be turned on explicitly).
- [ ] Confirm a seat-based product can be sold **one-time per seat**, not only
      as a recurring subscription. If it cannot, stop — that is a STOP
      condition, not something to work around with a subscription.
- [ ] Record privately, **outside this repository**: who owns the account, the
      account-recovery method, payout/account-review requirements, and the
      current fee plan.
- [ ] Test account recovery actually works before going further.
- [ ] Confirm no live product or live payment exists.

### Step 3 — Products and license benefits (Plan 003 Step 3)

Build exactly this. One product, one License Keys benefit, one File Downloads
benefit.

**Products**

| # | Product name | Pricing type | Billing |
| --- | --- | --- | --- |
| 1 | Alfred Supporter | standard one-time | one payment |

**License Keys benefits — one**

| Benefit | Activation limit | Expiration | Attached to |
| --- | --- | --- | --- |
| Alfred Supporter key | **3** | **none** — perpetual | the supporter product |

**File Downloads benefit — one, shared**

| Benefit | Attached to |
| --- | --- |
| Alfred installers | Alfred Supporter |

**Attachment matrix — check every cell**

| Product | Alfred Supporter key | Installers |
| --- | --- | --- |
| Alfred Supporter | ✅ | ✅ |

- [ ] Every license-key benefit uses a **three**-activation limit.
- [ ] The license-key benefit has **no expiration configured**. Supporter
      licences are perpetual; a benefit recorded with an expiry fails the
      verifier.
- [ ] Use a recognizable Alfred key prefix.
- [ ] One product grants exactly one license key. Alfred identifies the
      purchase from the `benefit_id` alone; two keys on one product makes that
      ambiguous.
- [ ] Revoking or refunding a purchase still revokes the key. Revocation is
      the only thing that ends entitlement — there is no expiry window.

### Step 4 — Teams seats and portal ownership (Plan 003 Step 4)

- [ ] Enable seat management in Polar's hosted customer portal.
- [ ] Configure the Teams checkout so the buyer picks a seat quantity inside
      the approved minimum/maximum.
- [ ] Keep Polar's **default hosted confirmation page**. A custom success URL is
      not allowed unless the whole hosted claim flow is re-tested from scratch.
- [ ] Place one sandbox order for **three seats**.
- [ ] Confirm the purchaser becomes the team **owner** and does **not**
      automatically receive benefits.
- [ ] The owner assigns and claims one seat for themselves through Polar's
      hosted flow → two seats remain available.
- [ ] A second email claims another seat and sees its **own** license key and
      downloads.
- [ ] The owner can assign, resend, and revoke seats.
- [ ] Record what buying *more* seats does on a one-time product — a second
      purchase, or a resize. This is undecided in section 0 and must be written
      down before launch, not discovered by a customer.

### Step 5 — Checkout link and portal entry points (Plan 003 Step 5)

- [ ] Create one **Alfred Supporter** checkout link and record it as
      `checkoutLinks.supporter.url`; until then the manifest keeps `null`
      there and the verifier stops at `manifest.checkout.supporter`.
      Leave the success URL on Polar's hosted confirmation page.
- [ ] Do **not** create any other checkout link: a link recorded under a
      retired or invented name (`individual`, `teams`) is rejected by the
      manifest.
- [ ] Enable only approved discount and billing options.
- [ ] Open the link in a **private browser window** and confirm: correct
      product, currency, tax display, terms, and return behavior.
- [ ] Confirm Polar's transactional emails link customers into the hosted
      customer portal. Alfred has no backend and cannot mint portal links.

> **Link shape matters, and it differs per environment.** A **sandbox** build
> can only open a sandbox checkout link
> (`https://sandbox-api.polar.sh/v1/checkout-links/polar_cl_…/redirect`); a
> **production** build can only open `https://buy.polar.sh/polar_cl_…` and
> `https://polar.sh/<org-slug>/portal`. Neither environment accepts the
> other's shapes.
> See "Known constraints" below before recording the values.

### Step 6 — Shared downloads benefit (Plan 003 Step 6)

- [ ] Attach the single File Downloads benefit to **the supporter product**.
- [ ] Upload a small **non-production** fixture file for the sandbox proof.
- [ ] A supporter purchaser can download it from the portal.
- [ ] An unrelated email cannot obtain the file URL or its bytes.

### Step 7 — Prove the activation lifecycle (Plan 003 Step 7)

Fill the manifest first (Step 8 table below), then supply the sandbox
**test** license key and run the verifier. It talks only to Polar's public
customer-portal endpoints and never sends an access token.

Supply the key one of two ways — never as a command-line argument:

```bash
# Preferred: a secret runner injects the values, nothing touches the disk.
op run -- bun run verify:polar-sandbox
```

```bash
# Or: the git-ignored file scripts/polar/sandbox-secrets.json.local
# { "supporter": "…" }
bun run verify:polar-sandbox
```

The verifier prints case names and `PASS`/`FAIL` only. If it stops on a
configuration problem it names the field and how to fix it, never the value.

- [ ] The supporter benefit activates, validates, and deactivates.
- [ ] A fourth activation is rejected, then succeeds after one deactivate.
- [ ] The key validates with `expires_at: null`. Supporter licences are
      perpetual — a benefit configured with an expiry fails the verifier.
- [ ] Refund a purchase in sandbox → its key stops being granted on Polar's
      documented timing.
- [ ] Entitlement is permanent once granted: nothing time-based ever ends it.
      Only revocation or refund does.

### Step 8 — Bind the public values (Plan 003 Step 8)

Fill in the table in section 2, then:

- [ ] Copy `.env.example` to `.env` at the repository root (`.env` is
      git-ignored — this is the one reviewed place these values live).
- [ ] Paste each value into the variable named in the table.
- [ ] Leave `ALFRED_RELEASE_DATE` **blank**. It is not an operator value: the
      release workflow sets it, and a blank value means a source build that
      never locks anything.
- [ ] Rebuild: `bun run build` (or `bun run dev` for a local check). `build.rs`
      bakes the `ALFRED_*` values into the binary and Vite bakes the `VITE_*`
      values into the bundle.
- [ ] Run `bun run check`.
- [ ] Activate the supporter key in the built app, relaunch, refresh, open
      the checkout link and the portal, then deactivate.
- [ ] Run the secret scan (section 4) and confirm it is clean.

---

## 2. The public values

Every value below is **public**. None of them is a credential. Product IDs,
price IDs, access tokens, webhook secrets, and customer IDs are **not needed**
and must not be added.

All of these describe the **sandbox** organization.

| # | Value | Where to find it in Polar | Recorded value | Binds to |
| --- | --- | --- | --- | --- |
| 1 | Organization ID | Settings → General | ⚠️ `e0cc243c-4521-439f-97d4-cc9b0016a554` — provisional, re-verify | `ALFRED_POLAR_ORGANIZATION_ID` in `.env` |
| 2 | Alfred Supporter benefit ID | Benefits → Alfred Supporter key | ✅ `3efa1743-af00-47f4-a85c-cd4bb3c71086` (created 2026-08-25) | `ALFRED_POLAR_INDIVIDUAL_BENEFIT_ID` in `.env` — legacy slot, kept on purpose (see note below) |
| 3 | Alfred Supporter checkout link | Products → Checkout links | 🔴 **NOT COLLECTED YET** — the manifest records `null` until it exists | `VITE_POLAR_INDIVIDUAL_CHECKOUT_URL` in `.env` |
| 4 | Hosted customer-portal URL | Customer portal settings | ⚠️ `https://sandbox.polar.sh/nirzhuk/portal` (verified live: 200) — re-verify the slug | `VITE_POLAR_CUSTOMER_PORTAL_URL` in `.env` |

Plus two build settings that are not Polar values:

| Value | Set it to | Binds to |
| --- | --- | --- |
| Environment | `sandbox` | `ALFRED_POLAR_ENVIRONMENT` in `.env` |
| Release date | **leave blank** — the release workflow sets it | `ALFRED_RELEASE_DATE` in `.env` |

> ### 🔴 Previously bound benefit IDs are STALE
>
> Earlier versions of this document recorded benefit IDs against the retired
> multi-product models:
>
> | Retired row | Recorded value | Status |
> | --- | --- | --- |
> | Desktop annual / later `individual` benefit ID | `69d283e8-fa0d-4d60-a474-5e3fee5cbe71` | ❌ **STALE — do not paste anywhere** |
> | Desktop lifetime benefit ID | `caed58b2-92c9-4859-9f73-258abe849f40` | ❌ **STALE — do not paste anywhere** |
> | `teams` benefit ID | `64d78b24-7e6c-4b6a-9f94-08525c53a157` | ❌ **STALE — do not paste anywhere** |
>
> On 2026-08-25 the owner collapsed the model to a single supporter licence
> and created a fresh benefit, so none of those IDs matches what Polar serves
> today. They are recorded here only so nobody re-binds them by memory.
>
> The live supporter benefit is
> `3efa1743-af00-47f4-a85c-cd4bb3c71086`, bound in
> `scripts/polar/sandbox-manifest.json` as `benefits.supporter.id`.
>
> This binding is still **unproven against Polar**: it is confirmed by
> dashboard inspection only. `bun run verify:polar-sandbox` is what proves
> it, once the checkout link is collected and a sandbox test license key
> exists.
>
> Treat **every** value in the table above as provisional until it has been
> read off the *current* sandbox organization and re-verified. Rows 1 and 4
> may well have survived the reset — confirm them, do not assume them.

`scripts/polar/sandbox-manifest.json` is the committed, reviewed, non-secret
record the verifier checks against. It records the supporter benefit ID and
leaves `checkoutLinks.supporter.url` as `null`. Until that link is filled in,
`bun run verify:polar-sandbox` stops pre-network at
`FAIL manifest.checkout.supporter` with "collect the checkout link from the
Polar dashboard". That is the intended fail-closed state, not a bug. The file
is safe to commit because it contains no credential.

> **The supporter benefit binds through the legacy `individual` slot.**
> `ALFRED_POLAR_INDIVIDUAL_BENEFIT_ID` was deliberately not renamed when the
> model collapsed to one product, so nothing else had to move with it;
> `ALFRED_POLAR_TEAMS_BENEFIT_ID` stays unset-optional. The same mapping
> holds for the verifier's test-key sources. See `scripts/polar/README.md`.

> **Row 4 was confirmed empirically, not guessed.** Polar's hosted portal is
> per-organization: `https://sandbox.polar.sh/<org-slug>/portal` returns 200 and
> redirects to `/portal/request` for the email sign-in code. The organization
> slug (`nirzhuk`) was read from the live checkout session. The previously
> documented `https://polar.sh/purchases` **404s** and appears nowhere in
> Polar's docs; it has been corrected everywhere.

**Expected shapes — sandbox**

| Value | Shape |
| --- | --- |
| Organization / benefit ID (1–2) | UUID v4, e.g. `xxxxxxxx-xxxx-4xxx-8xxx-xxxxxxxxxxxx`, and they must differ. None may be omitted. |
| Alfred Supporter checkout link (3) | `https://sandbox-api.polar.sh/v1/checkout-links/polar_cl_…/redirect`, no query string, no fragment |
| Customer portal (4) | `https://sandbox.polar.sh/<org-slug>/portal` — one slug segment, then `/portal` |

---

## 2a. PRODUCTION values — NOT for sandbox use

> ⛔ **Everything below is a LIVE link.** Do not paste any of it into `.env`,
> into `scripts/polar/sandbox-manifest.json`, or into any sandbox test. A
> sandbox build **rejects** these shapes on purpose, and the sandbox manifest
> rejects them too. They are recorded here only so the value is not lost before
> Plan 006 flips the build to `production`.

| Value | Recorded value | Status |
| --- | --- | --- |
| Alfred License checkout link (production) | `https://buy.polar.sh/polar_cl_tcZ5aEow7B3AYyY07BwOTUdLe1rQDb3URrYx80A5jj0` | ⚠️ predates the product reset — re-verify before Plan 006 |

**Expected shapes — production**

| Value | Shape |
| --- | --- |
| Checkout links | `https://buy.polar.sh/polar_cl_…`, no query string, no fragment |
| Customer portal | `https://polar.sh/<org-slug>/portal` |

A production build accepts **only** the production shapes and a sandbox build
accepts **only** the sandbox shapes. Neither is a superset of the other, so a
sandbox link can never reach a paying customer and a live link can never be
exercised by a sandbox test.

**Behavior when values are missing.** Leaving them all blank is the normal
source-build state: Alfred starts, License & Billing reports itself as
unconfigured, and nothing crashes. Filling in only *some* of them is reported
as an incomplete configuration rather than silently ignored — so a half-pasted
`.env` is caught at startup, not by a customer.

---

## 3. Known constraints to check before recording a value

These are limits in Alfred, not in Polar. Hitting one is a reason to pause and
talk to a developer, not to widen anything yourself.

1. **Accepted link shapes are per environment, keyed on
   `ALFRED_POLAR_ENVIRONMENT`.** One allow-list, in
   `src/features/licensing/public-link-rules.ts`, is followed by the frontend
   opener (`src/features/licensing/public-links.ts`), the manifest parser
   (`scripts/polar/manifest.ts`), and — mirrored by hand, because Tauri
   capabilities are static JSON — the `opener:allow-open-url` scope in
   `src-tauri/capabilities/default.json`. The rules are:

   | Environment | Checkout | Customer portal |
   | --- | --- | --- |
   | `production` | `https://buy.polar.sh/polar_cl_<id>` | `https://polar.sh/<org-slug>/portal` |
   | `sandbox` | `https://sandbox-api.polar.sh/v1/checkout-links/polar_cl_<id>/redirect` | `https://sandbox.polar.sh/<org-slug>/portal` |

   Neither environment accepts the other's shapes, and no other Polar host is
   accepted in either. An unset or unrecognised `ALFRED_POLAR_ENVIRONMENT`
   falls back to `production`, the tighter set. A value outside the rules for
   the build's environment is rejected by the manifest during configuration
   rather than silently refused during the desktop smoke.
2. **The customer portal is per-organization, not a global page.** Both
   environments accept exactly one path segment for the slug followed by
   `/portal`; the slug cannot be walked deeper (`/a/b/portal` and
   `/<slug>/portal/request` are both rejected — the app opens `/portal` and
   the browser follows Polar's redirect to `/portal/request` on its own).
   There is **no** global `/purchases` page — that path 404s on `polar.sh` and
   on `sandbox.polar.sh`, and appears nowhere in Polar's documentation. An
   earlier version of this file claimed otherwise; it was wrong.
3. **No query strings or fragments** in any recorded URL, in either
   environment, and no `user:password@`, no explicit port, and no scheme other
   than `https`. A portal or checkout URL carrying a `customer_session_token`
   — or any query parameter at all — is a credential, not a public link, and
   is rejected.
4. **Alfred reads `benefit_id` only.** One product grants exactly one
   license-key benefit: if more than one benefit ends up attached to it — or
   the manifest's recorded ID drifts from Polar's — validation fails.
   Re-check the attachment matrix.
5. **Alfred treats a licence key as perpetual.** A benefit issued *with* an
   expiry would silently time-box what supporters paid for. The verifier fails
   any recorded expiry rather than accept it.

---

## 4. DO NOT — read this before pasting anything

Never place any of the following into the repository, a plan, a commit message,
a screenshot, an issue, a pull request, or a CI log:

- **a Polar organization access token** — Alfred never needs one, in any file,
  for any reason;
- **a Polar webhook secret** — Alfred has no backend and no webhooks;
- **a license key, test or real** — including in a shell command, where it
  lands in shell history and the process list. The verifier refuses arguments
  for exactly this reason;
- **a customer email address, name, or customer ID**;
- **a signed download URL** — these grant the file to whoever holds them;
- **a customer session token**, including one embedded in a portal URL;
- **a dashboard export, or any screenshot showing customer data**.

Test keys go in a secret runner or in `scripts/polar/sandbox-secrets.json.local`
(git-ignored). Account ownership, recovery details, and fee-plan notes go in the
team's private password manager — not here.

Confirm before every commit:

```bash
rg -n "(POLAR_ACCESS_TOKEN|polar.*secret|POLAR_TEST_.*_KEY)" src src-tauri scripts --glob "!**/*.test.*"
```

Variable **names** in output are fine. A **value** next to one is not — stop and
get help rather than committing.

---

## 5. STOP conditions

Stop configuring, record what you saw, and escalate to the plan owner if any of
these is true. Do not work around one.

1. **Seat-based pricing is unavailable or unsuitable** for the launch account.
   It is a Polar beta feature and may not be enabled for every organization.
2. **A seat-based product cannot be sold one-time per seat** — only as a
   recurring subscription. The approved model has no subscription in it.
3. **A claimed Teams member cannot get their own license key and download
   benefit** — benefits landing only on the billing purchaser breaks the whole
   Teams offer.
4. **The billing purchaser cannot self-claim a seat** through Polar's hosted
   flow.
5. **A License Keys benefit cannot be created without an expiration**, or the
   public validation response stops exposing `expires_at` as nullable. The
   supporter model needs Polar to issue keys that simply never expire; if that
   becomes impossible, the offer must change rather than fake an expiry.
6. **The public license endpoints demand a Polar access token**, or expose any
   privileged operation. Alfred is a desktop app with no backend; it can only
   use endpoints that are safe without a credential.
7. **Revocation or refund behaves differently from what Alfred promises
   customers** — for example a refunded purchase that keeps granting, or a
   *merely expired* key that Polar treats as never purchased.
8. **Customer download files are reachable anonymously** — anyone without a
   purchase can fetch the file URL or its bytes.
9. **Any price, refund, renewal, seat, or support policy in section 0 is still
   unresolved** when you reach the step that needs it.

Additionally, from section 3: if Polar issues a link on a **host or path shape
Alfred does not accept for the build's environment**, stop and raise it.
Widening the allow-list is a reviewed code change to
`src/features/licensing/public-link-rules.ts` and the matching entry in
`src-tauri/capabilities/default.json`, never an edit by the operator, and never
by widening production to also accept sandbox shapes.
