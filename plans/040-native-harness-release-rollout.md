# Plan 040: Release and roll out the dual-harness system

> **Executor instructions**: This plan is the final integration gate after the
> foundation, account system, compatibility contract, and selected provider
> plans. Do not enable native providers globally just because their code builds.
> Each provider must pass its own auth/runtime/package gates.

## Status

- **Priority**: P0
- **Effort**: XL
- **Risk**: HIGH
- **Depends on**: Plans 030–039 and any provider plan marked shipped
- **Category**: release / compatibility / operations
- **Planned at**: 2026-08-24
- **Implementation**: DONE — Provider CLI remains available and zero
  Alfred-native providers are enabled

## 2026-08-25 implementation evidence

- `src-tauri/src/agents/capability_manifest.rs` is the versioned manifest and
  fail-closed runner gate. It records provider, harness, runtime target,
  platform/build/auth, billing/model/usage sources, tools/approvals/resume/
  cancellation, package evidence, status, and an exact stable reason. Missing
  entries and failed gates are disabled.
- `get_agent_capability_manifest` exposes that same source to the editor;
  `src/features/workflow/agent-capabilities.ts` consumes the backend's final
  execution decision without reconstructing package trust. Blocked native
  choices remain visible for already-saved nodes, but cannot be selected for
  new execution. There is no global native flag.
- The runner calls the manifest gate before model/account/runtime resolution.
  Native failure does not construct a CLI adapter and no fallback path was
  added. Provider-specific blocking does not affect CLI or another provider.
- `src-tauri/src/agents/runtime_package.rs` implements bounded, path-safe
  resource lookup and checksum/licence/notice/signing/rollback inspection. The
  manifest honestly reports that no native sidecar/SDK artifact is included in
  this release; cloud/direct-HTTPS adapters report packaging not applicable.
- `get_agent_harness_diagnostics` exposes only bounded provider/harness/status,
  runtime target/state, shortened opaque account identity, auth method, account
  state, and stable error codes. Tests prove private identity, credential,
  scope, prompt/payload, and path-shaped fields do not enter the DTO.
- Old/imported/duplicated/template graph fixtures remain CLI, new nodes persist
  explicit CLI selection, explicit Alfred selections remain visible without
  rewriting, and run terminal outcomes retain provider+harness metadata.
- `docs/agent-harnesses.md` and `docs/native-harness-support.md` document
  separate credentials/billing, account recovery, safe escalation, runtime
  rollback, and explicit user-selected CLI fallback.

### Release result

| Alfred-native provider | Status | Exact remaining blocker |
| --- | --- | --- |
| Codex | blocked | `codex_cross_platform_signing_and_packaged_smoke_missing` |
| Claude | blocked | `claude_api_key_account_intake_and_live_smoke_missing`; subscription OAuth separately lacks Anthropic approval |
| Cursor | blocked | `cursor_account_repository_consent_and_e2e_gates_missing` |
| OpenCode | blocked | `opencode_package_account_and_tool_bridge_unverified` |
| GitHub Copilot | blocked | `copilot_sdk_package_license_and_packaged_smoke_missing` |
| Gemini | blocked | `gemini_api_key_account_intake_and_live_smoke_missing`; desktop OAuth packaging remains separate |
| Grok | blocked | `grok_api_key_account_intake_and_live_smoke_missing` |
| Pi / OMP | disabled | `native_provider_not_implemented` |

Enabled Alfred-native providers: **none**. Enabled Provider CLI harnesses:
Claude Code, Cursor, Codex, OpenCode, GitHub Copilot, Gemini, Grok, Pi, and OMP.

Focused verification (broad suites/builds intentionally left to the release
coordinator):

```text
cargo test --locked --manifest-path src-tauri/Cargo.toml --lib capability_manifest::tests
cargo test --locked --manifest-path src-tauri/Cargo.toml --lib runtime_package::tests
bun test tests/store.test.ts tests/native-harness-release-matrix.test.ts
git diff --check -- <Plan 040 files>
```

## Goal

Ship CLI and Alfred harnesses side by side with safe migration, honest provider
capability reporting, platform-specific packaging, and a rollback path.

## Product contract

- CLI harnesses remain supported and are never labeled deprecated.
- Alfred harnesses are opt-in until each provider passes its gates.
- Existing agent nodes default to CLI.
- Native account credentials are separate from CLI credentials.
- Provider availability is reported per provider, harness, platform, auth method,
  and runtime version.
- A native provider can be unavailable without affecting CLI workflows.
- Alfred never promises that one provider's subscription OAuth works for another
  provider or for API billing.

## Scope

**In scope**:

- Provider/harness capability matrix.
- Feature flags and staged enablement.
- Graph migration/backward compatibility.
- Native runtime packaging/signing/update behavior.
- Account recovery and diagnostics.
- Cross-provider conformance and release smoke tests.
- User-facing documentation and support playbooks.

**Out of scope**:

- Cloud execution.
- Automatic migration from CLI accounts.
- One universal OAuth account.
- Removing existing provider CLIs.
- Enabling blocked provider plans through hidden flags.

## Implementation steps

### Step 1: Create the capability manifest

Produce a versioned manifest keyed by provider+harness with:

```text
provider
harness
runtime_version
platforms
auth_methods
billing_method
model_source
usage_source
supports_tools
supports_approvals
supports_resume
supports_cancellation
status: disabled|beta|available|blocked
block_reason
```

The UI and runner must consume the same validated capability source. A missing
entry is disabled, not “probably available.”

### Step 2: Add feature gates and safe defaults

Native providers default off until their plan is accepted. Gate separately by:

- provider;
- platform;
- runtime version;
- auth method;
- packaged/development build where necessary.

The fallback behavior is an explicit user choice. A failed native run must not
silently retry through CLI because that can change account, billing, data
routing, and permission semantics.

### Step 3: Migrate workflow/editor compatibility

Verify old graphs, imported graphs, duplicated nodes, templates, and run
history. Add an explicit migration notice only when a native capability is no
longer available; never rewrite `cli` to `alfred` automatically.

Persist the selected harness in new graphs and include it in run history so a
later reader can tell which runtime executed a step.

### Step 4: Package native runtimes

For every bundled runtime:

- pin the upstream version;
- verify source/license notices;
- sign artifacts;
- validate resource lookup from packaged Tauri apps;
- bound child process lifetime if applicable;
- clean up after crash/cancel/uninstall;
- support version mismatch and rollback;
- keep provider runtime updates separate from Alfred workflow data.

### Step 5: Add support diagnostics

Expose safe diagnostics:

- harness/provider/runtime version;
- account auth method and redacted identity;
- capability status;
- last stable error code;
- runtime start/exit state;
- native/CLI selection.

Do not expose tokens, cookies, raw authorization URLs with secrets, full
provider payloads, or private prompt/output content.

### Step 6: Run staged rollout

Recommended rollout:

1. Foundation and fake-runtime tests.
2. Codex native development builds.
3. Codex native beta on one platform.
4. Codex native packaged cross-platform smoke.
5. Additional providers only after their own policy/runtime gates.
6. Broader native beta with explicit opt-in.
7. General availability per provider, not globally.

### Step 7: Publish support/recovery documentation

Document:

- CLI versus Alfred harness choice;
- provider-specific billing/auth differences;
- native account disconnect/reconnect;
- how to keep using CLI mode if native mode is unavailable;
- runtime update failures;
- token revocation and local cleanup;
- how to report provider-specific failures without attaching secrets.

## Subagent-ready ownership slices

- **Manifest/gates**: capability source, feature flags, UI state.
- **Migration**: graph compatibility and run-history metadata.
- **Packaging**: binaries/bridges, signing, resource lookup, rollback.
- **Diagnostics**: safe support state and redaction.
- **Release QA**: cross-platform smoke and provider matrix.
- **Documentation**: user/support/operator runbooks.

## STOP conditions

- Native failure silently changes to a CLI account or billing path.
- A provider is marked available without auth/runtime evidence.
- Bundled runtime signing, licensing, or update behavior is unresolved.
- Migration rewrites existing CLI workflows without user choice.
- Diagnostics can leak credentials, prompts, raw provider payloads, or private
  filesystem paths.

## Verification

```bash
bun test
bun run build:frontend
cargo test --locked --manifest-path src-tauri/Cargo.toml
cargo check --locked --manifest-path src-tauri/Cargo.toml
bun run check
```

For each enabled provider+harness+platform, run:

1. clean install;
2. native/CLI account setup;
3. model discovery;
4. one bounded turn;
5. tool approval and denial;
6. cancellation;
7. usage/account state;
8. logout/disconnect;
9. restart and recovery;
10. native failure without CLI fallback.

## Done criteria

- [x] Capability manifest drives runner and UI consistently.
- [x] Existing CLI workflows remain stable through upgrade.
- [x] Native enablement is staged per provider/platform; every native entry is
      currently blocked or disabled.
- [x] No packaged runtime is claimed or shipped; version/checksum/licence/
      signing/rollback gates fail closed until real artifacts exist.
- [x] Diagnostics and support docs are safe and actionable.
- [x] Fixture release matrix passes for the zero-provider native rollout and
      every available CLI provider.
