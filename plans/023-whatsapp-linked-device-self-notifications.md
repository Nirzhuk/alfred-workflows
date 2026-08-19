# Plan 023: Send self-notifications through an experimental WhatsApp linked device

> **Executor instructions**: Execute only after Plan 021 and the mandatory
> feasibility spike in Step 1. This integration deliberately uses the
> unofficial `whatsapp-rust` WhatsApp Web implementation, not Meta's Business
> Platform. It links exactly one personal account by QR code, stays connected
> only while Alfred is running, and can send plain text only to that account's
> own “Message yourself” chat. Preserve this narrow boundary.
>
> **Risk warning**: `whatsapp-rust` is an unofficial protocol reimplementation.
> Protocol changes can break it, and WhatsApp may restrict or suspend an
> account. The connect UI must disclose this before pairing. Never describe the
> integration as official, supported by Meta, or ban-safe.
>
> **Drift check (run first)**: inspect Plans 008, 009, and 021 postconditions;
> the current `whatsapp-rust` release, license, transitive dependencies,
> storage traits, QR/session lifecycle, self-JID behavior, retry persistence,
> logging features, and supported Rust toolchain; Alfred's credential store,
> SQLite linkage, app-data paths, tray lifecycle, and shutdown handling; and
> packaged builds for every shipping OS. Pin reviewed source exactly.

## Status

- **Priority**: P1
- **Effort**: XL
- **Risk**: HIGH
- **Depends on**: Plans 008 and 009
- **Execution order**: after Plan 021; Step 1 gates all product work
- **Does not depend on**: Plan 010 events or Plan 011 relay
- **Category**: experimental integration
- **Planned at**: 2026-08-16 after splitting Plan 021
- **Implementation status**: IN PROGRESS (Step 1 spike: 7 of 9 gates green;
  gate 8 RED with a required Step 5 mitigation; gate 9 packaged smoke pending)

## Product decisions

- The only use case is an Alfred workflow sending a notification to the paired
  WhatsApp account's own self-chat.
- V1 supports exactly one paired WhatsApp account per Alfred installation.
- Pairing uses a WhatsApp Linked Devices QR code. There is no phone-number,
  pair-code, Business Platform, access-token, template, or recipient setup.
- The destination is derived from the authenticated device's own JID. A
  workflow, frontend form, or caller can never choose or interpolate a JID,
  phone number, contact, group, or recipient.
- The client remains connected for the entire Alfred process lifetime,
  including tray mode, and shuts down cleanly when Alfred exits. Alfred sends
  nothing while it is not running.
- V1 is strictly outbound-only. It exposes no incoming messages, history,
  contacts, search, replies, triggers, read state, media, or presence features.
- Session/device/Signal state persists in a separate encrypted local store.
  Its random encryption key lives in the OS credential store. Plaintext
  `whatsapp-rust` SQLite storage is forbidden.
- Protocol-required outbound retry payloads may persist only inside that
  encrypted store, are never user-visible history, and expire within 24 hours.
- A workflow supplies one interpolatable plain-text `message`, limited to 4,096
  Unicode characters after interpolation. There is no splitting or media.
- The configured message template remains in workflow JSON like every generic
  app action. The resolved/interpolated body is not copied into main SQLite,
  run output, logs, analytics, or errors; only the encrypted, expiring protocol
  retry record may retain it temporarily.
- Sends are serialized and locally limited to a burst of five per minute and
  60 per hour. Excess sends fail and are never queued.
- A bounded reconnect may occur when an action begins. Definitive pre-dispatch
  failure is retryable; ambiguous post-dispatch failure is `delivery_unknown`
  and is never retried automatically.
- The connection remains unavailable until an explicit self-test succeeds.
- Before QR pairing, the user must explicitly acknowledge the experimental,
  unofficial, breakage, and account-restriction risk.
- Disconnect always removes local session data. Remote Linked Device logout is
  best-effort; failure produces manual removal instructions.
- WhatsApp is enabled separately per OS only after packaged validation passes
  on that OS. A failed platform gate never blocks Telegram.

## Product outcome

A user opens WhatsApp setup, acknowledges that the integration is unofficial,
scans a short-lived QR code from WhatsApp's Linked Devices screen, and sends an
explicit test notification to their own self-chat. Alfred then restores that
encrypted linked-device session and maintains one background connection while
the app or tray process runs.

Workflows use `whatsapp.send_self_message`. The action returns only a protocol
message ID, submission timestamp, and masked self-destination. Alfred says the
message was submitted to WhatsApp; it does not claim delivery or reading.

Primary dependency reference:

- [`oxidezap/whatsapp-rust`](https://github.com/oxidezap/whatsapp-rust)

## Scope

**In scope**:

- An experimental action-only `whatsapp` provider with
  `linked_device_experimental` mode.
- Mandatory risk acknowledgement and QR pairing for one account.
- A single persistent runtime while Alfred runs, including tray mode.
- Self-JID derivation and a fixed self-chat destination.
- An encrypted, isolated protocol store with a key in the OS credential store.
- Minimal device, Signal, app-state, address-mapping, and bounded encrypted
  outbound-retry state required to stay paired and send.
- `whatsapp.send_self_message`, reconnect state, local throttling, explicit
  test send, safe disconnect/logout, redaction, documentation, and tests.

**Out of scope**:

- Meta Cloud API, WhatsApp Business onboarding, system-user tokens, templates,
  WABAs, business senders, BSPs, and customer messaging.
- Any recipient other than the paired account itself; multiple accounts;
  dynamic phone/JID inputs; contacts; groups; broadcasts; or marketing.
- Incoming messages, chat/history/contact sync as a product feature, triggers,
  replies, read receipts, search, presence, typing, media, calls, newsletters,
  status, profile changes, and group/community operations.
- Offline operation, a relay, a hidden queue, background operation after Alfred
  exits, or automatic retry of an ambiguous send.
- Pair codes, phone-number setup, importing another client's session, or
  exposing raw protocol controls.

## Commands you will need

- `bun test`
- `bun run build:frontend`
- `cargo test --manifest-path src-tauri/Cargo.toml`
- `cargo check --manifest-path src-tauri/Cargo.toml`
- Dependency/license/advisory audits for the exact pinned crate graph.
- The WhatsApp protocol-store, runtime, and mock/contract suites introduced by
  this plan.
- Packaged smoke tests on macOS, Windows, and Linux.

## Implementation steps

### Step 1: Gate the plan with an isolated feasibility and supply-chain spike

Do not build product UI first. Create a disposable, non-shipping Rust spike
against an exact reviewed `whatsapp-rust` version or immutable commit and prove:

1. The dependency compiles with Alfred's Rust toolchain and does not create an
   incompatible `libsqlite3-sys`/Diesel/rusqlite native-link graph.
2. QR pairing succeeds without enabling pair-code or phone-number setup.
3. The authenticated device exposes a stable own JID suitable for self-send.
4. Sending a 1:1 text message to that own JID reaches WhatsApp's “Message
   yourself” chat and returns a stable message ID.
5. The runtime can shut down, restart from durable state, reconnect, send
   again, and remotely log out.
6. Message/history events and lazy history-sync blobs can be ignored without
   decoding or persistence, while retaining only protocol state needed for
   self-send.
7. Sent-message retry data can be encrypted and purged within 24 hours without
   disabling required protocol cleanup.
8. No QR payload, phone number, JID, encryption key, Signal material, message
   body, incoming content, or raw frame appears in default logs or errors.
9. A packaged binary can run the same pair/send/restart/logout path on at least
   one target before the architecture is accepted.

Review the MIT license, crate ownership, release history, build scripts,
features, transitive native dependencies, default logging, and security
advisories. Disable unused default features and all PII/protocol tracing.
Commit a version and `Cargo.lock`; never track a floating branch.

**STOP** if self-send, durable reconnect, encrypted minimal storage, dependency
linkage, redaction, or packaged execution cannot be demonstrated. Record spike
results in this plan before proceeding.

#### Spike results (recorded 2026-08-18)

Spike lives at `spikes/whatsapp-feasibility/` with its own committed
`Cargo.lock`, pinned to `whatsapp-rust =0.7.0` (published 2026-08-07, MIT). It
is outside the Tauri build graph. Its `README.md` holds the full evidence.

| # | Gate | Result |
|---|------|--------|
| 1 | Toolchain + native-link graph | **GREEN, constrained** — see below |
| 2 | QR pairing without pair-code | **GREEN** — paired 2026-08-18 |
| 3 | Stable own JID | **GREEN** — PN and LID both available |
| 4 | Self-send returns a message ID | **GREEN** — 3 sends, distinct IDs |
| 5 | Restart, reconnect, resend, logout | **GREEN** with one caveat, see below |
| 6 | Ignore message/history-sync events | **GREEN** |
| 7 | Encrypt + purge retry data | PARTIAL — knobs exist, blocked on Step 2 |
| 8 | No PII in default logs | **RED** — mitigable, see below |
| 9 | Packaged binary runs the path | PARTIAL — release binary builds and runs |

**Gate 1 — two default features must be disabled or Alfred will not build.**

- `sqlite-storage` pulls Diesel → `libsqlite3-sys 0.37`. Alfred's
  `rusqlite 0.40.2` pulls `libsqlite3-sys 0.38.2`. Both declare
  `links = "sqlite3"`, which Cargo rejects with a hard resolver error. This is
  exactly the linkage risk the gate was written to catch.
- `simd` gates `#![feature(portable_simd)]` in `wacore-binary`. Nightly only;
  Alfred builds on stable `rustc 1.96.0`.

The pinned shipping configuration is therefore:

```toml
whatsapp-rust = { version = "=0.7.0", default-features = false, features = [
    "tokio-transport",
    "tokio-runtime",
    "tokio-native",
    "ureq-client",
] }
```

Verified: that config `cargo check`s clean beside `rusqlite 0.40.2 + bundled`,
leaves exactly one `libsqlite3-sys` (0.38.2, Alfred's) in the graph, and adds
183 packages with **zero** `-sys`/native-link crates of its own. `signal` is
dropped because Tauri already owns Alfred's shutdown path. `cargo audit` is
clean against 1,217 advisories on both the shipping and spike lockfiles. All
`whatsapp-rust` workspace crates are MIT, compatible with GPL-3.0-or-later.
`tracing-pii`, `danger-skip-tls-verify`, `danger-skip-cert-chain-verify`,
`debug-snapshots`, `legacy-session-interop`, `metrics`, `plugins`, and `voip*`
are off by default and stay off.

`ureq-client` adds a second HTTP stack next to Alfred's `reqwest`. It is
replaceable through `with_http_client`; consolidating is a Step 5 follow-up,
not a gate.

**Gate 6 — GREEN.** `BotBuilder::skip_history_sync()` declines the stream at
the protocol level, so blobs are never received rather than received and
dropped. Registering no `on_message` handler keeps inbound content entirely
unobserved.

**Gate 7 — partial.** `BotBuilder::with_resend_rate_limit(burst, refill_per_min)`
and `Client::set_retry_admission(..)` exist, so Step 6's 5/min cap can lean on
the library. Upstream `examples/retry_quarantine.rs` and
`examples/durability_hook.rs` are the references for bounding retention.
Encryption and 24-hour expiry depend on the Step 2 decision below.

**Gates 2 and 3 — GREEN.** A live account paired by QR on 2026-08-18 with no
pair-code or phone-number path enabled. `Client::pn()` and `Client::lid()` both
return a usable JID immediately after the post-pairing 515 reconnect, so the
self-chat destination can be derived entirely backend-side.

**Gate 4 — GREEN.** Three self-sends to `Client::pn().to_non_ad()` each returned
a distinct stable protocol message ID (`3EB0…`-form) and landed in the account's
own “Message yourself” chat. The action surface needs nothing beyond the ID, a
timestamp, and the masked destination.

**Gate 5 — GREEN, with one untested branch.** Proven end to end: process exit,
cold restart, session restored from durable state with **no** QR prompt,
reconnect, and repeat sends. A revoked session was also exercised — the server
answered `<failure reason="401"/>`, the client surfaced it as logged-out, and the
runtime refused to proceed instead of silently starting a new pairing flow. That
is precisely the `relink_required` behaviour Step 5 specifies.

Untested: a *successful* remote `logout()` against a still-valid session. The
account was already revoked phone-side before that call, so only the failure path
ran. Step 7 must verify the happy path separately.

The failure path did expose a defect worth carrying into Step 7: the first
implementation propagated the connect error and skipped local deletion entirely,
leaving the session database on disk. Local deletion must be unconditional. After
the fix, `logout` removed all three files (`.db`, `-wal`, `-shm`) despite the
remote call being impossible.

**Gate 8 — RED, but mitigable. Read this before Step 5.**

The *send* path is clean: at default `info`, three sends produced no message
body, no phone number, and no JID in the output. The problem is elsewhere.

At default `info`, during **pairing**, `whatsapp-rust` prints the linked
account's full E.164 phone number and full LID:

- `src/pair.rs:435` — `"Added own LID-PN mapping to cache: {} <-> {}"` emits the
  raw LID and the raw phone number.
- `src/handlers/notification/device.rs:660` —
  `"Updated own device list from account_sync: {} devices (user: {})"` emits the
  raw LID.

Worse, and not level-gated: a **`WARN`** from `wacore_libsignal::protocol::session_cipher`
emits the raw LID *and Signal ratchet/base key material* —

```
WARN wacore_libsignal::protocol::session_cipher] Failed to decrypt PreKey
message with ratchet key: <64 hex> and counter: 4. Session loaded for
<raw-lid>@lid.0. Local session has base key: <64 hex> …
```

A `WARN` passes any sane default filter, so lowering the level is not a defence.
At `debug`, five targets leak identity: `Client/Recv`, `Client/Send`,
`whatsapp_rust::client::sessions`, and `wacore_libsignal::protocol::session_cipher`.

The crate has its own redaction helper and uses it elsewhere (`pair.rs:483` logs
`jid.observe()`, hashing to `pn#<digest>:18@…`), so these are inconsistencies
rather than a design position. Twenty-one `info!`/`warn!` sites interpolate a
JID-ish value in total.

As written this trips the plan's STOP conditions on full identity *and* Signal
material reaching logs. It does **not** kill the plan, because the crate logs
through the `log` facade. Step 5 must therefore:

- filter by **target**, not by level — suppress `whatsapp_rust::*`, `wacore*::*`,
  and the bare `Client/*` targets before any client is constructed;
- never install a permissive global logger while a WhatsApp client exists;
- carry a test asserting no such target reaches Alfred's sinks;
- re-run this scan after every dependency bump. These are upstream log strings
  that can move without a semver signal.

**Gate 9 stays PARTIAL.** The release binary builds and runs the CLI, but the
packaged pair/send/logout path per OS is still outstanding.

Operational note: the spike stores its session at
`~/.cache/alfred-whatsapp-spike/session.db`, deliberately outside the repository.
An in-tree session database was destroyed by a branch switch during this spike,
which cost a pairing and orphaned a linked device that had to be removed by hand.

### Step 2: Design the encrypted protocol-store boundary

The default plaintext `SqliteStore` is not acceptable. **The spike resolved this
choice: the SQLCipher option is not viable and the Alfred-owned backend is the
only remaining path.**

- ~~a reviewed SQLCipher-backed dedicated database~~ — rejected.
  `whatsapp-rust-sqlite-storage` is Diesel on `libsqlite3-sys 0.37`, which
  cannot coexist with Alfred's `libsqlite3-sys 0.38.2` in one binary (see the
  Step 1 spike results). Swapping in SQLCipher does not change that.
- an Alfred-owned implementation of the required `whatsapp-rust` storage
  traits using authenticated encryption for every sensitive value and keyed
  digests for sensitive lookup identifiers. **This is the chosen path.**

Sizing measured against `wacore 0.7.0`: the backend must implement **82 trait
methods** — `SignalStore` (25), `ProtocolStore` (36), `AppSyncStore` (10),
`DeviceStore` (6), `MsgSecretStore` (5). The upstream Diesel reference
implementation is ~3,800 lines. Re-estimate this plan's XL effort against that
number before starting product work; it is the single largest cost in 023.

Use established, audited AEAD primitives with versioned envelopes, random
nonces, integrity-protected associated data, and zeroized key buffers. Generate
a random store key during pairing and put it in the OS credential store under
an opaque reference. Never derive it from a phone number, JID, device ID, or
user password.

Keep the store in a provider-specific app-data directory with owner-only file
permissions. Do not place its tables in Alfred's main `app.db`. Do not copy it
into logs, diagnostics, exports, artifacts, or unencrypted backups.

Persist only what the protocol demonstrably requires:

- linked-device identity and registration material;
- Signal sessions/prekeys/sender keys;
- minimal app-state and PN/LID mappings needed for self-send;
- the encrypted own JID;
- encrypted outbound retry records with creation/expiry timestamps.

Do not persist conversations, inbound messages, decoded history-sync blobs,
contacts, media, profiles, search indexes, or product analytics. Purge expired
retry records at startup, after sends, and on a bounded maintenance interval.
The hard maximum retention is 24 hours.

**Verify**: corruption, wrong/missing key, migration, crash recovery, atomic
creation, permissions, backup/temporary-file behavior, expiry, zeroization,
and deletion tests. A raw-file scan with sentinel values must find no plaintext
JID, phone number, message, or cryptographic fixture.

#### Step 2 build status (2026-08-18)

Landed in `src-tauri/src/integrations/whatsapp/`:

- `crypto.rs` — `StoreKey` (random, zeroized on drop, no `Debug`/`Clone`/
  `Serialize`), ChaCha20-Poly1305 sealing under a versioned `version || nonce ||
  ciphertext` envelope, HMAC-SHA256 keyed lookup digests, and per-purpose
  subkeys so the AEAD key and the index key are independent. AAD binds every
  value to its `(namespace, key digest)` row, so a ciphertext moved between rows
  or tables fails to open.
- `schema.sql` — a dedicated database, separate from `app.db`. Every key column
  is a digest and every value column an envelope. Namespace strings and
  retention timestamps are the only plaintext, because the expiry sweeps must
  range over them.
- `store.rs` — `EncryptedProtocolStore` implementing all five `wacore` traits
  (`SignalStore`, `AppSyncStore`, `ProtocolStore`, `MsgSecretStore`,
  `DeviceStore`) over Alfred's existing `rusqlite`, plus `delete_files` for the
  Step 7 disconnect path and `purge_expired` for the maintenance sweep.

Dependencies added to `src-tauri`: `whatsapp-rust =0.7.0` in the Step 1 pinned
configuration, `wacore =0.7.0` and `wacore-appstate =0.7.0` (both
`default-features = false` — their default `simd` feature needs nightly),
`chacha20poly1305 0.11`, `hmac 0.12`, `async-trait`, and `bytes`. Alfred's own
`cargo check` and full test suite pass with all of them.

Privacy decisions taken in the implementation:

- The inbound durability buffer is **not** implemented, so `wacore`'s defaults
  fail closed and no inbound message content can ever be persisted.
- Group metadata persistence is **not** implemented, so it stays a no-op.
- `DeviceStore::create` always returns device id 1: one linked account per
  installation, enforced in the store rather than only in the UI.
- `delete_expired_sent_messages` clamps the caller's cutoff to a hard 24-hour
  ceiling, so no caller can widen retry retention beyond the plan's limit.

**Verified** by 36 tests: seal/open roundtrip, ciphertext never contains the
plaintext, random nonce per seal, wrong key, wrong AAD, cross-row replay,
bit-flip tampering, truncation, unknown envelope version, malformed key
material, digest determinism/namespacing/key-binding, composite-digest
rearrangement, per-trait CRUD, pre-key `MAX(id)` and update-not-upsert upload
marking, LID/PN bidirectional lookup, single-use retry take, the 24-hour clamp,
msg-secret expiry merge rules, tc-token dual-bucket expiry, reopen durability,
owner-only `0600` permissions, sidecar deletion, and a raw-file sentinel scan
across `.db`/`-wal`/`-shm`/`-journal` proving no plaintext JID, phone number,
LID, or message body survives.

- `keyring.rs` — store-key custody through Alfred's existing `TokenStore`.
  `provision()` mints a random key under an **opaque** `whatsapp-protocol-store/
  <uuid>` reference; the reference is never derived from a phone number, JID,
  device id, or password, so the credential entry reveals nothing about which
  account is linked. `delete()` is idempotent because disconnect may run twice,
  and `store_path()` places the database in a provider-specific app-data
  sub-directory, never beside `app.db`.

**Still open for Step 2**: the startup/interval purge scheduler (its natural home
is the Step 5 runtime owner, which calls the existing `purge_expired`) and an
explicit corrupted-file recovery path. Migration tests are not yet meaningful —
the schema is at v1.

Gates run: `cargo test` 208 passed, `cargo clippy` clean on the new module,
`git diff --check` clean. Note for whoever runs the suite next: the pre-existing
`integrations::oauth_native` tests bind real localhost ports and flake against
each other under parallel execution — unrelated to this work.

### Step 3: Register the provider and one-account contract

Add `whatsapp` to `ProviderCatalog`, not `AgentProviderId`, with mode
`linked_device_experimental`, action-only capability copy, and no event
descriptors. Expose it only on OS targets whose packaged gate has passed.

Enforce one connection in Rust. The public connection row may contain only an
experimental badge, a non-sensitive display label, and a masked account suffix.
Backend-only metadata may contain store version, runtime state, and masked
identity. The store key, full own JID, device identity, and protocol material
remain behind the credential/protocol-store boundary.

Build the canonical identity from provider, mode, and authenticated own JID
before discarding plaintext setup identity. Never store a full phone number or
JID in Alfred's main SQLite database or command DTOs.

Generalize the shared `delivery_unknown` copy so it does not name Slack.

**Verify**: catalog, one-account, DTO, platform-availability, identity-digest,
and source/database serialization tests.

#### Step 3 build status (2026-08-19)

- `integrations/whatsapp/provider.rs` — `PROVIDER_ID`/`CONNECTION_MODE`
  constants, `identity_key()` built through the existing
  `canonical_identity_key(provider, mode, own_jid)`, `masked_account()`,
  `display_name()`, backend-only `provider_metadata` keys, and the
  risk-acknowledgement version Step 4 will record.
- `integrations/catalog.rs` — `whatsapp` registered with mode
  `linked_device_experimental`, action-only copy, and no event descriptors.
  `AppProviderDto` gained `experimental` and `single_connection`; both default
  to `false`, so no other provider changed behaviour.
- `db/app_connections.rs` — `upsert_app_connection` refuses a second account for
  a single-connection provider while still allowing the same account to relink.
  Enforced in Rust, not merely in the connect UI.
- Frontend contract (`types.ts`, `connected-apps-settings.tsx`) carries the two
  new fields; the unknown-provider fallback still renders providers whose
  integration is missing or newer than the app.

Platform gate: `provider::is_available()` is `PACKAGED_GATE_PASSED ||
cfg!(debug_assertions)`. Every OS constant is currently `false`, so release
builds hide WhatsApp everywhere; development builds expose it so the remaining
steps can be built. Flip a target's constant only when this plan records a green
packaged smoke for that OS.

Logo: `src/assets/apps/whatsapp.svg` (1.3 KiB, local, no remote assets) with an
`APP_LOGOS` entry. Recolored to WhatsApp's darker brand green `#128c7e` rather
than `#25d366`: measured against Alfred's four logo surfaces (`#ffffff`,
`#e8eee9`, `#232b2e`, `#12181a`), `#25d366` scores 1.98:1 and 1.68:1 on the
light theme, while `#128c7e` clears 3:1 on all four. Recoloring preserves brand
identity, so `requiresSurface` is not needed.

The shared `delivery_unknown` copy already reads "The provider may have accepted
this action. Check the target before retrying." in both `actions.rs` and
`store.ts` — it never named Slack, so no change was required.

Gates: `cargo test` 222 passed, `bun test` 105 passed, `bun run build:frontend`
green, `cargo clippy` clean across the WhatsApp module, `git diff --check` clean.

### Step 4: Build mandatory acknowledgement and QR pairing

The connect flow is:

1. Show an **Experimental / unofficial** explanation stating that protocol
   changes may break the integration and WhatsApp may restrict or suspend the
   linked account.
2. Require an explicit acknowledgement before starting the runtime or showing
   QR material. Store only the acknowledgement version and timestamp.
3. Generate a staging store key, create a staging encrypted protocol store,
   and start a single pairing runtime.
4. Stream each short-lived QR payload to the open modal only. Render it in
   memory; never persist, log, include in analytics, or reuse an expired code.
5. Tell the user to scan through **WhatsApp → Linked Devices → Link a device**.
6. On connection, derive and validate the authenticated own JID. Do not ask for
   or accept a phone number/JID from React.
7. Show a masked self-destination and require an explicit test message.
8. Mark the connection ready and promote staging state only after the test
   succeeds definitively.

On cancellation, expiry, pairing replacement, test failure, or modal close,
stop the staging runtime, attempt remote logout if pairing completed, and
delete staging database and key. If the test outcome is ambiguous, explain that
the message may have appeared but do not create a ready connection.

**Verify**: acknowledgement versioning, QR expiry/replacement, no QR logs,
cancel at every state, duplicate account rejection, invalid/self-JID failure,
ambiguous test, cleanup, keychain failure, and complete redaction.

#### Step 4 build status (2026-08-19) — backend complete, UI outstanding

`integrations/whatsapp/runtime.rs` introduces the boundary Steps 4 and 5 share.
The protocol client exists in exactly one place and is never returned, cloned
out, or reachable from a command: `RuntimeHandle` exposes only own-JID, send to
self, logout, and shutdown. `RuntimeLauncher` lets the pairing state machine and
(next) the runtime owner be driven by a scripted `FakeRuntime`, so every path
below is tested with no live account, no network, and no committed device state.

`WhatsAppLauncher` is the real implementation, compiled against
`whatsapp-rust 0.7.0`: `skip_history_sync()`, `with_resend_rate_limit(5, 5)`, no
`on_message` handler anywhere, and a `classify_send_error` that is deliberately
conservative — only `InvalidRequest` (never sent) and `Iq` (the pre-send device
query) are retryable; `Client` and `Internal` become `DeliveryUnknown` because
the stanza may already be on the wire. Retrying those is a STOP condition.

`integrations/whatsapp/pairing.rs` is the state machine:
`Starting → AwaitingScan → AwaitingTest → Ready`, plus `Failed`/`Cancelled`.

- `acknowledge()` is the gate. A missing or stale acknowledgement version, or an
  account that is already linked, is refused **before** a key is minted or a
  staging store is created.
- Each QR payload goes to a `QrSink` and nowhere else; a new code calls
  `expire()` on the previous one first, so a superseded payload can never be
  re-presented. No state variant carries a payload, and nothing logs one.
- The own JID is read from the authenticated client. `validate_own_jid` accepts
  only `s.whatsapp.net` and `lid`, so a group, broadcast, or newsletter
  identifier can never become the self-chat destination.
- `send_test()` moves to `Ready` only on a definitive success. An ambiguous
  outcome lands in `Failed { code: "test_delivery_unknown" }` and `finish()`
  still refuses, so no connection exists that has not passed a real test.
- `cancel()` stops the runtime, attempts a remote logout only if the device
  actually linked, then deletes the staging database and the staging key
  regardless of that outcome. It is idempotent.
- `finish()` promotes staging to the final store by rename and returns only the
  identity digest, masked account, credential reference, store path, and
  acknowledgement timestamp.

Verified by 70 module tests, 12 of them covering this step: acknowledgement
versioning, nothing provisioned before acknowledgement, duplicate-account
refusal, link-without-test, ambiguous test, QR supersession counting, revoked
session, group identity refusal, cleanup with and without a prior link, double
cancel, failed launch, promotion, and a redaction sweep asserting no state
variant renders a raw JID or a QR payload.

`PairingPaths` is injected rather than resolved internally, so tests never touch
the real app-data directory. That was not cosmetic: the first version resolved
paths internally and the parallel test run raced on one shared staging database,
crashing the test binary with SIGBUS.

`integrations/whatsapp/service.rs` holds at most one attempt and translates it
into DTOs: `WhatsAppPairingStateDto` (state code, masked account, failure code),
`WhatsAppTestSendDto`, and `WhatsAppQrDto`. `begin_pairing` cancels any previous
attempt first, so a reopened modal can never leave a second runtime or a stale
staging store behind.

**The QR is rendered to SVG in Rust**, not in the frontend. The scannable
payload therefore never exists as a JavaScript string, and no QR library was
added to the frontend bundle. A payload that cannot be rendered is dropped
rather than falling back to shipping the raw text. Tests assert the SVG contains
no `<script>`, `<image>`, `xlink:href`, or remote URL, so it satisfies the app
window's CSP.

Commands: `begin_whatsapp_pairing`, `whatsapp_pairing_state`,
`send_whatsapp_pairing_test`, `complete_whatsapp_pairing`,
`cancel_whatsapp_pairing`. Events: `whatsapp://qr` and `whatsapp://qr-expired`.

`whatsapp-connect.tsx` is the modal: the risk warning and an explicit
acknowledgement checkbox gate the "Link a device" button, the QR panel names the
**WhatsApp → Linked Devices → Link a device** path, the test-message step shows
the masked account, and closing the modal always cancels through Rust. It reuses
the existing connect-modal class vocabulary (`schedule-modal-body`,
`app-action-warning`, `field`, `primary`/`ghost`,
`connection-tutorial-inline-error`, `telegram-pairing-card`) rather than
inventing a parallel set; only the experimental badge, the acknowledgement row,
and the QR plate needed new rules, all built from existing tokens. The QR plate
is deliberately opaque white — a transparent QR inverts in the dark theme and
stops scanning.

Gates: `cargo test` 246 passed, `bun test` 105 passed,
`bun run build:frontend` green, `cargo clippy` clean across the WhatsApp module,
`git diff --check` clean.

### Step 5: Own one persistent runtime for the Alfred lifecycle

Create one backend runtime owner, separate from app-action requests. If a ready
WhatsApp connection exists, unlock its store and start one client during Alfred
startup. Keep it alive while the app or tray process runs; shut it down and
flush required encrypted state during orderly application exit.

The owner exposes only bounded capabilities: status, reconnect, send-to-self,
and logout. It does not expose the raw client to commands or workflow code.
Serialize lifecycle transitions and sends so pairing, reconnect, send, logout,
and shutdown cannot race.

Do not register inbound-message handlers. If protocol events must be observed
for connection health, match only safe event types and discard message,
history, contact, media, call, presence, and profile payloads immediately.
Never emit them through Tauri or store them in Alfred state.

Expose safe states such as `connecting`, `connected`, `reconnecting`,
`relink_required`, `error`, and `stopped`. A logged-out/401 session becomes
`relink_required` and never silently starts a new pairing flow.

**Verify**: startup, tray lifetime, graceful and forced shutdown recovery,
socket loss/backoff, network transitions, stream replacement, revoked session,
locked/missing keychain, corrupted store, concurrent actions, reconnect/send
races, and zero inbound-content propagation.

#### Step 5 build status (2026-08-19) — backend complete

**Gate 8 from Step 1 is now mitigated.** `integrations/whatsapp/log_guard.rs`
drops every record whose target starts with `whatsapp_rust`, `wacore`,
`waproto`, or `Client/` before it can reach a sink, and `install_silent()` runs
first in Tauri's `setup` — before any client can exist. Filtering by *target* is
the point: the worst record (`wacore_libsignal::protocol::session_cipher`,
carrying the raw LID plus ratchet and base keys) is a `WARN`, which passes every
sane level filter.

Alfred installs no logger today, so those calls were already no-ops — but that
safety was accidental. Adding `tauri-plugin-log` for debugging would have
started writing Signal key material to a file. The guard makes the policy
explicit, testable, and the mandatory path for any future logger. Six tests
cover it, including one asserting each exact leak site found in the spike.

`integrations/whatsapp/owner.rs` owns the one runtime for Alfred's lifetime:

- `start()` unlocks the encrypted store, purges expired retry payloads, and
  launches exactly one client — stopping any previous one first, so two can
  never coexist.
- One async lock serializes every lifecycle transition and every send, so
  pairing, reconnect, send, logout, and shutdown cannot interleave.
- Safe states only: `stopped`, `connecting`, `connected` (masked account),
  `reconnecting`, `relink_required`, `error` (stable code).
- The event pump matches **only** lifecycle variants. No message, history,
  contact, media, call, presence, or profile payload is observed, forwarded, or
  stored, because the runtime never emits one.
- A remote unlink, or an unexpected pairing code during normal operation, moves
  to `relink_required` and stops. It never silently starts a pairing flow, and
  the state survives shutdown so a restart cannot re-pair by accident.
- `send_self_message` makes one bounded reconnect when disconnected, but refuses
  outright once `relink_required`.

Wiring: startup spawns `start_stored_runtime` (non-fatal — a missing, revoked,
or broken connection leaves an error state the UI can show, and never blocks
Alfred from launching); `RunEvent::Exit` calls `shutdown_runtime`. Completing a
pairing hands the account straight to the owner, so no restart is needed.
Commands added: `whatsapp_runtime_status`, `reconnect_whatsapp_runtime`.

A defect worth recording: the event pump originally updated a **detached copy**
of the status, so `Connected` and `LoggedOut` never reached callers — the runtime
would have looked permanently `connecting`. All eleven tests passed anyway,
because none asserted that an event changed the owner's own status. The status is
now a shared `Arc<Mutex<_>>` and three regression tests cover the connect,
remote-unlink, and stray-QR transitions.

**Still open for Step 5**: the bounded maintenance-interval purge (startup purge
is done), and the socket-loss/backoff and network-transition cases, which need a
live account rather than the scripted fake. The status/reconnect **UI** belongs
to Step 7.

Gates: `cargo test` 266 passed, `cargo clippy` clean across the WhatsApp module,
`cargo build` green.

### Step 6: Register `whatsapp.send_self_message`

Register through Plan 009:

- `whatsapp.send_self_message`
  - `message`: required textarea, interpolation enabled.

The descriptor has no recipient, phone, JID, contact, account, or group field.
After interpolation, validate non-empty plain text and a maximum of 4,096
Unicode characters. Send to the runtime's authenticated own JID only.

Serialize sends and apply an in-memory limiter:

- maximum burst: five sends per minute;
- rolling cap: 60 sends per hour;
- no queue and no detached execution.

When disconnected, make one bounded reconnect attempt inside the action's
existing deadline. A failure known to occur before dispatch is provider
unavailable and may be attempted again later. A timeout, socket loss, or
shutdown after possible dispatch is `delivery_unknown`; never resend it
automatically.

Return only schema version, sanitized protocol message ID, submission
timestamp, and masked self-destination. Never return message content, own JID,
phone number, raw frame, provider object, receipt details, or session state.

**Verify**: success, empty/oversized text, both local rate limits, disconnected
reconnect success/failure, revoked session, cancellation, ambiguous dispatch,
concurrent serialization, malformed IDs, store failures, retry expiry, and
redaction across errors, logs, outputs, and persistence.

### Step 7: Integrate status, reconnect, and disconnect UI

Route the WhatsApp modal through the provider connect-handler registry. Show
an **Experimental** badge everywhere the provider is presented. The row copy
is: “Send plain-text notifications to your own WhatsApp chat while Alfred is
running.”

Show masked identity and safe runtime status. Reconnect starts a bounded
runtime reconnect; `relink_required` starts the acknowledged QR flow and never
accepts imported session data.

Disconnect behavior is fixed:

1. Block new sends and stop the runtime.
2. Attempt a bounded `logout()` to remove Alfred from Linked Devices.
3. Delete the encrypted protocol database, temporary/journal files, store key,
   and local connection metadata regardless of remote logout success.
4. If remote logout failed, instruct the user to remove Alfred manually under
   WhatsApp's Linked Devices screen.
5. If local deletion fails, retain only revoked recovery metadata and offer an
   explicit retry/local-cleanup path.

**Verify**: all safe states, warning visibility, one-account behavior, no
recipient field, reconnect/relink, workflow dependency warning, successful and
offline disconnect, manual unlink guidance, deletion failures, and no event UI.

### Step 8: Document risk, privacy, and operating boundaries

Add `docs/whatsapp.md` and link it from Connected Apps documentation. State:

- the integration is experimental and unofficial;
- account restriction/suspension and protocol breakage are possible;
- it sends only to the paired account's self-chat;
- it works only while Alfred is running, including tray mode;
- exactly what encrypted protocol state and retry data are stored;
- outbound retry payloads expire within 24 hours;
- no inbound content, history, contacts, media, triggers, or offline queue is
  retained or exposed;
- how reconnect, relink, ambiguous delivery, throttling, disconnect, and manual
  Linked Device removal work.

Do not describe the protocol as an API offered by Meta or suggest that Alfred
can guarantee account safety.

## Test and release plan

- Use deterministic fake runtime/store implementations for automated tests;
  never commit live device state, QR payloads, JIDs, phone numbers, Signal
  material, or message bodies.
- Add storage fixtures proving encryption, integrity failure, expiry, cleanup,
  and plaintext absence in database, WAL, journal, temp, crash, and backup files.
- Add lifecycle tests for pair/start/restart/reconnect/send/logout/shutdown and
  every race between them.
- Add command, DTO, connection-store, log, error, analytics, and run-output
  scans with sentinel QR, JID, phone, key, resolved retry message, and inbound
  values. Account for the intentional configured template in workflow JSON.
- Run full frontend and Rust test/build gates plus dependency/license/advisory
  review for the exact lockfile.
- Manually run QR pair, self-test, workflow send, scheduled tray send, restart,
  network loss/recovery, remote unlink, offline disconnect, and cleanup in a
  packaged build on macOS, Windows, and Linux.
- Enable the provider separately on each OS only after its packaged smoke
  passes. Keep it hidden or unavailable on failed/unvalidated targets.

## Done criteria

- [ ] Plan 021 ships first and the Step 1 feasibility spike is recorded green.
- [ ] WhatsApp is visibly experimental/unofficial and pairing requires explicit
      risk acknowledgement.
- [ ] Exactly one QR-linked account is supported per installation.
- [ ] The only destination is the authenticated account's own self-chat.
- [ ] A successful explicit self-test is required before readiness.
- [ ] One runtime remains connected only while Alfred or its tray process runs.
- [ ] Session state is encrypted with a key held in the OS credential store;
      plaintext default storage is not used.
- [ ] No incoming messages, history, contacts, media, triggers, or raw protocol
      surface is retained or exposed.
- [ ] Encrypted retry payloads expire within 24 hours.
- [ ] The action accepts only one interpolatable 4,096-character plain-text
      message and exposes no recipient control.
- [ ] Sends are serialized and capped at five per minute and 60 per hour.
- [ ] No queue or automatic retry after ambiguous dispatch exists.
- [ ] Disconnect always removes local session data and attempts remote logout.
- [ ] Each enabled OS has passed its own packaged smoke gate.
- [ ] QR material, keys, Signal state, full JIDs/phone numbers, inbound content,
      retry messages, resolved outgoing bodies, and raw frames never leak into
      plaintext connection persistence, DTOs, logs, analytics, errors, or
      outputs. The configured workflow template is the sole intended plaintext
      persistence exception.

## STOP conditions

- The exact pinned dependency cannot self-send, restore a session, reconnect,
  log out, or build safely with Alfred's native dependency graph.
- Product asks to hide the unofficial/account-risk warning.
- Product asks to send to another person, contact, group, dynamic JID/phone, or
  more than one account under this plan.
- The implementation requires plaintext session/retry storage, unbounded retry
  retention, decoded history, stored inbound content, PII tracing, or raw-client
  exposure.
- A recipient/JID/phone field is added to workflow configuration.
- The runtime continues after Alfred exits or adds an offline/relay queue.
- A possibly dispatched send is retried automatically.
- QR payloads, store keys, device/Signal material, full identity, resolved
  message bodies, incoming content, or raw frames enter main connection SQLite,
  frontend state beyond the live QR, logs, analytics, errors, or outputs. The
  configured workflow template is the sole intended persistence exception.
- The dependency is updated without repeating the protocol, storage, logging,
  supply-chain, and packaged-platform review.

## Maintenance notes

- Pin an exact reviewed release or immutable commit and keep `Cargo.lock`
  reviewable. Never follow `main`.
- Treat every dependency upgrade as a security/protocol migration, not routine
  semver maintenance.
- Re-run self-send, persistence, redaction, and packaged OS gates before each
  Alfred release that changes the dependency or store.
- Keep official WhatsApp Business Platform support, other recipients, inbound
  messaging, richer content, and managed/cloud operation in separate plans.
