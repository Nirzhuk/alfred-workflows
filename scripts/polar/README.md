# Polar sandbox verifier

`sandbox-manifest.json` is the reviewed, non-secret handoff for Alfred's Polar
sandbox resources. Leave an unavailable public value as `null`; the verifier
fails closed, names the field, and points at the fix until every value is
present and valid.

## Two products, two benefits

Alfred sells exactly two products (Plan 007): **Alfred License**, one-time for
one named user, and **Alfred Teams**, one-time per claimed seat. They map to
the benefit classes `individual` and `teams`.

The retired `desktopAnnual` / `desktopLifetime` / `companySeat` shape is
**rejected**, not migrated: a manifest carrying a third benefit fails to parse
and names the offending member. Both benefit IDs are required — there is no
optional class any more.

> **The bound sandbox benefit IDs are stale.** The operator reset the Polar
> products, so both `benefits.*.id` are `null` until they are read off the
> current products and re-verified. Until then `bun run verify:polar-sandbox`
> stops at `verifier-input.manifest`, which is the intended fail-closed state.

Every key now carries a **one-year expiry**, because the update window is
enforced by comparing the build's `ALFRED_RELEASE_DATE` against the license
deadline. The verifier asserts that every key has an expiry that is still ahead
and no more than about a year out. A key with no expiry is a misconfigured
Polar product — the opposite of the retired lifetime rule.

## Links

The manifest accepts only the **sandbox** link shape a sandbox desktop build is
allowed to open,
`https://sandbox-api.polar.sh/v1/checkout-links/polar_cl_.../redirect`, so a
value that would be silently refused at runtime is rejected here instead. The
rules live in `src/features/licensing/public-link-rules.ts`, shared with the
frontend opener and mirrored by the Tauri `opener:allow-open-url` scope. A
production link (`https://buy.polar.sh/polar_cl_...`,
`https://polar.sh/<org-slug>/portal`) is rejected here on purpose.

Only the **Alfred License** checkout link is recorded: Teams is sold on the
marketing website, so Alfred has no Teams checkout entry point and a recorded
`checkoutLinks.teams` is rejected.

`customerPortal.url` takes the sandbox portal,
`https://sandbox.polar.sh/<org-slug>/portal`. Polar's hosted portal is
per-organization; there is no global `/purchases` page (that path 404s). `null`
is still a valid recorded state meaning "not collected yet", so the rest of the
manifest stays usable. See `docs/polar-operator-handoff.md`.

## Test license keys

The verifier reads the two sandbox **test** keys from one of two places, in
order:

1. a secret runner exporting `POLAR_TEST_INDIVIDUAL_KEY` and
   `POLAR_TEST_TEAMS_KEY` (for example `op run -- bun run verify:polar-sandbox`);
2. the git-ignored file `scripts/polar/sandbox-secrets.json.local`, a JSON
   object with `individual` and `teams`.

Setting only some environment variables is an error, not a partial run. The
command takes **no arguments** and refuses to start if given any, so a key
cannot land in shell history or a process listing. Never commit a key, paste
one into a command, or include one in logs or evidence.

## Running it

`bun test scripts/polar` runs the offline mock suite; it needs no network and
no Polar access.

`bun run verify:polar-sandbox` runs the live sandbox proof, and only after the
manifest is complete and a secret source exists. It calls only the sandbox
customer-portal activate, validate, and deactivate endpoints, never sends an
`Authorization` header, and deactivates everything it created in a `finally`
block even when a case fails. Its output is case names plus `PASS`/`FAIL` only.
