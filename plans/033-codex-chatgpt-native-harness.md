# Plan 033: Run Codex through Alfred with ChatGPT Codex

> **Executor instructions**: Preserve the existing Codex CLI adapter exactly.
> The Alfred route must use a managed, account-scoped runtime and must never
> import `~/.codex`, call a user-installed CLI, or fall back to the CLI.

## Status

- **Priority**: P0
- **Effort**: XL
- **Risk**: HIGH
- **Depends on**: Plans 030–032
- **Category**: native provider / managed subscription runtime
- **Planned at**: 2026-08-24
- **Architecture revised**: 2026-08-26
- **Implementation**: BLOCKED — Phase 1 defines the product/runtime contract;
  no Codex managed runtime is packaged, registered, or available.

## Decision history

- **2026-08-25:** Alfred froze the official App Server `0.149.1` release and
  its six artifact digests as non-production protocol evidence. The release,
  license, and version-coupled protocol remain recorded at the official
  [`rust-v0.149.1` release](https://github.com/openai/codex/releases/tag/rust-v0.149.1),
  [Apache-2.0 license](https://github.com/openai/codex/blob/rust-v0.149.1/LICENSE),
  and [App Server README](https://github.com/openai/codex/blob/rust-v0.149.1/codex-rs/app-server/README.md).
- **2026-08-26:** Shipping App Server is **NO-GO**. OpenAI's current
  [App Server documentation](https://learn.chatgpt.com/docs/app-server) labels
  the command experimental and unsupported for production. The stable
  [Codex SDK](https://learn.chatgpt.com/docs/codex-sdk),
  [authentication contract](https://learn.chatgpt.com/docs/auth), and
  [`openai-codex` package](https://pypi.org/project/openai-codex/) are the new
  candidate route, but packaging, approval/cancellation, and no-CLI smoke are
  still unproved.

The stable App Server gate codes remain
`codex_app_server_production_unsupported`,
`codex_runtime_signature_verification_missing`,
`codex_runtime_license_notice_packaging_missing`, and
`codex_packaged_no_cli_smoke_missing`. Shipping diagnostics use the aggregate
`codex_cross_platform_signing_and_packaged_smoke_missing`. No gate is passed by
the architecture change.

## Product routes

Codex has two independent Alfred products. They must never share credentials,
billing labels, entitlement claims, or automatic fallback:

| Product ID | Auth and custody | Billing | Runtime |
| --- | --- | --- | --- |
| `chatgpt_codex` | ChatGPT login owned by an isolated managed runtime profile | ChatGPT subscription account | `codex_python_sdk` `0.147.0` |
| `openai_api` | Alfred-managed API key | OpenAI API credential owner | No managed runtime in Phase 1 |

The `chatgpt_codex` account stores an opaque `runtime_profile_ref`, not a fake
secret credential reference. The profile lives under an account-scoped
`CODEX_HOME`. Alfred must not read or serialize the profile's token files.
Entitlement is initially `unknown`; no supported SDK call provides an
authoritative subscription rate-limit read.

## Runtime decision

The primary production candidate is the stable Python package
`openai-codex==0.147.0`, paired with the exact
`openai-codex-cli-bin==0.147.0` runtime artifact. Alfred would package a
hermetic Python sidecar with experimental mode disabled. The public high-level
SDK surface covers login/account/logout, models, threads, turns, and streaming.

The raw Rust App Server remains a separate, approval-gated route. Its protocol
work and fixtures are evidence, not a production runtime decision. Do not make
App Server the shipping default, expose arbitrary JSON-RPC, or describe it as
available until OpenAI approves the integration and its package gates pass.

## Tool ownership gate

The descriptor must declare exactly one tool execution owner. Codex may ship
only when the chosen stable SDK can represent cancellation and approval safely:

- prefer `alfred_executed` when typed SDK callbacks let Alfred execute tools;
- otherwise use `runtime_executed_with_host_approval` only when every execution
  request can be observed, approved or denied, cancelled, and bounded by the
  host;
- use `no_tools` for a deliberately tool-free release.

**STOP** if approvals or cancellation require private Python classes, raw
App-Server JSON-RPC, or undocumented endpoints. Do not weaken the contract to
make the SDK fit.

## Implementation sequence

1. Freeze the exact Python and binary packages, license/notice bundle,
   checksums, platform support, signing verification, rollback, and updater
   ownership.
2. Build a hermetic sidecar lifecycle with an absolute packaged path, empty
   inherited auth/config state, account-scoped `CODEX_HOME`, bounded IPC, and
   deterministic crash cleanup.
3. Implement runtime-owned ChatGPT login and return only safe account metadata
   plus an opaque profile reference through Plan 031.
4. Implement public high-level SDK model, thread, turn, stream, interruption,
   and logout surfaces. Reject private/raw fallbacks.
5. Map bounded output into Plan 032 events and prove the declared tool owner.
6. Run signed-package no-user-CLI smoke on every shipping platform before
   registration or UI availability changes.

## Release gates

- Stable public SDK covers the required login, turn, approval, and cancellation
  path without private/raw dependencies.
- Runtime profile isolation, deletion, reinstall, and account switching are
  verified without reading ambient CLI state.
- License, checksums, signatures, notices, updates, and rollback are owned for
  both Python and binary artifacts.
- Packaged macOS, Windows, and Linux no-CLI smoke passes.
- Entitlement remains honest: `unknown` is acceptable; inferred quota is not.
- Native failure returns a native error and never invokes `codex exec`.

## STOP conditions

- Native mode depends on a user-installed Codex CLI or ambient `~/.codex`.
- Subscription access is represented by an Alfred secret or API key.
- A private/raw App Server call is required for a production-critical action.
- Billing can silently switch between ChatGPT and OpenAI API.
- Alfred cannot prove the selected tool execution owner.

## Done criteria

- [x] Stable product, runtime, billing, entitlement, and profile contracts are
  explicit.
- [x] CLI compatibility and no-fallback behavior remain mandatory.
- [ ] Stable Python runtime package and lifecycle are implemented and verified.
- [ ] Public SDK approval/cancellation contract passes Plan 032 conformance.
- [ ] Packaged no-CLI release matrix passes before registration is enabled.
