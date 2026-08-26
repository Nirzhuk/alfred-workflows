# Plan 036: Add an Alfred-managed OpenCode server

> **Executor instructions**: Preserve the existing OpenCode CLI adapter. The
> first managed route is the documented local server, isolated from every user
> OpenCode home. Never infer a universal provider entitlement or add fallback.

## Status

- **Priority**: P1
- **Effort**: XL
- **Risk**: HIGH
- **Depends on**: Plans 030–032
- **Category**: native runtime / managed server
- **Architecture revised**: 2026-08-26
- **Implementation**: BLOCKED — Phase 1 defines the products and runtime
  profile contract; no managed OpenCode server is packaged or registered.

## Decision history

- **2026-08-25:** Alfred froze the documented local
  [server](https://opencode.ai/docs/server/),
  [SDK](https://opencode.ai/docs/sdk/), and official
  [`1.18.23` release](https://github.com/anomalyco/opencode/releases/tag/v1.18.23)
  as the technical candidate. The repository's
  [MIT license](https://github.com/anomalyco/opencode/blob/v1.18.23/LICENSE)
  covers source redistribution but does not settle the hosted product terms.
- **2026-08-26:** Alfred split OpenCode Go subscription access from Zen PAYG
  and kept broad commercial launch **NO-GO** pending written clarification of
  the official [OpenCode Terms of Service](https://opencode.ai/legal/terms-of-service).
  The [OpenCode Go](https://opencode.ai/docs/go/) key may enter only the
  isolated managed server. Exhaustion must never fall through to Zen.

The exact stable gates are `opencode_native_package_unverified`,
`opencode_native_secret_entry_unavailable`, and
`opencode_native_tool_bridge_unavailable`. Shipping diagnostics use the
aggregate `opencode_package_account_and_tool_bridge_unverified`. No OpenCode
native runtime is registered.

## Product routes

| Product ID | Auth and custody | Billing | Runtime |
| --- | --- | --- | --- |
| `opencode_go` | User provides an OpenCode Go key transiently to the isolated server | OpenCode Go subscription account | `opencode_server` `1.18.23` |
| `opencode_zen` | Separate PAYG credential routed only to Zen | OpenCode Zen credential owner | `opencode_server` `1.18.23` |

Go and Zen are separate products. There is no routing fallback from exhausted
or ineligible Go entitlement to Zen. Both use an opaque runtime profile;
`opencode_go` must not receive a fake Alfred secret reference. Zen may also
hold an Alfred secret reference because it is a distinct PAYG credential.

## Runtime decision

Package and launch `opencode serve` `1.18.23` first. Bind only loopback with
runtime authentication, use an absolute packaged executable, and isolate all
XDG config/data/cache/state/temp paths. Never use PATH, HOME, a user OpenCode
database, or global configuration.

For Go, the key crosses only a transient secret-entry boundary and is sent to
the isolated runtime using documented `PUT /auth/opencode-go`; the runtime
persists it inside its managed profile. Alfred retains only the opaque profile
reference. Model routes are explicit and may not silently switch product or
billing owner.

## Tool ownership

OpenCode built-in tools execute in the managed runtime. The descriptor must be
`runtime_executed_with_host_approval`, and every permission request must be
observable, denyable, cancellable, and bounded by Alfred before runtime
execution. If the documented server cannot prove that ordering, native tools
remain disabled as `no_tools`; do not invent an undocumented tool-result bridge.

## Implementation sequence

1. Freeze 1.18.23 platform artifacts, checksums, signatures, MIT notice,
   updater, rollback, and signed-package ownership.
2. Build a loopback-only managed server lifecycle with isolated XDG roots,
   bounded HTTP/SSE, authentication, deadlines, and crash cleanup.
3. Implement the transient Go-key handoff to `PUT /auth/opencode-go`, returning
   only safe identity and a runtime profile reference.
4. Implement Zen as a separate credential, billing, model, and entitlement
   route. Reject cross-product defaults and fallback.
5. Map sessions, events, cancellation, model catalog, and honest entitlement
   observations into Plans 031–032.
6. Prove runtime-executed tools receive host approval before execution, or ship
   no tools.
7. Pass packaged no-user-CLI smoke before registration or UI availability.

## External policy gate

Broad commercial launch of an Alfred-packaged OpenCode Go runtime requires
written Terms-of-Service clarification for this integration shape. MIT source
redistribution permission does not settle hosted-service/subscription terms.
Keep production registration disabled until that decision and the technical
package gates are recorded.

## Release gates

- Written commercial/ToS clarification for managed OpenCode Go use.
- Pinned packages, notices, signatures, updates, rollback, and platform smoke.
- Isolated runtime profile creation, deletion, reinstall, and account switching.
- No secret or profile reference crosses command DTOs or React state.
- Exact product/model route and no Go-to-Zen fallback.
- Host approval is proven before built-in tool execution, or tools are absent.
- Native failure never calls the OpenCode CLI adapter.

## STOP conditions

- Runtime reads global OpenCode state, PATH, HOME, or a user database.
- Go is represented as an Alfred-held API secret after runtime persistence.
- Go exhaustion silently incurs Zen or another provider's billing.
- Tool execution can precede host approval.
- Commercial terms or packaged artifact ownership remain unresolved.

## Done criteria

- [x] Go and Zen product, billing, credential, and entitlement boundaries are
  explicit.
- [x] Managed server/profile and tool-owner contracts are explicit.
- [x] CLI compatibility and no-fallback behavior remain mandatory.
- [ ] ToS clarification and package gates pass.
- [ ] Managed server, auth, event, tool, and cleanup conformance pass before
  registration is enabled.
