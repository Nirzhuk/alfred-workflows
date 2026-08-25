# Agent harnesses

Each agent step stores two separate choices: a provider and a harness. Provider
ids stay stable; changing the harness never turns Codex into a different
provider, for example.

## Provider CLI

The `cli` harness delegates the step to the provider's installed command-line
agent. It is a first-class option, not a deprecated compatibility mode. The CLI
keeps ownership of its login, configuration, plugins, model aliases, and local
sessions. Alfred neither reads nor imports the CLI's credentials.

Workflows saved before the harness selector existed have no `harness` field.
Alfred reads those workflows as `cli`, so they continue through the same local
adapter. Newly created agent steps save `harness: "cli"` explicitly.

## Alfred

The `alfred` harness is the boundary for future Alfred-managed provider
execution. It has a separate native runtime and a separate opaque account
reference. Native account credentials are never written to workflow JSON, run
events, or model/provider capability responses, and CLI credentials are not
reused as native credentials.

No provider-native runtime or account connection is enabled in this release.
The editor reads the same versioned capability manifest as the runner and shows
each provider's `blocked` or `disabled` reason. Attempting to run an unavailable
native node returns `native_runtime_unavailable`; Alfred does not fall back to a
provider CLI. As native providers are added later, their model and account
support may remain narrower than the corresponding CLI.

## Compatibility and migration

Old workflows, imported graphs, duplicated nodes, and templates that omit the
`harness` field are read as `cli`. Alfred never rewrites them to `alfred`.
New agent nodes persist `harness: "cli"`, and changing the selector is an
explicit edit that also clears any account reference that belonged to the
previous harness.

Run history records both the provider and harness for completed, failed, and
cancelled agent steps. This is execution identity, not a migration hint: a
reader must never infer that a failed native step ran through the CLI.

## Current native release gates

No native provider is enabled. These provider-specific blockers do not affect
CLI execution or one another:

| Provider | Status | Exact external/release blocker |
| --- | --- | --- |
| Codex | Blocked | Cross-platform runtime signing and packaged no-CLI smoke evidence are missing for the pinned app-server 0.149.1 artifacts |
| Claude | Blocked | Approved non-React API-key intake and live API smoke are missing; Claude.ai subscription login also requires Anthropic approval not on record |
| Cursor | Blocked | API-key custody, explicit repository consent persistence, and live end-to-end Cloud Agents validation are missing |
| OpenCode | Blocked | Runtime artifact/checksum/signing/update ownership, upstream-secret intake, and a typed Alfred tool-result bridge are unresolved |
| GitHub Copilot | Blocked | The pinned SDK/CLI is not linked and packaged with its required license notices and packaged smoke evidence |
| Gemini | Blocked | Approved API-key intake and live API smoke are missing; desktop OAuth client packaging is a separate unresolved gate |
| Grok | Blocked | Approved xAI API-key intake and live API smoke are missing |
| Pi / OMP | Disabled | No Alfred-native provider implementation or release evidence exists |

Runtime versions in the manifest are protocol/package targets, not claims that
an artifact is bundled. Bundled metadata separately reports resource presence,
checksum, licence/notice, signing, and rollback gates. A missing resource or
manifest entry fails closed.

## Recovery and explicit fallback

CLI and Alfred credentials are separate. If a provider later enables native
reconnection, it will not change the CLI's login; signing in to a CLI never
connects a native account. If native mode is unavailable:

1. Keep the failed node unchanged while collecting its safe provider/harness
   diagnostic.
2. This zero-native release cannot establish, refresh, or reconnect a native
   account. Existing recovery records can only be disconnected or cleaned up
   from **Settings → Native agent accounts**. When cleanup cannot finish, revoke
   or rotate the credential at the provider before choosing **Remove local
   data**.
3. To keep working through the CLI, explicitly edit the node and select
   **Provider CLI**. Confirm the CLI is installed and logged in first.

Alfred never performs step 3 automatically because it can change the account,
billing owner, data route, and tool permissions. See the
[native harness support runbook](native-harness-support.md) for safe diagnostic
and runtime-recovery guidance.
