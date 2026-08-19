# Plan 023 Step 1 — WhatsApp feasibility spike

Disposable, non-shipping crate. It is **not** in the Tauri build graph and must
never be linked into Alfred. Its only job is to answer the nine gating
questions in `plans/023-whatsapp-linked-device-self-notifications.md` Step 1.

Pinned dependency: `whatsapp-rust =0.7.0` (published 2026-08-07), MIT.
`Cargo.lock` is committed. Never move this to a floating branch.

## Running it

```
cargo run -- pair          # link a device by QR, print the masked own JID
cargo run -- send "hello"  # restore the session and self-send
cargo run -- logout        # remote logout, then delete every local session file
```

`pair` needs a phone with WhatsApp installed. It prints a QR to the terminal;
scan it under **WhatsApp → Linked Devices → Link a device**.

## Findings

### 1.1 Dependency graph and toolchain — GREEN, with two mandatory constraints

Two default features are unusable in Alfred and **must** be disabled:

| Default feature  | Why it cannot ship                                              |
|------------------|-----------------------------------------------------------------|
| `sqlite-storage` | Pulls Diesel → `libsqlite3-sys 0.37`. Alfred's `rusqlite 0.40.2` pulls `libsqlite3-sys 0.38.2`. Both declare `links = "sqlite3"`, which Cargo rejects outright. |
| `simd`           | `wacore-binary` gates `#![feature(portable_simd)]` behind it. Nightly only; Alfred builds on stable (`rustc 1.96.0`). |

The conflict is a hard resolver error, not a warning:

```
package `libsqlite3-sys` links to the native library `sqlite3`, but it
conflicts with a previous package which links to `sqlite3` as well:
package `libsqlite3-sys v0.38.2`
```

The shipping configuration is therefore:

```toml
whatsapp-rust = { version = "=0.7.0", default-features = false, features = [
    "tokio-transport",
    "tokio-runtime",
    "tokio-native",
    "ureq-client",
] }
```

Verified: that config `cargo check`s clean in a crate that also depends on
`rusqlite 0.40.2` with `bundled`, leaving exactly one `libsqlite3-sys` (0.38.2,
Alfred's) in the graph. It adds 183 packages and **zero** `-sys`/native-link
crates of its own — the whole tree is pure Rust.

`signal` is intentionally dropped from the shipping set: it only installs a
Ctrl+C/SIGTERM handler, and Tauri already owns Alfred's shutdown path.

Dangerous features confirmed **off** by default and left off: `tracing-pii`,
`danger-skip-tls-verify`, `danger-skip-cert-chain-verify`, `debug-snapshots`,
`legacy-session-interop`, `metrics`, `plugins`, `voip*`.

Licenses: `whatsapp-rust`, `wacore`, `wacore-binary`, `wacore-libsignal`,
`wacore-noise`, `wacore-appstate`, `waproto`, and the three pluggable backend
crates are all MIT — compatible with Alfred's GPL-3.0-or-later.

`ureq-client` adds a second HTTP stack next to Alfred's `reqwest`. It is
replaceable via `with_http_client`; folding it onto `reqwest` is a Step 5
follow-up, not a Step 1 blocker.

### 1.6 Ignoring history sync — GREEN

`BotBuilder::skip_history_sync()` declines the stream at the protocol level, so
blobs are never received rather than received-and-dropped. This spike also
registers no `on_message` handler at all, so no inbound content is ever
observed, decoded, or persisted.

### 1.7 Retry payload control — partial, feeds Step 2

`BotBuilder::with_resend_rate_limit(burst, refill_per_min)` and
`Client::set_retry_admission(..)` exist, so Plan 023's 5/min cap can lean on the
library instead of reimplementing it. The upstream `examples/retry_quarantine.rs`
and `examples/durability_hook.rs` are the references for bounding retry
retention. Encrypting and expiring those payloads still depends on the Step 2
storage decision below.

### Step 2 storage sizing — the plan's SQLCipher option is not viable

`whatsapp-rust-sqlite-storage` cannot be reused with SQLCipher: it is Diesel on
`libsqlite3-sys 0.37`, which cannot coexist with Alfred's 0.38.2 (see 1.1).
That leaves the plan's second option — an Alfred-owned backend implementing the
`wacore::store::traits` surface:

| Trait           | Methods |
|-----------------|---------|
| `SignalStore`   | 25      |
| `AppSyncStore`  | 10      |
| `ProtocolStore` | 36      |
| `DeviceStore`   | 6       |
| `MsgSecretStore`| 5       |
| **Total**       | **82**  |

The upstream Diesel reference implementation is ~3,800 lines. Step 2 should be
re-estimated against that number before any product work starts.

### 1.2–1.5 — GREEN (run against a live account 2026-08-18)

Paired by QR with no pair-code path enabled. `Client::pn()` and `Client::lid()`
both resolve straight after the post-pairing 515 reconnect. Three self-sends to
`pn().to_non_ad()` returned distinct `3EB0…` message IDs and landed in the
account's own chat. A cold restart restored the session from disk with no QR
prompt and sent again. A revoked session surfaced as `<failure reason="401"/>`
and the client refused to proceed rather than silently re-pairing.

Not covered: a *successful* remote `logout()`. The account was already revoked
phone-side when that ran, so only the failure branch executed.

### 1.8 — RED, mitigable

The send path is clean at `info`: no body, no phone number, no JID. Pairing is
not. See the plan for the full write-up and the required Step 5 mitigation; the
short version is that `wacore_libsignal::protocol::session_cipher` emits raw LID
plus Signal ratchet/base key material at **`WARN`**, so filtering has to be by
log *target*, not by level.

Reproduce with a sentinel body:

```
RUST_LOG=debug cargo run -- send "SENTINEL-BODY-9f3a" 2>&1 | tee spike.log
```

then scan `spike.log` for the sentinel and for any run of 10+ digits. Delete the
log afterwards — at `debug` it contains real identity and key material.

`mask()` in `src/main.rs` is the only formatter in this spike allowed to touch a
JID, and its unit tests assert the full identifier never survives it.

### 1.9 — PARTIAL

`cargo build --release` produces a working binary that runs the CLI. The
packaged per-OS pair/send/logout smoke is still outstanding.

## Cleanup

Delete this directory once Step 1 is recorded in the plan. The session lives at
`~/.cache/alfred-whatsapp-spike/session.db` (override with `SPIKE_DB`) —
deliberately outside the repo, because an in-tree copy was destroyed by a branch
switch, costing a pairing and orphaning a linked device. It holds real
linked-device credentials: always run `logout` before deleting anything, and
remove the entry from **WhatsApp → Linked Devices** if the remote logout fails.
