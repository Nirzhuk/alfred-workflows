# Plan 036: Add an Alfred-managed OpenCode runtime

> **Executor instructions**: Preserve the existing OpenCode CLI adapter. Treat
> OpenCode as a runtime/router that may reach multiple upstream providers, not
> as a universal subscription provider. Do not infer account ownership or quota
> from local OpenCode files without an explicit runtime contract.
>
> **Official reference to re-check before implementation**:
> [OpenCode documentation](https://opencode.ai/docs/)

## Status

- **Priority**: P1
- **Effort**: XL
- **Risk**: HIGH
- **Depends on**: Plans 030–032
- **Category**: native runtime / provider router
- **Planned at**: 2026-08-24
- **Implementation**: BLOCKED — safe provider policy/protocol fixtures exist;
  production registration is withheld on the package, account-entry, and
  Alfred-owned tool bridge gates below.

## Frozen runtime decision and evidence (2026-08-25)

- **Supported surface**: OpenCode's documented local HTTP server and generated
  JS/TS SDK are suitable integration surfaces. The server documents OpenAPI,
  basic authentication, `/auth/:id`, provider/model discovery, session create,
  prompt, abort, permission reply, and SSE events. The SDK documents
  `createOpencode`, `createOpencodeClient`, explicit `{ providerID, modelID }`
  routing, session APIs, and event subscription.
  ([server](https://opencode.ai/docs/server/),
  [SDK](https://opencode.ai/docs/sdk/))
- **Frozen version**: upstream release and SDK `1.18.23`, published 2026-08-25.
  The upstream release publishes command-runtime assets for macOS arm64/x64,
  Linux arm64/x64 (including musl variants), and Windows arm64/x64.
  ([release](https://github.com/anomalyco/opencode/releases/tag/v1.18.23),
  [SDK package](https://github.com/anomalyco/opencode/blob/v1.18.23/packages/sdk/js/package.json))
- **License/redistribution**: the OpenCode repository and SDK declare MIT. MIT
  permits redistribution when the copyright and permission notice accompany
  copies. This answers the source-license question only; Alfred still needs an
  owned artifact manifest, checksum verification, code-signing/notarization,
  platform smoke evidence, notice inclusion, and updater policy before a
  binary may enter a package.
  ([license](https://github.com/anomalyco/opencode/blob/v1.18.23/LICENSE))
- **Lifecycle**: the published SDK's server helper launches the `opencode`
  executable from `PATH`, inherits the parent environment, and injects inline
  config. That helper is not an acceptable Alfred runtime by itself. The safe
  launch contract in `providers/opencode/launch.rs` instead requires an
  absolute bundled executable, loopback-only server with basic auth, an empty
  inherited environment, dedicated XDG config/data/cache/state/temp paths,
  disabled project config, runtime tool denial, and no `PATH` or user `HOME`
  fallback. It remains data-only until the package gate passes.
  ([SDK launcher source](https://github.com/anomalyco/opencode/blob/v1.18.23/packages/sdk/js/src/server.ts),
  [config precedence](https://opencode.ai/docs/config/))
- **Auth and billing ownership**: OpenCode is a router, not the billing owner by
  default. `OpenCodeAccountBinding` records the real OpenCode upstream provider
  id, provider-approved auth kind, and human-readable billing owner. Every
  model is stored as `<upstream-provider>/<model-id>` and a mismatch is a hard
  `account_mismatch`; no catalog default can switch the route. OpenCode Zen is
  represented explicitly as the `opencode` upstream billed by OpenCode Zen;
  OpenRouter, Anthropic, OpenAI, local providers, and others retain their own
  identities and billing. OpenCode's provider docs distinguish these auth
  paths and warn that one provider connection does not grant others.
  ([providers](https://opencode.ai/docs/providers/))
- **Credential ownership**: the documented SDK `auth.set` call writes a
  provider-specific API/OAuth credential through `/auth/:id`; source stores it
  under OpenCode's data path. Alfred's launch contract redirects that entire
  path to its dedicated runtime home and never reads the user's OpenCode
  auth/config/database/history. It does not rely on the source-only
  `OPENCODE_AUTH_CONTENT` shortcut as a stable public auth contract. Plan 031
  also has no approved non-React secret-entry seam for provider API keys. Test
  fixtures consume only a test-resolved `NativeCredential`; live account setup
  is **BLOCKED** as `opencode_native_secret_entry_unavailable`.
  ([documented SDK auth](https://opencode.ai/docs/sdk/#auth),
  [auth storage source](https://github.com/anomalyco/opencode/blob/v1.18.23/packages/opencode/src/auth/index.ts))
- **Models and usage**: the documented config provider endpoint returns the
  upstream provider catalog and defaults; prompts require explicit provider
  and model ids. OpenCode message parts report per-turn token counts and cost,
  but these are runtime observations/estimates, not authoritative subscription
  quota. Plan 036 therefore does not claim quota or subscription ownership.
- **Events, sessions, and cancellation**: documented SSE events, session
  identity, session idle/error, exact-session resume, and session abort are
  supportable. The provider decoder accepts only bounded named variants,
  rejects reasoning, cross-session, malformed, and oversized events, and
  ignores unknown methods instead of exposing server passthrough. Live
  cancellation still requires the packaged process/transport gate to pass.
- **Tools/approval**: 1.18.23 documents allow/reject permission responses, but
  its generated `Permission` contract types tool metadata as unknown and the
  official server offers no endpoint for injecting an Alfred-owned tool result.
  Approving an OpenCode permission would therefore execute inside OpenCode,
  bypassing Plan 032's tool executor. Permission approval/denial is decoded in
  fixtures but live tools remain **BLOCKED** as
  `opencode_native_tool_bridge_unavailable`; no CLI-output recreation or
  undocumented plugin bridge is used.
  ([generated event types](https://github.com/anomalyco/opencode/blob/v1.18.23/packages/sdk/js/src/gen/types.gen.ts))

### Exact native release blockers

1. `opencode_native_package_unverified`: no Alfred-owned pinned artifact
   manifest/checksums, signing/notarization evidence, notice bundle, supported
   platform smoke matrix, or updater ownership exists for 1.18.23.
2. `opencode_native_secret_entry_unavailable`: the frozen native account
   contract has no approved non-React upstream secret-entry seam; secrets may
   not cross React/Tauri DTOs.
3. `opencode_native_tool_bridge_unavailable`: the official permission/tool
   contract cannot route typed tool arguments and results through Alfred's
   executor boundary.

OpenCode remains absent from production native-runtime registration. This is a
package/capability gate, not a request for a user-installed CLI. Existing
`provider=opencode,harness=cli` behavior was not edited.

### Provider fixture evidence

`src-tauri/src/agents/native/providers/opencode/` covers version/license/platform
freeze, dedicated runtime-home launch shape, upstream auth/billing/model route,
startup and protocol failure, account unavailable, malformed/oversized and
reasoning events, permission approval/denial observation, cancellation/timeout,
rate limit, exact-session resume binding, route mismatch, redaction, and ignored
arbitrary server methods. A focused test command was attempted; compilation was
initially blocked by concurrently incomplete sibling provider modules, then
passed after those files settled: `11 passed; 0 failed; 575 filtered out` for
`cargo test --locked --manifest-path src-tauri/Cargo.toml
agents::native::providers::opencode --no-fail-fast`. No broad build or
prohibited suite was run. The native settings disclosure separately shows
version, upstream/billing ownership, the three capability gaps, and continued
CLI-harness separation without accepting or rendering a credential.

## Goal

Offer an Alfred harness mode that uses OpenCode capabilities without requiring
users to install or separately launch the OpenCode CLI, while keeping the
existing OpenCode CLI harness intact.

## Provider/runtime reality

OpenCode can route requests to multiple upstream providers and may expose a
server/API/SDK surface distinct from its terminal CLI. Native mode must identify
which layer owns:

- authentication;
- provider billing;
- model catalog;
- tool execution;
- session persistence;
- usage data.

“OpenCode OAuth” is not automatically equivalent to Claude, OpenAI, Gemini, or
other provider subscription OAuth.

## Scope

**In scope**:

- Discovery of the supported OpenCode server/SDK/runtime surface.
- Bundled or in-process OpenCode runtime decision.
- Native account/provider configuration with secure storage.
- Model/provider selection and capability mapping.
- Event, tool, approval, cancellation, and session integration.
- Native OpenCode UI distinct from the CLI.

**Out of scope**:

- Reading the user's OpenCode SQLite database as an auth source.
- Scraping OpenCode credentials or local history.
- Claiming one OpenCode account grants every upstream provider subscription.
- Recreating OpenCode's runtime from CLI output.
- Supporting every upstream provider in the first native release.

## Implementation steps

### Step 1: Verify the runtime contract

Determine whether the current OpenCode release provides a documented:

- local server API;
- SDK;
- embeddable runtime;
- auth/account surface;
- model/provider catalog;
- event and tool protocol.

Record versions, licensing, supported platforms, and whether a bundled runtime
may be redistributed.

**STOP** if only undocumented CLI internals are available.

### Step 2: Choose bundled versus direct runtime

Prefer a documented server/SDK with a dedicated Alfred runtime home. If a
server process is bundled, it must be version-pinned, signed, bounded, and
terminated with the Alfred run. If an SDK is used, define the bridge lifecycle
and crash behavior.

The native runtime must not accidentally read a user's global OpenCode config or
credentials. Explicit import may be considered later as a separate plan, but
not in the first implementation.

### Step 3: Define upstream account model

Represent OpenCode's upstream provider identity explicitly. A native account
row must identify whether it is:

- OpenCode-hosted account;
- upstream provider API key;
- upstream provider OAuth;
- local/provider configuration.

Store only provider-approved secrets in Plan 031's account store. Show the
actual billing owner in the UI.

### Step 4: Implement native execution

Map Alfred requests to the selected OpenCode runtime:

- provider/model route;
- prompt/context and skill loading;
- working directory and permission profile;
- tool/approval events;
- cancellation and timeout;
- normalized assistant/tool output;
- session/resume only when supported.

Do not expose arbitrary OpenCode server methods through workflow JSON.

### Step 5: Implement model and usage boundaries

Model discovery must report the upstream provider and billing method. Usage is
provider-specific. If OpenCode can only report local estimates, label them as
estimates and do not present them as authoritative subscription quotas.

### Step 6: Add UI and fixtures

Native OpenCode settings must show runtime version, upstream provider/account,
auth method, billing owner, and capability gaps. Add fake server fixtures for
login, provider unavailable, malformed event, tool approval, cancellation,
rate limit, and session resume.

## Subagent-ready ownership slices

- **Runtime research**: official server/SDK surface, version, license, packaging.
- **Account model**: upstream identity and secure credential mapping.
- **Transport**: server/SDK lifecycle, events, tools, cancellation.
- **Usage/model**: provider attribution and honest quota states.
- **UI/tests**: native configuration and contract fixtures.

## STOP conditions

- Native mode requires reading global OpenCode credentials or private DB files.
- OpenCode has no stable documented server/SDK surface.
- Upstream billing/auth ownership cannot be shown to users.
- A bundled runtime cannot be signed, updated, or legally redistributed.
- Native mode would silently route data to a different upstream provider.

## Verification

```bash
bun test
bun run build:frontend
cargo test --locked --manifest-path src-tauri/Cargo.toml agents runner
cargo check --locked --manifest-path src-tauri/Cargo.toml
bun run check
```

Manual smoke must install Alfred without OpenCode CLI, connect one supported
upstream provider, run a bounded workspace task, inspect account/model/usage
labels, then disconnect and verify cleanup.

## Done criteria

- [x] A supported OpenCode native runtime surface is selected and versioned.
- [x] Upstream provider and billing ownership are explicit.
- [x] No global OpenCode credential/database scraping exists.
- [ ] Native events/tools/cancellation pass Plan 032 (tool-result bridge and
      packaged live cancellation remain blocked).
- [x] Existing OpenCode CLI mode remains unchanged.
- [x] Unsupported upstream/auth combinations are blocked honestly.
