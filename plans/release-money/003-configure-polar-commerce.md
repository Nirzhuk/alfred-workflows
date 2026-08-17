# Plan 003: Configure, bind, and prove Alfred's Polar commerce model

> **Executor instructions**: Complete Plans 001–002 first. This is an
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
- **Risk**: HIGH (external billing and beta seat semantics)
- **Depends on**: `archive/001-connect-desktop-polar-licensing.md` and
  `archive/002-build-polar-license-settings.md` (both DONE, archived)
- **Category**: direction, migration, release
- **Planned at**: commit `ecb94d6`, 2026-08-15
- **Execution**: Step 1 completed on isolated branch
  `codex/003-polar-sandbox` at `2ef8cdb`; Steps 2–8 are blocked pending
  approved commercial policy and authenticated Polar sandbox access.

## Why this matters

Polar can replace Alfred's custom checkout, tax, customer-account, email,
license, seat, portal, and download services only if the sandbox proves the
exact product model. Company seat-based pricing is currently beta, so its
member benefit behavior is a release gate—not an assumption for the desktop
implementation.

## Current state

- Alfred's public source remains GPL-3.0-or-later and independently usable.
- The release workflow already stages private GitHub draft installers; macOS
  signing/notarization has passed and Windows is an accepted unsigned beta.
- No Polar production catalog or approved public identifier mapping is
  recorded for the desktop.
- Plans 001–002 provide tested injectable configuration seams, a direct Polar
  license client, safe local state, and License & Billing UI. They use local
  fixtures until this plan supplies the real sandbox public values.
- A custom Stripe implementation exists under the workspace's
  `api-licenses/` directory, but it is abandoned by this roadmap. Do not
  deploy, extend, copy, or delete it in this plan.
- Polar documents public desktop-safe activate, validate, and deactivate
  license-key endpoints that do not require an access token.
- Polar's hosted customer portal handles email-code authentication,
  subscriptions, receipts, license keys, file downloads, activations, and
  seat management.

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

Create these sandbox products:

| Alfred offer | Polar product | Pricing | Benefits |
| --- | --- | --- | --- |
| Desktop annual | standard subscription | yearly | Desktop annual key + downloads |
| Desktop lifetime | standard one-time | one payment | Desktop lifetime key + downloads |
| Company monthly | seat-based subscription | monthly per seat | Company seat key + downloads per claimed seat |
| Company annual | seat-based subscription | yearly per seat | Company seat key + downloads per claimed seat |

Create three separate License Keys benefits so Alfred can identify the safe
product class from `benefit_id`:

- Desktop annual;
- Desktop lifetime;
- Company seat, shared by monthly and annual Company products.

Every key benefit uses a three-activation limit. Create one shared File
Downloads benefit for official installers and attach it to all four products.
For seat-based products, both benefits must be granted to each claimed member,
not only to the billing customer.

## Decisions that must be approved before configuration

- default currency and exact annual/lifetime/monthly/yearly prices;
- tax-inclusive or tax-exclusive display policy;
- refund and cancellation policy;
- confirmation that Desktop annual receives new downloads only while its
  subscription benefit is active;
- confirmation that Desktop lifetime includes future Polar-hosted Alfred
  releases; Polar's one-time File Downloads benefit is perpetual, so a
  backendless limited update window is not an approved alternative;
- three-device limit and customer-facing explanation;
- support address and response promise;
- Company minimum and maximum self-serve seat count.

Do not copy another product's prices. If perpetual future downloads make the
lifetime offer economically unacceptable, remove that offer before continuing
instead of publishing a window Polar cannot enforce without custom machinery.

## Commands you will need

| Purpose | Command | Expected on success |
| --- | --- | --- |
| Full repository check | `bun run check` | frontend tests/build and Rust tests pass |
| Verifier tests | `bun test scripts/polar` | manifest/parser/redaction/mock tests pass |
| Sandbox verifier | `bun run verify:polar-sandbox` | all three configured benefit classes activate, validate, and deactivate; no secret output |
| Secret scan | `rg -n '(POLAR_ACCESS_TOKEN|polar.*secret|POLAR_TEST_.*_KEY)' src src-tauri scripts --glob '!**/*.test.*'` | environment-variable names only; no values or fixtures resembling real keys |

The sandbox verifier reads test license keys from ignored local environment or
the operator's secret runner. Never place a key directly in a shell command,
plan, screenshot, CI log, or committed fixture.

## Scope

**In scope**:

- Polar sandbox organization, products, prices, benefits, checkout links, and
  customer-portal settings;
- four sandbox purchase/benefit journeys;
- a private operator handoff containing the public Polar organization ID,
  three benefit IDs, checkout-link URLs, and portal URL;
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
- release artifact upload (Plan 004);
- deleting the abandoned Stripe implementation.

## Git workflow

- Branch: `codex/003-polar-sandbox`.
- Commit the verifier/config seam separately from operator evidence.
- Do not commit dashboard exports, test keys, customer data, signed download
  URLs, screenshots containing PII, or any Polar access token.
- Do not push, enable live mode, or publish checkout links unless instructed.

## Steps

### Step 1: Add the safe sandbox manifest and verifier

Create a typed, non-secret manifest for the expected organization ID, three
benefit IDs, two checkout links, portal URL, and product/benefit labels. Add a
`verify:polar-sandbox` script that reads the three test license keys only from
ignored environment/secret input, calls the public activate/validate/deactivate
endpoints, checks benefit allow-listing and three-activation behavior, and
prints case names plus pass/fail only.

Unit-test the verifier through a local mock. It must redact request/response
bodies, never send `Authorization`, and clean up activations in `finally`.

**Verify**: `bun test scripts/polar` passes without network or Polar access.

### Step 2: Create the sandbox organization and approve policy

Create or select Alfred's Polar organization in sandbox. Enable seat-based
pricing. Record the organization owner, recovery method, payout/account-review
requirements, and current fee plan privately. Approve every pricing/support
decision above before making public checkout links.

**Verify**: the operator can sign in with recovery tested; sandbox is visibly
selected; no live product or payment is created.

### Step 3: Create products and license benefits

Create the four products and three License Keys benefits exactly as specified.
Use recognizable Alfred prefixes, three activations, automatic subscription
revocation, and no arbitrary expiration on lifetime ownership. Attach the
correct license benefit to each product.

**Verify**: dashboard inspection shows four products, three distinct benefit
IDs, and the exact attachment matrix; no product grants two license keys.

### Step 4: Configure Company seats and portal ownership

Enable seat management in Polar's hosted portal. Configure Company checkout so
the buyer can purchase a bounded quantity and receives a member/owner record.
Use Polar's default confirmation page for the first release. Current Polar
documentation says the purchaser becomes the team owner but does not receive
benefits automatically; the owner must assign and claim one purchased seat for
themselves through Polar's hosted flow. A custom success URL is not allowed
unless the full hosted claim flow is re-tested.

Confirm that a billing owner can assign, resend, revoke, and resize seats and
that every claimed member—not merely the purchaser—receives the Company key
and downloads benefits.

**Verify**: one three-seat sandbox order creates the purchaser as owner without
granting benefits prematurely; the owner can assign and claim one seat for
themselves, leaving two available seats. A second email can claim another seat
and see its own benefits.

### Step 5: Create checkout links and hosted portal entry points

Create a Desktop checkout link offering annual/lifetime choice and a Company
checkout link offering monthly/annual seat-based choice. Leave success URLs on
Polar's hosted confirmation page. Enable only approved discount and billing
options. Confirm Polar's transactional email links reach the customer portal,
so Alfred does not need a pre-authenticated portal-link backend.

**Verify**: opening each persistent checkout link in a private browser creates
a new sandbox checkout with correct product choices, currency, tax display,
quantity controls, terms, and return behavior.

### Step 6: Configure the shared downloads benefit

Create one File Downloads benefit and attach it to all four products. Upload a
small non-production fixture for sandbox proof. Confirm standard purchasers
and claimed Company members receive personal signed download access, while an
unclaimed invite and an unrelated email do not.

**Verify**: authorized portal downloads succeed; unauthorized access cannot
obtain the file URL or bytes.

### Step 7: Prove license activation lifecycle without credentials

Run `bun run verify:polar-sandbox` against the three test key-benefit types.
Use temporary device labels/activation IDs. Verify a fourth activation fails
after three active allocations and succeeds after one is deactivated.

Cancel/end an annual subscription and revoke a Company member in sandbox.
Confirm their keys become non-granted according to Polar's documented timing.
Confirm the lifetime key remains granted.

**Verify**: every request succeeds without `Authorization` or an organization
access token; sanitized evidence records only status, benefit class, and test
case result—not full keys, emails, URLs, or customer payloads.

### Step 8: Bind the public configuration and run desktop integration smoke

Bind these public values to the configuration seams created by Plans 001–002
through repository/release variables or a reviewed public configuration file:

- Polar organization ID;
- Desktop annual, Desktop lifetime, and Company seat benefit IDs;
- Desktop and Company checkout-link URLs;
- hosted customer-portal URL.

Product IDs, price IDs, access tokens, webhook secrets, and customer IDs are
not needed in Alfred. Build a sandbox-configured Alfred, activate each benefit
class, relaunch, refresh, open both checkout links and the portal, then
deactivate. Keep macOS/Windows packaged breadth for Plan 005; this plan needs
one available desktop target.

**Verify**: a reviewer maps every benefit ID to exactly one Alfred product
class; all three activation classes and configured links work on one desktop;
`bun run check` passes; secret scanning shows no Polar access token.

## Done criteria

- [ ] Pricing, tax, refund, lifetime-update, device, Company, and support policy are approved.
- [ ] The redacting sandbox verifier and its local mock tests pass.
- [ ] Four sandbox products and the three-key/one-download benefit model exist.
- [ ] Company purchase, buyer self-assignment/claim, member claim, seat revoke, and seat resize pass.
- [ ] Desktop annual/lifetime and Company checkout links show correct live sandbox data.
- [ ] Authorized customers and claimed members receive hosted downloads.
- [ ] Public activate/validate/deactivate and the three-device limit pass.
- [ ] Subscription end and Company revoke remove access; lifetime remains granted.
- [ ] The public configuration handoff contains no credential.
- [ ] A sandbox-configured Alfred completes activation, refresh, link opening, and deactivation.
- [ ] `bun run check` and the exact secret scan pass.
- [ ] The roadmap row is `DONE`.

## STOP conditions

- Polar seat-based pricing is unavailable or unsuitable for the launch account.
- A claimed Company member cannot receive an independent license key and download benefit.
- The billing purchaser cannot self-claim a seat through the chosen hosted flow.
- Public key endpoints require a Polar access token or expose a privileged operation.
- Lifetime or subscription revocation semantics contradict Alfred's published promise.
- The operator wants a limited lifetime-download window but will not remove the lifetime offer.
- Customer download files are anonymously accessible.
- Final price, refund, update, seat, or support policy is unresolved.

## Maintenance notes

Polar's seat model is beta. Re-run the Company proof before changing portal
settings, confirmation URLs, benefits, or product pricing type. Keep the
private identifier handoff current, but never turn it into a credential file.
