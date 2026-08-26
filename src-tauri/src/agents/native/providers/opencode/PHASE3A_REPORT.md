# OpenCode Go managed provider — Phase 3A

## Implemented provider slice

- Exact `opencode_go` / `opencode_server` `1.18.23` descriptor with
  `runtime_executed_with_host_approval`; production `register()` remains
  fail-closed.
- Six-target code-owned package manifest with frozen release-archive and
  extracted-executable SHA-256 values, byte sizes, bundled MIT license and
  notice digests, immutable update policy, and required publisher-verification
  evidence supplied only through `OpenCodePackageVerifier`.
- Managed launch contract for the selected verified executable: `opencode
  serve`, random IPv4 loopback port, authenticated readiness, explicit
  repository cwd, isolated profile roots, disabled mDNS/CORS/share/self-update
  and project config, with no user `HOME`, raw `PATH`, or external CLI lookup.
- Backend-only Basic-auth HTTP/SSE V1 client with bounded responses; transient
  `PUT`/`DELETE /auth/opencode-go` key custody; strict `opencode-go` catalog and
  model routing; session create/exact resume, subscribe-before-prompt,
  text/tool/status/error mapping, once/always/reject permission replies,
  abort/cancel, deep-link-only usage, disconnect, and profile purge.
- Deterministic fake HTTP/SSE server and conformance fixtures for auth secrecy,
  wrong password, strict model routing, streaming, approvals, cancellation,
  exact resume, rate limits, disconnect, process crash, and no CLI fallback.

## Shared hooks still required

1. The sealed runtime-package substrate needs a production constructor that
   returns a `RuntimePackageSelection` only after it has independently verified
   the OpenCode package publisher. Release digests alone are intentionally not
   treated as publisher proof.
2. `ManagedRuntimeSupervisor` needs a backend capability handoff that joins its
   internally generated OpenCode Basic password to the provider HTTP client
   without logging, serializing, persisting, or returning it to a command DTO.
3. The account service/command boundary needs transient OpenCode Go secret
   entry and must persist only the opaque runtime profile reference.
4. The native turn host needs a decision callback for
   `runtime_executed_with_host_approval` permission requests; `invoke_tool` is
   correctly unavailable for this owner mode.
5. Written commercial approval and a packaged, offline/no-external-CLI live
   smoke are still release gates.

## Validation handoff

No formatter, Rust tests, build, or commit was run, as required by the dispatch.
`git diff --check -- src-tauri/src/agents/native/providers/opencode` completed
without output, and the bundled legal assets matched their frozen SHA-256
constants. Recommended integration validation commands are:

```bash
cargo test --manifest-path src-tauri/Cargo.toml agents::native::providers::opencode::tests -- --nocapture
cargo check --manifest-path src-tauri/Cargo.toml --lib
```

After the shared gates are implemented, run the packaged `1.18.23` live smoke
with the network blocked except for OpenCode Go, an empty user `PATH` and home,
and assertions that startup, auth, model discovery, permissions, cancellation,
disconnect, and crash recovery never invoke a user-installed `opencode` CLI or
route to Zen/upstream.
