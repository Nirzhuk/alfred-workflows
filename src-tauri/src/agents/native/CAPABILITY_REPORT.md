# Native harness capability report

Contract versions: request `1`, event `1`, capability `3`, Alfred tool boundary `1`.
Requests also carry the Alfred package version and the registered runtime version.

Capability contract `3` retains explicit `supports_patch` and adds an exact
provider/product binding plus `alfred_executed`,
`runtime_executed_with_host_approval`, or `no_tools` execution ownership.
Contradictory capability/owner declarations fail registration. A resolved
account for another product is rejected before credential access or execution.

The provider-neutral fake runtimes are the only runtimes registered by the Plan
032 conformance fixtures. Their evidence is the focused
`agents::native::conformance` suite; neither is registered in production.

`NativeExecutionRouter` is installed in `lib.rs` and consumed by the workflow
runner, and `AgentAccountResolver` (Plan 031) is its only production account
resolver. A provider plan registers one `NativeAgentRuntime` in the managed
`NativeRuntimeRegistry`; it does not edit the runner, the account schema, or
this contract.

| Capability | Fake runtime | Evidence |
| --- | --- | --- |
| Account validation | supported | Resolves an opaque account reference and validates the in-memory credential boundary. |
| Model discovery | supported | Returns a bounded catalog and rejects an unknown selected model. A runtime declaring no catalog instead runs with a bounded, screened explicit model. |
| Streamed turn | supported | Emits versioned start, assistant delta, and completion events. |
| Tool calls | supported | Uses the Alfred-owned executor boundary; no shell fallback exists. Shell and process tools require a workspace-confined cwd. |
| Tool execution owner | `alfred_executed` | The fake runtime delegates typed calls to Alfred; descriptors using another owner cannot invoke Alfred's executor. |
| Patch application | supported | Declared through `supports_patch`; refused when neither patch nor native filesystem is declared. |
| Approval events | supported | Allow and deny decisions are emitted and enforced; every denial emits a terminal `tool_completed`. |
| Cancellation | supported | The registry cancels an active cooperative turn by provider-scoped handle, and a run's Stop flag cancels the live turn through `AgentRunHooks`. |
| Timeout | supported | A bounded deadline fails with the typed `timed_out` classification. |
| Usage | supported | Returns a typed supported snapshot; runtimes without the capability return `unavailable`. |
| Sessions/resume | unsupported | The fake descriptor declares neither capability; requests fail visibly. |
| OAuth | unsupported | Authentication transport is outside Plan 032. |
| MCP/subagents | unsupported | No optimistic defaults or hidden shell emulation. |
| CLI binary dependency | unsupported | The runtime trait and fixture do not locate, invoke, or inspect a CLI binary. |

Production providers (`claude_code`, `cursor`, `codex`, `opencode`,
`github_copilot`, `gemini`, `grok`, `pi`, and `omp`) are **blocked** until their
provider plans register a native runtime and supply provider-owned conformance
evidence. Absence from the registry is reported as `provider_unavailable`; it
does not fall back to the CLI harness.
