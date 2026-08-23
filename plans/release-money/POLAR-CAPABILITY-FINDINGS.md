# Polar capability findings for Plan 007

> **Scope**: answers the two STOP conditions in
> `plans/release-money/007-two-product-perpetual-model.md` from Polar's current
> public documentation, OpenAPI schema, and open-source server code, before the
> operator logs into the sandbox.
>
> **Researched at**: 2026-08-20, against `docs.polar.sh` (now redirecting to
> `polar.sh/docs`), the `2026-04` OpenAPI version, and `polarsource/polar@main`.
>
> **Method**: every claim below is backed by a URL and quoted text. Where the
> published prose was silent, the OpenAPI schema and the Apache-2.0 server
> source were used, and that is stated explicitly.

---

## Read this first

**Neither of Plan 007's two named STOP conditions is triggered. The product
model is buildable.**

**But a third problem, not on the plan's STOP list, breaks Step 2 as written.**

> Polar's customer-portal validate endpoint returns **HTTP 404 for an expired
> key and HTTP 404 for a revoked or disabled key**. The only difference on the
> wire is the English `detail` string. There is **no `expired` status value** —
> `LicenseKeyStatus` is `granted | revoked | disabled` and nothing else.
>
> Plan 007 Step 2 requires Alfred to distinguish "expired → entitled, window
> closed" from "revoked/disabled → not entitled". **Polar's public endpoint does
> not give Alfred a stable machine-readable way to do that.**

This does not change the offer. It changes the enforcement point: the update
window has to be decided from a deadline Alfred **already holds locally**, not
from the live response at the moment the key has lapsed. See
[The expiry-detection problem](#the-expiry-detection-problem) for the concrete
consequence and the two workable designs.

There is also a live bug in the shipped client that this research surfaced —
see [What this means for the current Alfred code](#what-this-means-for-the-current-alfred-code).

---

## Verdicts

| STOP condition | Verdict |
| --- | --- |
| (a) "Polar cannot issue a one-time product with an expiring license key." | **SUPPORTED** — not triggered. Expiry is configured per license-key benefit and, per Polar's source, is applied *only* to non-subscription grants. |
| (b) "Polar's seat-based product cannot be sold one-time per seat." | **SUPPORTED (beta)** — not triggered. Polar documents One-time + Seat-based as an explicit product configuration. Seat-based pricing is still in beta and must be switched on per organization. |
| *(new, not on the plan's list)* Alfred can tell `expired` apart from `revoked`/`disabled` on the wire. | **NOT SUPPORTED** — both are an indistinguishable `404 ResourceNotFound`. Requires a design change in Step 2. |

---

## 1. Can a ONE-TIME (non-subscription) Polar product grant a License Key benefit?

**Yes. UNCLEAR from prose, CONFIRMED from source.**

The published page is deliberately vague — it covers both cases in one sentence
without naming either:

> "Once customers buy your product or subscribes to your tier, they will
> automatically receive a unique license key. It's easily accessible to them
> under their purchases page."
> — <https://polar.sh/docs/features/benefits/license-keys>

The product page confirms benefits are attached to products generally, with no
recurrence restriction:

> "Benefits are what your customers actually get when they buy: license keys,
> Discord roles, GitHub repository access, file downloads, feature flags, or a
> custom benefit you wire up yourself. Polar grants and revokes benefits
> automatically as customers purchase, renew, or cancel."
> — <https://polar.sh/docs/features/products>

And the refund page names license keys as a one-time-purchase benefit outright:

> "**One-time purchases.** You can revoke the customer's access to product
> benefits — e.g. file downloads, license keys, or Discord/GitHub invites."
> — <https://polar.sh/docs/features/refunds>

The server code is unambiguous: the license-key grant path takes an optional
subscription scope and works without one.

- <https://github.com/polarsource/polar/blob/main/server/polar/benefit/strategies/license_keys/service.py>

---

## 2. Can that license key be issued with an EXPIRY, and is the expiry configurable per benefit?

**Yes to both. CONFIRMED. And — important — the expiry is applied *only* to
one-time purchases.**

### The published capability

> "* Brandable prefixes, e.g `POLAR_*****`
> * **Automatic expiration after `N` days, months or years**
> * Limited number of user activations, e.g devices
> * Custom validation conditions
> * Usage quotas per license key
> * Automatic revokation upon cancelled subscriptions"
> — <https://polar.sh/docs/features/benefits/license-keys>

> "### Automatic Expiration
> Want license keys to expire automatically after a certain time period from
> when the customer bought them? No problem."
> — same page

### The exact field, per benefit

`BenefitLicenseKeyExpirationProperties` in the `2026-04` OpenAPI document
(<https://docs.polar.sh/openapi/2026-04.openapi.json>):

```json
{
  "properties": {
    "ttl":       { "type": "integer", "exclusiveMinimum": 0.0 },
    "timeframe": { "type": "string", "enum": ["year", "month", "day"] }
  },
  "required": ["ttl", "timeframe"]
}
```

It hangs off `expires` on `BenefitLicenseKeysCreateProperties` /
`BenefitLicenseKeysProperties`, which are **per-benefit**. A one-year window is
literally `{"ttl": 1, "timeframe": "year"}`.

**Consequence for Plan 007**: Alfred License and Alfred Teams can each own a
separate license-key benefit with an independent expiry. Plan 007 Step 4's
two-benefit-ID shape is compatible with this.

### Expiry applies only to one-time products — verify this, it is load-bearing

`server/polar/benefit/strategies/license_keys/service.py`:

```python
license_key = await license_key_service.customer_grant(
    self.session,
    customer=customer,
    benefit=benefit,
    ...
    # Subscription-backed keys never expire; they follow the subscription.
    set_expiration=scope.get("subscription_id") is None,
    regrant=not update,
)
```

The inline comment is Polar's, not ours. A **subscription** grant gets **no**
`expires_at`; a **one-time** grant gets the benefit's configured TTL. This is
exactly the shape Plan 007 needs, and it means the one-time decision and the
expiry decision are not independent — you cannot get an expiring key from a
subscription product.

### The date is computed at GRANT time, not purchase time

`server/polar/license_key/schemas.py`:

```python
@classmethod
def generate_expiration_dt(cls, ttl, timeframe) -> datetime:
    now = utc_now()
    match timeframe:
        case "year":  return now + relativedelta(years=ttl)
        case "month": return now + relativedelta(months=ttl)
        case _:       return now + relativedelta(days=ttl)
```

For a single-user purchase, grant time ≈ purchase time, so the docs' phrasing
("from when the customer bought them") holds. **For a seat, grant happens when
the seat is *claimed*.** A Teams seat claimed three months after purchase gets a
deadline one year from *claim*. That is generous rather than harmful, but the
operator should know it, and the plan should not assume a single shared
organization-wide deadline.

### The field in the validate response

`expires_at`, an RFC-3339 timestamp, nullable, and **required** in the
`ValidatedLicenseKey` schema (so it is always present, possibly `null`):

```json
{
  "id": "508176f7-065a-4b5d-b524-4e9c8a11ed63",
  "organization_id": "fda84e25-7b55-4d67-916d-60ead04ff61f",
  "benefit_id": "32a8eda4-56cf-4a94-8228-792d324a519e",
  "key": "1C285B2D-6CE6-4BC7-B8BE-ADB6A7E304DA",
  "display_key": "****-E304DA",
  "status": "granted",
  "limit_activations": 3,
  "usage": 15,
  "limit_usage": 100,
  "validations": 5,
  "last_validated_at": "2024-09-02T13:57:00.977363Z",
  "expires_at": "2026-08-30T08:40:34.769148Z",
  "activation": { "...": "..." }
}
```
— <https://polar.sh/docs/features/benefits/license-keys>, "Validate License Keys → Response (200 OK)"

The `activate` response carries the same `expires_at` nested under
`license_key`. **`expires_at` is the field that carries Plan 007's
`licenseUpdateDeadline`.** Alfred already parses it
(`src-tauri/src/licensing/client.rs:30`).

---

## 3. Can a seat-based product be sold ONE-TIME per seat?

**Yes. CONFIRMED in prose, with a beta caveat.**

Polar documents this as a first-class configuration, not a workaround:

> "**Seat-based pricing is ideal for:**
> * Team subscriptions where one billing manager pays for multiple users
> * **Perpetual team licenses with one-time payment**
> * Organizational licenses with per-seat pricing
> * Products with flat, graduated, or volume-discounted seat pricing"
> — <https://polar.sh/docs/features/seat-based-pricing>

The product form explicitly crosses the two axes:

> "**Select seat-based pricing** — Under **Pricing**, select:
> * **Product type**: Subscription or One-time
> * **Billing cycle** (subscriptions only): Monthly or Yearly
> * **Pricing type**: Seat-based"
> — same page

And there is a comparison table:

| Feature | Subscriptions | One-Time Purchases |
| --- | --- | --- |
| **Payment** | Recurring (monthly/yearly) | Single payment |
| **Seat Duration** | Active while subscribed | Perpetual (never expire) |
| **Adding Seats** | Modify subscription | Purchase new order |
| **Benefits** | While subscription active | Forever after claim |

> "Use **subscriptions** for ongoing team access. Use **one-time purchases** for
> perpetual team licenses."
> — same page

### Beta gate — an operator action item

The marketing page still gates it:

> "Seat-based pricing is in beta today; enable it under Settings → General →
> Features."
> — <https://polar.sh/features/seats>

The docs page carries no beta banner (compare the Shared Slack Channel benefit,
which does say "currently in preview"), so the two surfaces disagree. Treat the
feature flag as required until the operator sees the option in the dashboard.

### What happens to a claimed seat's license key benefit under a one-time purchase

This is the subtlest point in the whole document, and it is **good news for
Plan 007** — but only because two different things both use the word
"perpetual".

- **The seat grant is perpetual.** "Seat Duration: Perpetual (never expire)",
  "Benefits: Forever after claim". Polar never auto-revokes the benefit grant on
  a one-time seat.
- **The license key issued by that grant still carries its own `expires_at`**,
  because the grant path sets expiration whenever there is no `subscription_id`
  — and a one-time seat purchase is an order, not a subscription.

So the seat holder keeps the benefit forever *and* holds a key that lapses after
the configured TTL. **That is precisely Plan 007's model: entitlement is
permanent, the key expiry marks the update window.** Confirm it in the sandbox
anyway (click path C) — it is inferred from source, and it is the single
assumption that Teams depends on.

Grants are per member, not per billing customer:

> "Because benefits are granted to members, not to the billing customer, always
> identify the end user by their member — not the customer who paid."
> — <https://polar.sh/docs/features/seat-based-pricing>

A revoked seat *does* remove the benefit, and the key's status becomes
`revoked` (`customer_revoke` → `key.mark_revoked()` in
`server/polar/license_key/service.py`). That maps cleanly onto Alfred's
`Revoked` and correctly ends entitlement.

### Documented seat-based limitations that affect Alfred Teams

> "* Seats must be assigned individually (no bulk import via dashboard, use API instead)
> * Claim links expire after 24 hours
> * **Billing manager does not receive product benefits**
> * Maximum of 1,000 seats per subscription"
> — <https://polar.sh/docs/features/seat-based-pricing>

The third one bites. The buyer gets nothing unless they hold a seat:

> "When using the default Polar confirmation page (no custom `success_url`), the
> buyer's seat is automatically claimed during checkout... If you set a custom
> `success_url`, the buyer will need to manually assign themselves a seat
> through the Customer Portal or API if they also want benefits."
> — same page

**If Alfred's checkout sets a custom `success_url`, a solo buyer of Alfred Teams
gets no license key and no downloads until they manually assign themselves a
seat.** Check what Plan 003's checkout link does before launch.

---

## 4. Is the File Downloads benefit perpetual for a one-time purchase, and does it survive license-key expiry?

**Yes to both. CONFIRMED — the two benefits are fully independent.**

File Downloads has **no expiry configuration at all**. The documented options
are files, filenames, ordering, SHA-256 checksums, and enable/disable:

> "* Up to 10GB per file
> * Upload any type of file - from ebooks to full-fledged applications
> * SHA-256 checksum validation throughout for you and your customers (if desired)
> * Customers get a signed & personal downloadable URL"
> — <https://polar.sh/docs/features/benefits/file-downloads>

There is no TTL field on the downloadables benefit properties in the `2026-04`
OpenAPI document, and no `expires_at` on the benefit grant.

Access ends in exactly three merchant-driven ways, none of them time-based:

1. **Refund with benefit revocation** — "You can revoke the customer's access to
   product benefits — e.g. file downloads... This is selected by default"
   (<https://polar.sh/docs/features/refunds>).
2. **Deleting a file** — "**Active subscribers & customers will lose access
   too!**" (file-downloads page).
3. **Disabling a file** — "You can disable files at any point to prevent new
   customers getting access to it."

A license key expiring does not touch the downloadables grant. They are separate
benefits with separate grants.

**This confirms Plan 007's core premise verbatim**: *"Polar's File Downloads
benefit is perpetual and cannot express a one-year window... So the window
cannot be enforced at purchase or download time."* Correct. Polar will hand an
out-of-window customer the new files, exactly as the plan predicts. The plan's
warning that the app must explain this on first run is the right call.

---

## 5. What does validate return for an EXPIRED key?

**A `404`, indistinguishable from revoked/disabled except by an English string.
This is the finding that changes Plan 007.**

### The endpoint

`POST /v1/customer-portal/license-keys/validate` — no authentication:

> "This endpoint doesn't require authentication and can be safely used on a
> public client, like a desktop application or a mobile app. If you plan to
> validate a license key on a server, use the `/v1/license-keys/validate`
> endpoint instead."
> — `2026-04` OpenAPI, operation description

Rate limit, worth noting for Alfred's retry policy:

> "Unauthenticated validation, activation, and deactivation endpoints are
> limited to **3 requests per second** in both environments."
> — <https://polar.sh/docs/api-reference/2026-04/introduction>

### The only documented responses

From the `2026-04` OpenAPI document, this operation declares exactly three:

| Code | Schema | Description |
| --- | --- | --- |
| `200` | `ValidatedLicenseKey` | Successful Response |
| `404` | `ResourceNotFound` | **License key not found.** |
| `422` | `HTTPValidationError` | Validation Error |

There is no `403`, no `410`, and **no status code that means "expired"**.

### There is no `expired` status value

```json
"LicenseKeyStatus": { "type": "string", "enum": ["granted", "revoked", "disabled"] }
```

`ValidatedLicenseKey.status` is a `$ref` to that enum. **Expiry is never
expressed as a status** — only as the `expires_at` timestamp, and only on a
`200`.

### The server never returns 200 for an expired key

`server/polar/license_key/service.py`, `validate()`:

```python
if not license_key.is_active():
    bound_logger.info("license_key.validate.invalid_status")
    raise ResourceNotFound("License key is no longer active.")

if license_key.expires_at and utc_now() >= license_key.expires_at:
    bound_logger.info("license_key.validate.invalid_ttl")
    raise ResourceNotFound("License key has expired.")
```

with `is_active()` in `server/polar/models/license_key.py`:

```python
def is_active(self) -> bool:
    return self.status == LicenseKeyStatus.granted
```

and `ResourceNotFound` in `server/polar/exceptions.py`:

```python
class ResourceNotFound(PolarError):
    def __init__(self, message: str = "Not found", status_code: int = 404) -> None:
```

### The exact wire shape

`ResourceNotFound` serializes as `{ "error": <const>, "detail": <string> }`.
Both cases are byte-identical apart from `detail`:

| Case | Status | Body |
| --- | --- | --- |
| Expired key | `404` | `{"error": "ResourceNotFound", "detail": "License key has expired."}` |
| Revoked / disabled key | `404` | `{"error": "ResourceNotFound", "detail": "License key is no longer active."}` |
| Wrong `benefit_id` | `404` | `{"error": "ResourceNotFound", "detail": "License key does not match given benefit."}` |
| Wrong `organization_id` / unknown key | `404` | `{"error": "ResourceNotFound", "detail": "Not found"}` |

`activate` differs — it raises `NotPermitted` (`403`) for both expired and
inactive, again separable only by `detail`:

```python
if not license_key.is_active():
    raise NotPermitted("License key is no longer active. This license key can not be activated.")
if license_key.expires_at and utc_now() >= license_key.expires_at:
    raise NotPermitted("License key has expired.")
```

---

## The expiry-detection problem

Plan 007 Step 2 says:

> "`expired` must become **'entitled, update window closed'**, not a loss of
> access... `revoked` and `disabled` keep their current meaning and still end
> entitlement immediately. Do not merge these three states."

**Polar merges them for you, at the transport layer, and gives you back one
404.** Matching on the `detail` prose would work today, but it is an
unversioned, human-readable English string with no schema contract and no
changelog — the wrong thing for a decision that grants or denies paid features.

### Why this is smaller than it looks

Plan 007's own design already sidesteps it. The rule is:

> "a build is in-window when `ALFRED_RELEASE_DATE <= licenseUpdateDeadline`"

Both operands are **local**. `ALFRED_RELEASE_DATE` is baked in at compile time.
`licenseUpdateDeadline` comes from `expires_at`, which Alfred receives on **every
successful validation while the key is still live** — which is every validation
for the entire first year. Alfred does not need the network to tell it the key
expired; it can compute that from a deadline it stored a year earlier.

The genuinely new requirement is: **persist `expires_at` when it arrives, and
treat a later 404 as "check the stored deadline first" rather than "invalid
license".**

### Two workable designs — pick one before Step 2

**Option A — trust the stored deadline (recommended).** On a `404`, look at the
persisted `expires_at`. If `now >= expires_at`, the key expired: entitlement
intact, window closed. If there is no stored deadline, or `now < expires_at`,
treat it as revoked/disabled: entitlement ends. No dependence on Polar prose.
Costs one stored timestamp. Its ceiling: a key revoked *after* its deadline
passed reads as merely expired — acceptable, since an out-of-window customer
already keeps only what they paid for.

**Option B — read `detail`, with A as the fallback.** Match
`"License key has expired."` for a positive signal, fall back to Option A when
the string does not match. Slightly sharper today; adds a dependency on an
uncontracted string. Only worth it if Option A's ceiling is unacceptable.

Either way, **do not** let `404` alone mean "unlicensed". That is what ships
today.

---

## What this means for the current Alfred code

Two concrete problems, both found while verifying the above. Fixing them is
Plan 007 Step 2 work — flagged here so the executor does not rediscover them.

**1. `LicenseStatus::Expired` is unreachable in production.**
`src-tauri/src/licensing/service.rs`, `confirmed_state()` (as of this writing,
~line 624) derives it from a *successful* result:

```rust
PolarLicenseState::Granted
    if result.expires_at.is_some_and(|expires| now >= expires) =>
{
    LicenseStatus::Expired
}
```

The comment above it states the intent correctly — "A key past it is `Expired`:
still entitled, window closed." The problem is the input. `PolarLicenseResult`
only exists on a `200`, and Polar never returns `200` once
`utc_now() >= expires_at` — it 404s first, before it ever serializes a body.
This branch can fire in tests (which construct the result directly) and never
in the field.

The same applies to the offline path: a cached `expires_at` will age past `now`
locally, so offline evaluation reaches `Expired` correctly. Only the **online**
confirmation path is dead.

**2. `404` collapses expired into invalid.**
`src-tauri/src/licensing/client.rs:502` maps `(404, PolarClientError::InvalidLicense)`.
An expired key therefore presents as an invalid one — the exact merge Plan 007
Step 2 forbids. **This is the line that must change**: the 404 handler needs to
consult the stored deadline (Option A) before deciding.

---

## What could NOT be determined from documentation alone

Stated plainly, because each of these is a real assumption the build rests on:

1. **Whether the Polar dashboard actually renders "One-time" together with
   "Seat-based".** Documented as a supported combination; the marketing page
   still calls seat-based beta behind a feature flag. The docs step list is
   prose, not a screenshot. **Must be seen.**
2. **Whether a claimed one-time seat's license key really carries a non-null
   `expires_at`.** Inferred from `set_expiration=scope.get("subscription_id") is None`
   plus "one-time purchases have no subscription". Never stated in prose.
   **The single highest-risk inference in this document.** If it is wrong,
   Teams has no update window and Plan 007's Teams row must change.
3. **Whether `main` matches the deployed API.** All source quotes are from
   `polarsource/polar@main` as of 2026-08-20. Polar ships fast. The `detail`
   strings especially may already differ in production — another reason to
   prefer Option A.
4. **Whether the `detail` strings are stable across API versions.** No
   contract, no changelog entry, not in the OpenAPI document. Assume they are
   not.
5. **Whether an out-of-window customer can still download new files from the
   customer portal.** Strongly implied (downloadables grant is untouched by key
   expiry), never stated. Plan 007 already assumes yes and plans the first-run
   explanation around it — worth confirming, since the copy depends on it.
6. **What the customer portal *shows* for an expired key.** Docs say customers
   "See expiration date (if applicable)". Whether it reads as lapsed, invalid,
   or alarming determines whether Alfred's in-app copy contradicts what the
   customer sees on Polar. Cosmetic, but it is the customer's first impression.
7. **Refund-plus-revoke timing.** The refund flow revokes benefits by default;
   how quickly `status` flips to `revoked` on the validate path was not
   measured.
8. **Whether editing a benefit's `expires` TTL re-dates existing keys.**
   `customer_update_grant` copies `expires_at` from a freshly built schema,
   which recomputes from `utc_now()`. A re-grant could silently extend live
   customers' deadlines. Not exercised.

---

## Sandbox verification click paths

Sandbox: <https://sandbox.polar.sh/start>. Fully isolated — "data, users,
tokens, and configuration" are separate from production. API base
`https://sandbox-api.polar.sh/v1`.

Run A, B, C in order; each builds on the last. Total ≈ 35 minutes.

### Path A — one-time product + expiring license key (≈ 10 min)

Resolves: STOP (a), unknown #1 (partly).

1. Open <https://sandbox.polar.sh/start> and enter your sandbox organization.
2. Go to **Products → Benefits → + New Benefit**. Choose Type **License Keys**.
3. Set a prefix (e.g. `ALFRED`). Enable **Automatic Expiration** and set
   **TTL `1`**, **Timeframe `day`**. *Use `day`, not `year` — you need the key
   to actually lapse inside this session. Re-create with `year` for real.*
4. Leave activations and usage limits off for now. Save. **Record the benefit ID.**
5. Go to **Products → New Product**. Under **Pricing**, choose product type
   **One-time**. Set any price. Attach the benefit from step 4. Save.
6. **CHECKPOINT — STOP (a):** if the product form let you pick **One-time** and
   still attach the license-key benefit, (a) is confirmed in the UI.
7. Buy it with a sandbox test card. Open the customer portal from the
   confirmation page.
8. **CHECKPOINT:** the purchase shows a license key **and an expiration date**.
   Copy the key.
9. Validate it and confirm the `200` body carries a non-null `expires_at`:
   ```bash
   curl -sS -X POST https://sandbox-api.polar.sh/v1/customer-portal/license-keys/validate \
     -H "Content-Type: application/json" \
     -d '{"key":"<KEY>","organization_id":"<SANDBOX_ORG_ID>"}' | jq '{status, expires_at, benefit_id}'
   ```
   Expect `{"status": "granted", "expires_at": "<~24h from now>", ...}`.
   **If `expires_at` is `null`, STOP condition (a) is live — report immediately.**

### Path B — the 404 shapes (≈ 10 min, plus a 24h wait or a clock trick)

Resolves: question 5, unknowns #3 and #4. **The highest-value path.**

1. Take a **second** key from a separate purchase of the same product. Revoke it:
   dashboard → **Benefits → your license-key benefit → License Keys** → pick the
   key → **Revoke** (or `PATCH /v1/license-keys/{id}` with `{"status":"revoked"}`
   using an org access token).
2. Validate the revoked key and capture the **full** body and status:
   ```bash
   curl -sS -o /tmp/revoked.json -w '%{http_code}\n' \
     -X POST https://sandbox-api.polar.sh/v1/customer-portal/license-keys/validate \
     -H "Content-Type: application/json" \
     -d '{"key":"<REVOKED_KEY>","organization_id":"<SANDBOX_ORG_ID>"}'
   cat /tmp/revoked.json
   ```
   Expect `404` and `{"error":"ResourceNotFound","detail":"License key is no longer active."}`.
3. Wait for the 1-day key from Path A to lapse, then repeat step 2 against it.
   Expect `404` and `{"error":"ResourceNotFound","detail":"License key has expired."}`.
   *To avoid the wait: create a third benefit with TTL `1` `day`, buy it, then
   `PATCH /v1/license-keys/{id}` setting `expires_at` to a past timestamp if the
   API permits — otherwise just take the 24h.*
4. **CHECKPOINT — the decision:** if the two responses differ **only** in
   `detail`, this document's central finding is confirmed and Plan 007 Step 2
   must adopt Option A (or B). If Polar returns a different status code or a
   distinguishable `error` value for one of them, say so — that reopens the
   simple design.
5. Save both raw bodies. They become the fixtures for the Step 2 acceptance
   tests in `src-tauri/src/licensing/acceptance.rs`.

### Path C — seat-based, one-time, per seat (≈ 15 min)

Resolves: STOP (b), unknowns #1 and #2. **Do not skip step 5.**

1. Dashboard → **Settings → General → Features**. Find **Seat-based pricing**
   and enable it. **If the toggle is absent, request beta access before going
   further — everything below is blocked.**
2. **Products → New Product.** Under **Pricing**: product type **One-time**,
   pricing type **Seat-based**, tiering **Fixed price per seat**. Set a per-seat
   price.
3. **CHECKPOINT — STOP (b):** if **One-time** and **Seat-based** can both be
   selected on the same product, (b) is confirmed. **If selecting Seat-based
   forces the product to Subscription, STOP condition (b) is live — report
   immediately.**
4. Attach the **same license-key benefit** from Path A step 4. Save. Buy it with
   2 seats via the **default Polar confirmation page** (no custom `success_url`),
   so your own seat auto-claims.
5. **CHECKPOINT — the Teams assumption:** open the customer portal and validate
   the seat's key. **Confirm `expires_at` is non-null.**
   ```bash
   curl -sS -X POST https://sandbox-api.polar.sh/v1/customer-portal/license-keys/validate \
     -H "Content-Type: application/json" \
     -d '{"key":"<SEAT_KEY>","organization_id":"<SANDBOX_ORG_ID>"}' | jq '{status, expires_at}'
   ```
   **If `expires_at` is `null`, one-time seats have no update window and Plan
   007's Teams row must change. This is the one result that can still break the
   model.**
6. Assign the second seat to another email. Claim it (link is valid 24h) and
   confirm that seat gets its **own** key with its **own** `expires_at`, dated
   from the claim, not the purchase.
7. Revoke that seat from the portal. Validate its key: expect `404` with
   `"License key is no longer active."` — confirming revoked seats read as
   revoked, not expired.
8. Also confirm the **File Downloads** benefit still lists its files for the
   still-claimed seat, independent of any key state.

### Path D — optional, only if Alfred's checkout uses a custom `success_url`

1. Check whether Plan 003's checkout link sets `success_url`.
2. If it does, repeat Path C step 4 with that link and confirm whether the buyer
   still receives a key. Per the docs they will **not** — they must self-assign
   a seat first.
3. If confirmed, either drop the custom `success_url` for Teams or add a
   self-assign step to the post-purchase flow. This is a launch blocker for
   solo Teams buyers, not a model problem.

---

## Sources

- License Keys benefit — <https://polar.sh/docs/features/benefits/license-keys>
- File Downloads benefit — <https://polar.sh/docs/features/benefits/file-downloads>
- Seat-Based Pricing — <https://polar.sh/docs/features/seat-based-pricing>
- Seat-Based Billing (beta flag) — <https://polar.sh/features/seats>
- Products & benefits — <https://polar.sh/docs/features/products>
- Refunds & benefit revocation — <https://polar.sh/docs/features/refunds>
- API basics, rate limits, sandbox base URL — <https://polar.sh/docs/api-reference/2026-04/introduction>
- Sandbox environment — <https://polar.sh/docs/integrate/sandbox>
- OpenAPI `2026-04` — <https://docs.polar.sh/openapi/2026-04.openapi.json>
  (`ValidatedLicenseKey`, `LicenseKeyStatus`, `ResourceNotFound`,
  `BenefitLicenseKeyExpirationProperties`, `BenefitLicenseKeysProperties`)
- Machine-readable docs corpus — <https://polar.sh/docs/llms-full.txt>
- `polarsource/polar@main` (Apache 2.0):
  - `server/polar/license_key/service.py` — `validate()`, `activate()`, `customer_grant()`, `customer_revoke()`
  - `server/polar/license_key/schemas.py` — `generate_expiration_dt()`, `LicenseKeyCreate.build()`
  - `server/polar/benefit/strategies/license_keys/service.py` — `grant()`, `set_expiration`
  - `server/polar/models/license_key.py` — `is_active()`
  - `server/polar/exceptions.py` — `ResourceNotFound`
  - `server/polar/customer_portal/endpoints/license_keys.py` — portal routes
