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
- **Implementation status**: TODO

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

### Step 2: Design the encrypted protocol-store boundary

The default plaintext `SqliteStore` is not acceptable. Choose the storage
implementation only after the spike compares:

- a reviewed SQLCipher-backed dedicated database that coexists safely with
  Alfred's current SQLite linkage on every target; or
- an Alfred-owned implementation of the required `whatsapp-rust` storage
  traits using authenticated encryption for every sensitive value and keyed
  digests for sensitive lookup identifiers.

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
