# Polar paid-release acceptance report — TEMPLATE

> Copy this file to `docs/release-acceptance/<YYYY-MM-DD>-polar.md` before the
> first scenario. Fill it in as you go; do not batch evidence at the end.
> Source matrix: `plans/release-money/005-run-polar-paid-release-acceptance.md`.

---

## ⛔ NEVER PUT THESE IN EVIDENCE

Every attachment, screenshot, log excerpt, checksum note, and table cell in
this report is reviewed by people who are not the buyer. The following must
**never** appear here, in a linked artifact, or in a commit message:

- **full license keys** — record the masked form only (`••••-CRET`);
- **activation IDs** — record the device label instead;
- **customer email addresses** — record `buyer-1` / `member-2` style aliases;
- **one-time login codes** or portal magic links;
- **personal download URLs** — these are per-customer authorizations;
- **payment details** — card data, last four digits, billing addresses, tax IDs;
- **Polar customer objects** or any raw API response body;
- **signing credentials**, notarization keys, API tokens, or `POLAR_ACCESS_TOKEN`.

Refer to Polar resources by their **sandbox label** (for example
`sandbox / Alfred License / license-key benefit`), never by a customer payload.
Redact before pasting. If a secret reaches this report, treat it as a STOP
condition, rotate it, and rewrite the history that contains it.

---

## 1. Frozen candidate

| Field | Value |
| --- | --- |
| Source commit | `—` |
| `package.json` version | `—` |
| `src-tauri/tauri.conf.json` version | `—` |
| `src-tauri/Cargo.toml` version | `—` |
| `ALFRED_RELEASE_DATE` baked into artifacts | `—` (ISO `YYYY-MM-DD`) |
| Same value in the acceptance manifest? | `—` |
| Polar environment | `sandbox` |
| Sandbox organization label | `—` |
| Report started (UTC) | `—` |
| Lead tester | `—` |

### Artifacts under test

| Artifact | Platform / arch | SHA-256 | Polar file label |
| --- | --- | --- | --- |
| `—` | macOS arm64 | `—` | `—` |
| `—` | macOS x86_64 | `—` | `—` |
| `—` | Windows x64 (unsigned beta) | `—` | `—` |

### Repository gates

| Gate | Command | Result | UTC time |
| --- | --- | --- | --- |
| Desktop checks | `bun run check` | `—` | `—` |
| Workflow lint | `actionlint .github/workflows/release.yml` | `—` | `—` |
| Release hygiene scans | `bun run verify:release-hygiene` | `—` | `—` |
| Rust acceptance (clock + date injection) | `bun run test:rust` | `—` | `—` |
| Tier scan (read every hit; none may offer an Alfred annual / lifetime / subscription tier) | `rg -ni 'annual\|lifetime\|subscription' README.md docs src src-tauri` | `—` | `—` |

---

## 2. Matrix A — Alfred License purchase and hosted benefits

**One product, one run.** Alfred License is a single one-time purchase; there is
no annual/lifetime choice and no subscription to cancel.

- [ ] A1 — checkout link shows the correct product, one-time price, tax treatment, and terms
- [ ] A2 — sandbox purchase completes on Polar's hosted confirmation page
- [ ] A3 — receipt email reaches the buyer and its portal link authenticates by code
- [ ] A4 — portal shows exactly one correct key and official download benefit
- [ ] A5 — downloaded installers and checksums equal the accepted candidate
- [ ] A6 — the issued key carries a one-year expiry, and every surface presents that date as an **update deadline**, never as the end of the purchase or of access
- [ ] A7 — refund/revocation produces the approved key and download transition and **does** end entitlement
- [ ] A8 — Alfred consumes no checkout success URL and no webhook

| Row | Commit / version | Platform / arch | Sandbox resource label | Artifact SHA-256 | Expected | Observed | Evidence | Tester | UTC |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| A1 | `—` | `—` | `—` | n/a | correct product, one-time price, tax, terms | `—` | `—` | `—` | `—` |
| A2 | `—` | `—` | `—` | n/a | hosted confirmation completes | `—` | `—` | `—` | `—` |
| A3 | `—` | `—` | `—` | n/a | receipt arrives; code login works | `—` | `—` | `—` | `—` |
| A4 | `—` | `—` | `—` | n/a | exactly one key + download benefit | `—` | `—` | `—` | `—` |
| A5 | `—` | `—` | `—` | `—` | checksum equals candidate | `—` | `—` | `—` | `—` |
| A6 | `—` | `—` | `—` | n/a | one-year expiry, worded as an update deadline | `—` | `—` | `—` | `—` |
| A7 | `—` | `—` | `—` | n/a | approved transition; entitlement ends | `—` | `—` | `—` | `—` |
| A8 | `—` | `—` | `—` | n/a | no success-URL or webhook consumption | `—` | `—` | `—` | `—` |

---

## 3. Matrix B — Alfred Teams seats

**One run**, one-time per seat, with at least three seats. There are no
proration, paid-quantity-floor, or `canceled`/`past-due` rows: none of those
states exist on a one-time purchase.

- [ ] B1 — checkout quantity and total are correct, and are presented as a **one-time payment per seat**, not a recurring charge
- [ ] B2 — buyer becomes owner, then explicitly assigns and claims one seat; no benefit is granted merely for being the billing purchaser
- [ ] B3 — assigning a second email sends an invitation that stays pending until claim
- [ ] B4 — claim grants that member an independent Teams key and downloads
- [ ] B5 — a third available seat can be assigned and claimed
- [ ] B6 — revoking a member removes their benefits and frees the assignment
- [ ] B7 — adding seats behaves as an **observed, recorded** transaction (expected: a second purchase of the same product). Proration or a recurring charge is a STOP, not a variation
- [ ] B8 — a refunded purchase does not over-grant and ends entitlement for the seats it covered
- [ ] B9 — unclaimed and unrelated users receive no key or file access
- [ ] B10 — each claimed member's key carries its own one-year expiry

| Row | Commit / version | Platform / arch | Sandbox resource label | Artifact SHA-256 | Expected | Observed | Evidence | Tester | UTC |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| B1 | `—` | `—` | `—` | n/a | quantity and total correct, one-time per seat | `—` | `—` | `—` | `—` |
| B2 | `—` | `—` | `—` | n/a | owner must claim a seat to hold a benefit | `—` | `—` | `—` | `—` |
| B3 | `—` | `—` | `—` | n/a | invitation pending until claim | `—` | `—` | `—` | `—` |
| B4 | `—` | `—` | `—` | `—` | independent member key + downloads | `—` | `—` | `—` | `—` |
| B5 | `—` | `—` | `—` | n/a | third seat assigns and claims | `—` | `—` | `—` | `—` |
| B6 | `—` | `—` | `—` | n/a | revoke removes benefits, frees the seat | `—` | `—` | `—` | `—` |
| B7 | `—` | `—` | `—` | n/a | record exactly what Polar does when seats are added | `—` | `—` | `—` | `—` |
| B8 | `—` | `—` | `—` | n/a | refund does not over-grant | `—` | `—` | `—` | `—` |
| B9 | `—` | `—` | `—` | n/a | outsiders get nothing | `—` | `—` | `—` | `—` |
| B10 | `—` | `—` | `—` | n/a | per-member one-year expiry | `—` | `—` | `—` | `—` |

---

## 4. Matrix C — Desktop activation and secure storage

Run for the **individual** and **teams** keys, on each required platform.

- [ ] C1 — activate on device 1 and validate
- [ ] C2 — quit/relaunch loads cached status with no network delay
- [ ] C3 — activate through device 3; device 4 receives the device-limit state
- [ ] C4 — deactivate an old activation in Polar, then activate a replacement
- [ ] C5 — Refresh maps `granted`, `revoked`, `disabled`, and invalid safely, and maps **`expired` to "entitled, update window closed"** — a licensed state. `expired` appears in the licensed badge states; `revoked` and `disabled` do not
- [ ] C6 — Deactivate this device calls Polar before clearing local credentials
- [ ] C7 — locked/unavailable keychain never falls back to SQLite or plaintext
- [ ] C8 — logs, SQLite, DOM after submit, URLs, and evidence contain no full key or activation ID
- [ ] C9 — only the public organization/benefit IDs and `ALFRED_RELEASE_DATE` are compiled in; no access token is present

| Row | Key type | Commit / version | Platform / arch | Sandbox resource label | Artifact SHA-256 | Expected | Observed | Evidence | Tester | UTC |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| C1 | individual \| teams | `—` | `—` | `—` | `—` | activation succeeds and validates | `—` | `—` | `—` | `—` |
| C2 | individual \| teams | `—` | `—` | `—` | `—` | cached status, no network wait | `—` | `—` | `—` | `—` |
| C3 | individual \| teams | `—` | `—` | `—` | `—` | device 4 shows `deviceLimit` | `—` | `—` | `—` | `—` |
| C4 | individual \| teams | `—` | `—` | `—` | `—` | replacement device activates | `—` | `—` | `—` | `—` |
| C5 | individual \| teams | `—` | `—` | `—` | `—` | `expired` = entitled, window closed; `revoked`/`disabled` = not entitled | `—` | `—` | `—` | `—` |
| C6 | individual \| teams | `—` | `—` | `—` | `—` | remote call precedes local clear | `—` | `—` | `—` | `—` |
| C7 | individual \| teams | `—` | `—` | `—` | `—` | `secureStorageUnavailable`, no fallback | `—` | `—` | `—` | `—` |
| C8 | individual \| teams | `—` | `—` | `—` | `—` | no key or activation ID anywhere | `—` | `—` | `—` | `—` |
| C9 | n/a | `—` | `—` | `—` | `—` | public IDs + release date only, no token | `—` | `—` | `—` | `—` |

---

## 5. Matrix D — Offline, restrictive-state, and update-window behavior

**Mostly automated.** Every time boundary is proven by injected-clock tests and
every update-window case by injected-date tests, both in
`src-tauri/src/licensing/acceptance.rs`. Run `bun run test:rust` on the frozen
candidate and record the result once. Do not change the system clock, and do not
hand-edit a binary's baked release date — W3 and W4 need a second candidate
built with an out-of-window `ALFRED_RELEASE_DATE`.

- [ ] D1 — cached active state before day 7 does not trigger a refresh
- [ ] D2 — refresh becomes due exactly at day 7
- [ ] D3 — transient timeout / connect (DNS) / 429 / 5xx yields offline grace
- [ ] D4 — grace continues through day 30 and the boundary is exact
- [ ] D5 — after day 30 the state is `needsOnline`
- [ ] D6 — a key that was never successfully validated gets no grace
- [ ] D7 — a key whose benefit ID is not in the configured allow-list gets no grace
- [ ] D8 — a confirmed **revoked or disabled** response overrides grace immediately. **`expired` does not** — it is an entitled state and must never be treated as restrictive
- [ ] D9 — network failure alone never renders as revoked
- [ ] D10 — every restrictive state leaves local workflows, memories, schedules, triggers, and data usable
- [ ] D11 — packaged smoke: block Polar at the network layer on a packaged build and confirm the settings surface matches D3/D9

| Row | Test | Expected | Observed | Evidence | Tester | UTC |
| --- | --- | --- | --- | --- | --- | --- |
| D1 | `cached_active_state_before_day_seven_never_triggers_a_refresh` | days 0–6 not due | `—` | `—` | `—` | `—` |
| D2 | `refresh_becomes_due_exactly_at_day_seven_and_stays_due` | 6 no / 7 yes / 8 yes | `—` | `—` | `—` | `—` |
| D3 | `every_transient_failure_class_yields_offline_grace_through_day_thirty` | all four classes grant grace | `—` | `—` | `—` | `—` |
| D4 | `offline_grace_ends_exactly_after_day_thirty` | 29 and 30 grace | `—` | `—` | `—` | `—` |
| D5 | `cached_reads_expose_needs_online_only_after_the_day_thirty_deadline` | 30+1ns and 31 `needsOnline` | `—` | `—` | `—` | `—` |
| D6 | `a_key_that_was_never_validated_receives_no_offline_grace` | state unchanged | `—` | `—` | `—` | `—` |
| D7 | `an_unknown_benefit_key_receives_no_offline_grace` | `disabled` / `unsupported_product` | `—` | `—` | `—` | `—` |
| D8 | `a_confirmed_restrictive_response_overrides_remaining_grace_immediately`, `a_confirmed_invalid_license_revokes_without_consuming_grace` | `revoked`/`disabled` restrict at once; **`expired` stays entitled** | `—` | `—` | `—` | `—` |
| D9 | `network_failure_alone_never_renders_as_revoked`, `an_outage_past_day_thirty_needs_online_rather_than_revoked` | `offlineGrace` then `needsOnline` | `—` | `—` | `—` | `—` |
| D10 | `every_restrictive_license_state_leaves_local_data_usable`, `a_refresh_past_grace_never_touches_local_data`, `the_licensing_contract_exposes_no_kill_switch_over_local_features` | local data reads and writes keep working | `—` | `—` | `—` | `—` |
| D11 | packaged smoke (manual) | UI shows grace, never revoked | `—` | `—` | `—` | `—` |

### Update window

The rule is one line: a build is in window when
`ALFRED_RELEASE_DATE <= licenseUpdateDeadline`.

- [ ] W1 — **in-window build** (release date before the deadline, key `granted`): pro features on
- [ ] W2 — **exactly at the deadline** (`ALFRED_RELEASE_DATE == licenseUpdateDeadline`): **in window**, pro features on. The comparison is `<=`
- [ ] W3 — **out-of-window build**: the app runs, every workflow, memory, schedule, trigger, and file stays intact and usable, and only pro features are locked
- [ ] W4 — **out-of-window explains itself once** on first run: one dismissible message, then silence — not a silent lock, not a repeated nag, never a block on local data
- [ ] W5 — **expired key keeps entitlement** on the build it was bought for: still licensed, pro features still on, permanently
- [ ] W6 — **revoked ends entitlement** immediately, on any build
- [ ] W7 — **disabled ends entitlement** immediately, on any build
- [ ] W8 — **unset `ALFRED_RELEASE_DATE`** (source build): never locks anything; no window comparison runs
- [ ] W9 — **source build is unlocked**: a build with no `ALFRED_POLAR_*` configuration has every feature, no licence prompt, and no nag

| Row | How proven | Expected | Observed | Evidence | Tester | UTC |
| --- | --- | --- | --- | --- | --- | --- |
| W1 | injected date | pro features on | `—` | `—` | `—` | `—` |
| W2 | injected date | equal dates are **in window** | `—` | `—` | `—` | `—` |
| W3 | packaged out-of-window candidate | runs; all local data intact; only pro locked | `—` | `—` | `—` | `—` |
| W4 | packaged out-of-window candidate | explained once, dismissible | `—` | `—` | `—` | `—` |
| W5 | injected date | expired = entitled, permanently | `—` | `—` | `—` | `—` |
| W6 | injected state | entitlement ends at once | `—` | `—` | `—` | `—` |
| W7 | injected state | entitlement ends at once | `—` | `—` | `—` | `—` |
| W8 | injected date (unset) | nothing locks | `—` | `—` | `—` | `—` |
| W9 | source build | every feature available, no nag | `—` | `—` | `—` | `—` |

---

## 6. Matrix E — Official distribution, update truthfulness, and packaged credential storage

- [ ] E1 — required DMGs verify signing, notarization, and stapling
- [ ] E2 — the Windows NSIS installer is labeled unsigned beta everywhere
- [ ] E3 — Polar copies match the private draft checksums byte for byte
- [ ] E4 — unrelated and unclaimed accounts cannot download
- [ ] E5 — customer-facing download pages link the corresponding source and license notices
- [ ] E6 — **Download latest version** opens Polar's portal
- [ ] E7 — no public binary release, public cask, update manifest, update plugin, or automatic-update promise exists
- [ ] E8 — a customer whose update window has closed **can** still download newer files. This is expected (the File Downloads benefit is perpetual) and the app explains it per W4 — it is not a defect
- [ ] E9 — the acceptance manifest's `ALFRED_RELEASE_DATE` matches the value actually baked into the tested artifacts
- [ ] E10 — **signed/notarized macOS package credential-store smoke** (moved here from `plans/008-connected-apps-foundation.md`): on the signed, notarized, stapled build, create / read / overwrite / delete a connected-app credential in the macOS Keychain, restart the app, and confirm the credential persists and is readable under the production app identity
- [ ] E11 — **packaged Windows build credential-store smoke** (moved here from `plans/008-connected-apps-foundation.md`): the same create / read / overwrite / delete / restart cycle against Windows Credential Manager on the packaged build

The packaged **Linux** Secret Service equivalent is **not** in this report — it
stays in `plans/008-connected-apps-foundation.md`, because Plan 005 mandates no
Linux environment. E10 and E11 exist separately from C7 because development and
production app identities can receive different keychain access, so a
dev-checkout pass proves nothing about the shipped bundle.

| Row | Commit / version | Platform / arch | Sandbox resource label | Artifact SHA-256 | Expected | Observed | Evidence | Tester | UTC |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| E1 | `—` | macOS arm64 / x86_64 | `—` | `—` | `codesign` and `stapler` exit 0 | `—` | `—` | `—` | `—` |
| E2 | `—` | Windows x64 | `—` | `—` | labeled unsigned beta on every surface | `—` | `—` | `—` | `—` |
| E3 | `—` | all | `—` | `—` | Polar copy equals draft checksum | `—` | `—` | `—` | `—` |
| E4 | `—` | n/a | `—` | n/a | download refused | `—` | `—` | `—` | `—` |
| E5 | `—` | n/a | `—` | n/a | source and license links present | `—` | `—` | `—` | `—` |
| E6 | `—` | all | `—` | n/a | button opens the hosted portal | `—` | `—` | `—` | `—` |
| E7 | `—` | n/a | `—` | n/a | `bun run verify:release-hygiene` passes | `—` | `—` | `—` | `—` |
| E8 | `—` | n/a | `—` | n/a | lapsed customer still downloads; app explains | `—` | `—` | `—` | `—` |
| E9 | `—` | all | `—` | `—` | manifest date equals baked date | `—` | `—` | `—` | `—` |
| E10 | `—` | macOS arm64 / x86_64 (signed + notarized) | `—` | `—` | credential survives create/read/overwrite/delete and restart | `—` | `—` | `—` | `—` |
| E11 | `—` | Windows x64 (packaged) | `—` | `—` | credential survives create/read/overwrite/delete and restart | `—` | `—` | `—` | `—` |

---

## 7. Matrix F — Product, legal, and support consistency

Check `README.md`, the install / open-source / releasing docs, in-app settings,
Polar checkout, portal and product descriptions, receipts, release notes, and
support pages. Every surface must agree on each claim below.

- [ ] F1 — **Alfred License** is sold to one named user, one-time
- [ ] F2 — **Alfred Teams** is licensed one-time per claimed seat
- [ ] F3 — pro features are **permanent** once purchased
- [ ] F4 — the purchase includes **one year of updates**
- [ ] F5 — **what lapsing does**: newer builds' pro features stay locked until the customer buys again
- [ ] F6 — **what lapsing does not do**: it never disables the build they have, never removes a paid feature, and never touches local data
- [ ] F7 — refunded / revoked / disabled **does** end entitlement, and is described as different from a lapsed window
- [ ] F8 — the device limit is stated identically everywhere
- [ ] F9 — the 7-day refresh / 30-day offline policy is stated identically everywhere
- [ ] F10 — updates are manual through Polar
- [ ] F11 — refund and cancellation terms agree
- [ ] F12 — the Windows build is described as beta everywhere
- [ ] F13 — GPL source rights are stated correctly, and **building from source is free and fully featured, forever**
- [ ] F14 — Teams is purchased on the marketing website, not in the app
- [ ] F15 — no surface claims payment restricts commercial use of the GPL source
- [ ] F16 — no surface offers an Alfred annual, lifetime, or subscription tier (read every tier-scan hit; third-party subscriptions and labeled history are fine)
- [ ] F17 — no surface still describes the pre-Polar commerce or update architecture (`bun run verify:release-hygiene`)

| Row | Surfaces checked | Commit / version | Expected | Observed | Evidence | Tester | UTC |
| --- | --- | --- | --- | --- | --- | --- | --- |
| F1 | `—` | `—` | one named user, one-time | `—` | `—` | `—` | `—` |
| F2 | `—` | `—` | one-time per claimed seat | `—` | `—` | `—` | `—` |
| F3 | `—` | `—` | pro features permanent | `—` | `—` | `—` | `—` |
| F4 | `—` | `—` | one year of updates | `—` | `—` | `—` | `—` |
| F5 | `—` | `—` | lapse locks only newer builds | `—` | `—` | `—` | `—` |
| F6 | `—` | `—` | lapse takes nothing away | `—` | `—` | `—` | `—` |
| F7 | `—` | `—` | refund/revoke/disable ends entitlement | `—` | `—` | `—` | `—` |
| F8 | `—` | `—` | identical device limit | `—` | `—` | `—` | `—` |
| F9 | `—` | `—` | identical 7/30-day policy | `—` | `—` | `—` | `—` |
| F10 | `—` | `—` | manual updates via Polar | `—` | `—` | `—` | `—` |
| F11 | `—` | `—` | identical refund/cancellation terms | `—` | `—` | `—` | `—` |
| F12 | `—` | `—` | Windows beta everywhere | `—` | `—` | `—` | `—` |
| F13 | `—` | `—` | GPL rights correct; source build free and full | `—` | `—` | `—` | `—` |
| F14 | `—` | `—` | Teams sold on the website | `—` | `—` | `—` | `—` |
| F15 | `—` | `—` | no claim restricting GPL source use | `—` | `—` | `—` | `—` |
| F16 | `—` | `—` | no annual/lifetime/subscription tier | `—` | `—` | `—` | `—` |
| F17 | `—` | `—` | hygiene scans pass | `—` | `—` | `—` | `—` |

---

## 8. Defects

| ID | Row | Severity | Summary | Status | Fix commit | Rows invalidated |
| --- | --- | --- | --- | --- | --- | --- |
| `—` | `—` | P0 \| P1 \| P2 | `—` | open \| fixed \| accepted | `—` | `—` |

Any source or configuration fix creates a new candidate and invalidates the
affected evidence. Re-run that scenario plus its regression dependencies and
record the new commit in section 1.

---

## 9. Launch recommendation

| Field | Value |
| --- | --- |
| Decision | `GO` \| `NO-GO` |
| Known limitations | `—` |
| Unresolved P0/P1 defects | `—` |
| Report SHA-256 | `—` |
| Signed by | `—` |
| UTC time | `—` |

Record `GO` only when every A–F row — including every W row — has reproducible
PASS evidence and no P0/P1 defect remains. A `NO-GO` leaves Plan 005 `BLOCKED`
with defect references.

Two failures are automatic P0 regardless of anything else: **`expired` removing
entitlement anywhere**, and **an out-of-window build hiding, dropping, or
refusing access to local data**. Both take away something a customer already
paid for.
