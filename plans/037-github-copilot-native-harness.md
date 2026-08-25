# Plan 037: Add a native GitHub Copilot harness

> **Executor instructions**: Preserve the existing Copilot CLI adapter. Native
> mode must use an official Copilot SDK/runtime or documented API. Do not call
> undocumented Copilot endpoints or pretend a GitHub OAuth token alone is a
> complete agent runtime.
>
> **Official references**:
>
> - [Copilot CLI authentication](https://docs.github.com/en/copilot/how-tos/copilot-cli/set-up-copilot-cli/authenticate-copilot-cli)
> - [Copilot SDK](https://github.com/github/copilot-sdk)
> - [Copilot SDK authentication](https://docs.github.com/en/copilot/how-tos/copilot-sdk/authenticate-copilot-sdk/authenticate-copilot-sdk)

## Status

- **Priority**: P1
- **Effort**: XL
- **Risk**: HIGH
- **Depends on**: Plans 030–032
- **Category**: native provider / OAuth
- **Planned at**: 2026-08-24
- **Implementation**: TODO

## Goal

Allow users with GitHub Copilot access to authenticate through Alfred and run a
Copilot agent without manually installing or launching the Copilot CLI, while
keeping the current CLI harness available.

## Provider reality

GitHub documents OAuth device login for Copilot CLI and a Copilot SDK that
communicates with a Copilot CLI server through JSON-RPC. This may satisfy the
user experience requirement if the SDK/runtime can be bundled or managed by
Alfred, but it is not automatically a direct HTTP model API.

The native plan must decide whether the SDK's managed runtime is redistributable
and whether Alfred can securely own its lifecycle.

## Scope

**In scope**:

- Official Copilot OAuth/device authentication path.
- Copilot SDK/runtime transport evaluation.
- Bundled or SDK-managed runtime lifecycle.
- Local workspace/tool permission mapping.
- Streaming activity, approvals, cancellation, and final results.
- Model/catalog and usage/account state where officially available.
- Native account UI and billing/auth copy.

**Out of scope**:

- Calling Copilot APIs with a raw GitHub token without an SDK/API contract.
- Scraping Copilot CLI keychain/configuration.
- Assuming GitHub API OAuth equals Copilot entitlement.
- Remote Copilot control surfaces or cloud workflow execution.
- Replacing the current Copilot CLI adapter.

## Implementation steps

### Step 1: Verify SDK/runtime distribution

Read the current SDK repository and documentation. Record:

- supported languages and Rust integration path;
- whether the SDK downloads, launches, or requires Copilot CLI;
- runtime version pinning and update behavior;
- OAuth/token custody;
- workspace and tool policy;
- redistribution and packaging terms;
- local versus cloud execution.

**STOP** if a user-installed CLI remains mandatory for native mode and Alfred
cannot bundle or manage the required runtime.

### Step 2: Implement OAuth/account lifecycle

Use the documented OAuth device flow or SDK-managed login. Store Alfred's
account metadata and credential reference through Plan 031. Do not import an
existing `copilot-cli` keychain entry.

Show GitHub account identity, Copilot entitlement state, organization/SSO
requirements, and token expiry only when officially available.

### Step 3: Implement SDK/runtime adapter

Map the Plan 032 native request to the Copilot SDK/server:

- workspace root;
- prompt and skill/context block;
- model;
- permission/approval policy;
- tool calls and progress;
- cancellation;
- final response and changed files.

Normalize SDK JSON-RPC events. Bound all tool output and redact provider
credentials before Alfred persistence.

### Step 4: Implement account/model/usage states

Use supported SDK/account APIs. Distinguish:

- GitHub authentication;
- Copilot entitlement;
- API-key/BYOK mode;
- organization policy/SSO;
- unavailable quota.

Do not present GitHub login success as proof that a Copilot seat is active.

### Step 5: Add native UI and compatibility fixtures

Provide separate harness entries:

- GitHub Copilot CLI;
- GitHub Copilot · Alfred.

Fixtures must cover device-code success/denial, account mismatch, SSO/policy
denial, expired token, runtime startup failure, JSON-RPC event mapping,
approval, cancellation, rate limit, and logout.

## Subagent-ready ownership slices

- **SDK/runtime research**: official SDK, lifecycle, packaging, license.
- **OAuth/account**: device flow, entitlement, secure storage.
- **Transport**: SDK JSON-RPC, events, tools, cancellation.
- **Workspace safety**: local roots, approvals, data boundaries.
- **UI/release QA**: native settings, packaged runtime, fixtures.

## STOP conditions

- SDK requires an unbundled CLI for the promised user experience.
- Raw GitHub OAuth cannot prove Copilot entitlement or invoke supported agent
  execution.
- Runtime messages cannot be bounded/redacted.
- Organization/SSO policy cannot be surfaced safely.
- SDK/runtime licensing or update distribution is unresolved.

## Verification

```bash
bun test
bun run build:frontend
cargo test --locked --manifest-path src-tauri/Cargo.toml agents runner
cargo check --locked --manifest-path src-tauri/Cargo.toml
bun run check
```

Manual smoke must start Alfred with no Copilot CLI installed, complete the
supported login path, run a bounded workspace task, exercise an approval and
cancellation, then disconnect and verify cleanup. The existing CLI path gets a
separate regression smoke.

## Done criteria

- [ ] Official Copilot SDK/API surface is selected and versioned.
- [ ] Native mode does not require manual CLI installation.
- [ ] OAuth, entitlement, SSO, and billing states are distinct.
- [ ] Runtime events/tools/cancellation pass Plan 032.
- [ ] CLI mode remains unchanged.
- [ ] Packaged runtime and license/update gates pass.
