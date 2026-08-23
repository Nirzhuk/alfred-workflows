# Polar release acceptance

This directory holds the acceptance evidence for the paid Polar release
described in `plans/release-money/005-run-polar-paid-release-acceptance.md`.

- `TEMPLATE-polar.md` — copy to `<YYYY-MM-DD>-polar.md` and fill in per release
  candidate. Read its "NEVER PUT THESE IN EVIDENCE" header before pasting
  anything.
- `<YYYY-MM-DD>-polar.md` — one completed report per candidate. A source or
  configuration fix creates a new candidate and invalidates the affected rows.

## What is already automated

Matrix D is fully automated with an injected clock in
`src-tauri/src/licensing/acceptance.rs`. It runs as part of
`bun run test:rust` (and therefore `bun run check`) and needs no Polar account,
no purchase, no second machine, and no network access. It proves the exact
day 6/7/8 and 29/30/31 boundaries, the four transient failure classes, the
no-grace cases, immediate restrictive-state precedence, and that no restrictive
license state gates local workflows, memories, schedules, triggers, or data.

Never change the system clock to test these boundaries. If a boundary needs a
new case, add it to the injected-clock module.

The repeatable scans Plan 005 lists are also automated:

```
bun run verify:release-hygiene
```

It exits non-zero on any violation and covers three checks:

| Check | What it enforces |
| --- | --- |
| `architecture-scan` | No shipped README, doc, or source surface still describes the pre-Polar commerce or update architecture. |
| `secret-scan` | No Polar server credential in application source, and license-key handling stays inside the reviewed ephemeral/keychain-only file set. |
| `updater-scan` | `uploadUpdaterJson` stays `false`, no update artifacts are built, and no update plugin is configured. |

Adding a file that touches license-key material fails `secret-scan` until it is
added to the allow-list in `scripts/release/verify-release-hygiene.ts` — that
addition is a deliberate review decision, not a rename.

Matrix D row **D11** stays manual: it is the packaged smoke test that blocks
Polar at the network layer and confirms the settings surface matches what the
automated tests prove.

## What still needs a Polar sandbox

These rows need an authenticated Polar sandbox organization, configured
products and benefits, at least two sandbox email identities, and two private
browser profiles. They cannot be automated from this repository.

| Matrix | Rows | Why |
| --- | --- | --- |
| A | A1–A8 | Real sandbox checkout, receipts, portal login by code, benefit grants, one-year key expiry, refund. |
| B | B1–B10 | Alfred Teams seat assignment, invitations, claims, revocation, and what Polar does when seats are added to a one-time purchase. |
| C | C1, C4, C5 | Needs live activation against issued sandbox keys. |
| E | E3, E4, E6, E8 | Needs files uploaded to the Polar download benefit, a second account to be refused, and a lapsed-window customer to confirm downloads still work. |

## What still needs a clean second machine

| Matrix | Rows | Environment |
| --- | --- | --- |
| C | C1–C9 | Clean Apple Silicon macOS, clean Intel macOS, and a clean Windows 10/11 x64 VM. Three-device and device-limit coverage needs three distinct devices plus a fourth attempt. |
| D | D11, W3, W4 | A packaged build on a machine where Polar can be blocked at the network layer, plus a second candidate built with an out-of-window `ALFRED_RELEASE_DATE` on a machine that already holds real local data. |
| E | E1, E2, E10, E11 | Signing, notarization, and stapling verification against the real downloaded artifacts, plus the signed-macOS and packaged-Windows credential-store smokes inherited from `plans/008-connected-apps-foundation.md`. |

`C7` (locked or unavailable keychain) additionally needs an OS-level locked
credential store, which is why it cannot run in CI.

## What is a documentation review

Matrix F is a human read of every customer-facing surface. `F11` is covered by
`architecture-scan`; `F1`–`F10` are consistency judgements no scan can make.

## Order of work

1. Run `bun run check` and `bun run verify:release-hygiene` on the frozen
   candidate. Record both in section 1 of the report.
2. Fill in matrix D from the automated run, then do the `D11` packaged smoke.
3. Do matrices A and B in the Polar sandbox.
4. Do matrix C on each clean machine.
5. Do matrices E and F, then sign the recommendation.
