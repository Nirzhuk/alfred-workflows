# Managed runtime release preparation

This directory is the release-only packaging boundary for the three managed
runtimes that have an exact provider contract:

| Runtime | Version | Package source |
| --- | --- | --- |
| `claude_code_managed` | `2.1.246` | Anthropic's signed release manifest |
| `opencode_server` | `1.18.23` | OpenCode's immutable GitHub release assets |
| `codex_python_sdk` | `0.147.0` | PyPI wheels plus python-build-standalone |

`manifests/*.json` are code-owned source inputs. They contain no `latest`,
range, fallback, or user-installed-CLI path. `prepare.ts` downloads only the
listed exact URL (following only an HTTPS final URL without latest/fallback
semantics), checks the pinned size/digest, extracts into a same-filesystem
temporary tree, and atomically renames a complete target tree into
`src-tauri/sidecars/managed-runtimes` (or an explicit `--output`). The
`package/` child is deliberately the exact input root expected by
`RuntimePackageStore::stage_and_activate`: declared files only, with no
manifest or signature sidecars inside it. Codex's CPython archive and locked
  SDK sdist, CPython archive, and locked wheels are verified cache inputs to
  the separately built sidecar executable;
the staged Codex package contains that executable, its extracted `codex`
binary, legal files, and final SBOM so it remains below the substrate's
declared-resource bound.

Use `--offline` with `verify.ts` to check an already prepared tree without any
network access. Offline verification checks every package file, generated
runtime manifest, legal resource, and publisher-evidence hook. It never treats
a serialized `verified` boolean as publisher proof; the platform verifier hook
must produce the evidence consumed by the native package verifier.

The repository intentionally does not carry third-party runtime binaries,
publisher signatures, or Codex's final sidecar executable. A strict release
preparation therefore fails until the release environment supplies all of
these inputs. Source/dev builds remain valid because the Tauri resource
directory contains only a placeholder and no runtime is registered from it.

## Commands

```sh
bun run prepare:managed-runtimes -- --target x86_64-unknown-linux-gnu --release
bun run verify:managed-runtimes -- --target x86_64-unknown-linux-gnu --offline
bun test scripts/managed-runtimes
```

The first command intentionally requires a cache populated with exact source
files and publisher evidence. Cache files are named by the pinned artifact
digest and manifest, and are never resolved by package name, version range, a
`latest` URL, or PATH.

## Publisher evidence still required

- Claude's detached `manifest.json.sig` must be checked against Anthropic's
  pinned signing key/fingerprint, then the platform artifact must pass Apple
  Developer ID + notarization, Windows Authenticode, or the signed Linux
  release-manifest hook selected by the target.
- OpenCode publishes immutable release archives and recorded GitHub digests,
  but this checkout has no publisher signature/attestation URL or trusted
  verification key for the `PlatformPackageSignature` scheme. An external
  release verifier must supply and verify that evidence for each archive.
- Codex's PyPI wheels and python-build-standalone archives have exact URLs and
  SHA-256 values here, but target Sigstore bundles/identity evidence are not
  checked into this repository. A trusted offline Sigstore verifier must supply
  those bundles, as well as a built target sidecar executable, a CycloneDX
  target SBOM, and complete third-party legal files (each inventory entry must
  bind its expected license expression, paths, and SHA-256 digests).
