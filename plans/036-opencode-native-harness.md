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
- **Implementation**: TODO

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

- [ ] A supported OpenCode native runtime is selected and versioned.
- [ ] Upstream provider and billing ownership are explicit.
- [ ] No global OpenCode credential/database scraping exists.
- [ ] Native events/tools/cancellation pass Plan 032.
- [ ] Existing OpenCode CLI mode remains unchanged.
- [ ] Unsupported upstream/auth combinations are blocked honestly.
