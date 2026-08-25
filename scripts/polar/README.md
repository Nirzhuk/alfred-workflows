# Polar sandbox verifier

`sandbox-manifest.json` is the reviewed, non-secret handoff for Alfred's Polar
sandbox resources. Leave an unavailable public value as `null`; the verifier
fails closed, names the field, and points at the fix until every value is
present and valid.

## One product, one benefit

Alfred sells exactly one product (supporter model settled 2026-08):
**Alfred Supporter**, a one-time purchase. It maps to the single benefit class
`supporter`.

The retired shapes are **rejected**, not migrated: a manifest carrying a
two-product (`benefits.individual`, `benefits.teams`) or older four-product
(`desktopAnnual`, `desktopLifetime`, `companySeat`) member fails to parse and
names the offending member. The supporter benefit ID is required.

> **The sandbox checkout link is not collected yet.** The manifest records
> `checkoutLinks.supporter.url` as `null` until the operator creates the link
> in the Polar dashboard. Until then `bun run verify:polar-sandbox` stops at
> `manifest.checkout.supporter` before making any network call, which is the
> intended fail-closed state.

Supporter licences are **perpetual**: benefits are configured WITHOUT a
license-key expiration, and keys are issued with `expires_at: null`. The
verifier enforces the absence on both layers:

- The benefit may not record an expiration. `benefits.supporter.expiry` is
  absent from the manifest; if a value is ever recorded there (or an
  expiration is configured on the benefit in Polar), `bun run
  verify:polar-sandbox` fails at `manifest.expiry` with "supporter licences
  are perpetual". A structurally invalid recorded expiry (a ttl below 1, an
  unknown timeframe, an extra key) fails to parse.
- Every live test key must read back `expires_at: null`. A key carrying any
  expiry means the Polar product is misconfigured — the opposite of the
  retired one-year rule.

The update-window machinery from Plan 007 stays idle in this model; keys never
expire and no build loses capabilities over time.

## Legacy env-slot mapping (intentional)

The supporter benefit binds through the previously individual-named slots,
which were deliberately **not** renamed:

- Rust/`.env`: the supporter benefit ID goes into
  `ALFRED_POLAR_INDIVIDUAL_BENEFIT_ID` (unchanged today). The Teams slot
  `ALFRED_POLAR_TEAMS_BENEFIT_ID` stays unset-optional.
- Verifier secrets: the test key is read from `POLAR_TEST_INDIVIDUAL_KEY`
  or from the ignored local file under either `"supporter"` or the legacy
  `"individual"` name, so an operator's existing secret-runner invocation
  keeps working.

## Links

The manifest accepts only the **sandbox** link shape a sandbox desktop build is
allowed to open,
`https://sandbox-api.polar.sh/v1/checkout-links/polar_cl_.../redirect`, so a
value that would be silently refused at runtime is rejected here instead. The
rules live in `src/features/licensing/public-link-rules.ts`, shared with the
frontend opener and mirrored by the Tauri `opener:allow-open-url` scope. A
production link (`https://buy.polar.sh/polar_cl_...`,
`https://polar.sh/<org-slug>/portal`) is rejected here on purpose.

Only the **Alfred Supporter** checkout link exists, and it is optional at
parse time: `url: null` records "not collected yet" so the rest of the
manifest stays usable. A still-null link fails verification pre-network at
`manifest.checkout.supporter`. A recorded `checkoutLinks.teams` or
`checkoutLinks.individual` is rejected outright.

`customerPortal.url` takes the sandbox portal,
`https://sandbox.polar.sh/<org-slug>/portal`. Polar's hosted portal is
per-organization; there is no global `/purchases` page (that path 404s). `null`
is still a valid recorded state meaning "not collected yet". See
`docs/polar-operator-handoff.md`.

## Test license key

The verifier reads the single sandbox **test** key from one of two places, in
order:

1. a secret runner exporting `POLAR_TEST_INDIVIDUAL_KEY` (for example
   `op run -- bun run verify:polar-sandbox`);
2. the git-ignored file `scripts/polar/sandbox-secrets.json.local`, a JSON
   object with `"supporter"` (the legacy `"individual"` name is accepted).

Setting the variable to an empty value is not a partial run; there is exactly
one key. The command takes **no arguments** and refuses to start if given any,
so a key cannot land in shell history or a process listing. Never commit a
key, paste one into a command, or include one in logs or evidence.

## Running it

`bun test scripts/polar` runs the offline mock suite; it needs no network and
no Polar access.

`bun run verify:polar-sandbox` runs the live sandbox proof, and only after the
manifest is complete and a secret source exists. It calls only the sandbox
customer-portal activate, validate, and deactivate endpoints, never sends an
`Authorization` header, and deactivates everything it created in a `finally`
block even when a case fails. Its output is case names plus `PASS`/`FAIL` only.
