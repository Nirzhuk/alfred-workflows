# Codex SDK managed sidecar

This source directory is the only production candidate for Alfred's
`chatgpt_codex` managed runtime. It imports symbols exported by
`openai_codex`, pins `openai-codex==0.147.0` and
`openai-codex-cli-bin==0.147.0`, and always passes
`CodexConfig.experimental_api=False`.

The packaged executable must contain its own Python 3.10 or newer runtime and
place the matching Codex binary at `libexec/codex` beside the sidecar's `bin`
directory. The package launcher never searches `PATH`. Alfred supplies an
account-scoped `CODEX_HOME` through `ManagedRuntimeSupervisor`, and the
sidecar forces ChatGPT login plus keyring-backed credential storage. It rejects
ambient API credentials and a profile `auth.json`.

The public SDK supports browser and device-code login, account reads, logout,
models, thread creation and resume, streamed turns, login cancellation, and
turn interruption. It does not export a host approval callback. The public
`ApprovalMode` only offers `deny_all` and `auto_review`; the underlying default
handler is not part of the exported API and cannot be used. Alfred therefore
keeps production registration blocked by
`codex_python_sdk_host_approval_unavailable` and sends turns only with
`ApprovalMode.deny_all` plus `Sandbox.read_only` in this non-shipping slice.

Release packaging must consume `runtime-package.source.json` and
`sbom.cdx.json`, vendor every selected wheel from a recorded SHA-256, copy the
upstream `LICENSE` and `NOTICE`, produce one signed/notarized sealed package per
target, and hand the result to the shared runtime-package verifier. Building a
sidecar executable from this source does not create trusted verification
evidence.
