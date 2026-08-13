# Contributing to Agentflow

Thanks for helping improve Agentflow. Bug reports, focused feature proposals,
documentation fixes, tests, and code contributions are welcome.

By submitting a contribution, you confirm that you have the right to submit it
and agree that it may be distributed under the repository's
[GPL-3.0-or-later license](LICENSE).

## Before making a change

1. Search existing issues and pull requests to avoid duplicate work.
2. Open an issue before a large feature, data-model change, new network
   integration, or user-visible policy change.
3. Never include credentials, private prompts, model output, personal file
   paths, customer data, signing material, or a real workflow database in an
   issue or pull request.
4. Use [private vulnerability reporting](SECURITY.md) for security problems.

Small fixes and documentation improvements can go directly to a pull request.

## Development setup

Follow [docs/building-from-source.md](docs/building-from-source.md), then run:

```bash
bun install --frozen-lockfile
bun run dev
```

Agentflow is a desktop-only Tauri app. Do not add Android/iOS targets or treat
the Vite frontend as an independently deployable website.

## Project structure

| Path | Purpose |
| --- | --- |
| `src/` | React/TypeScript desktop UI |
| `src/features/` | Feature-specific UI, state, and frontend APIs |
| `src/components/` | Reusable UI components |
| `src-tauri/src/` | Rust commands, persistence, triggers, and agent processes |
| `tests/` | Existing frontend behavior tests |
| `docs/` | User, source-build, and maintainer documentation |
| `plans/` | Design and implementation handoff documents, not shipped features |

Shared React components live in
`src/components/<kebab-name>/`; feature components live in
`src/features/<feature>/components/<kebab-name>/`. Keep component files,
tests, and public `index.ts` exports together. Exported component names use
PascalCase; folders and filenames use kebab-case.

## Pull-request expectations

- Keep each pull request focused and explain the user-visible outcome.
- Add or update tests for behavior changes.
- Preserve local-first behavior and redact secrets from errors, logs, reports,
  fixtures, and screenshots.
- Do not weaken Tauri capabilities, the content security policy, loopback-only
  webhook defaults, or command validation without a documented security reason.
- Do not add production dependencies when a platform or standard-library
  capability is sufficient.
- Update user docs when behavior, requirements, or supported platforms change.
- Do not commit generated output such as `dist/`, `src-tauri/target/`, local
  databases, logs, certificates, or `.env` files.

Before opening a pull request, run:

```bash
bun run check
```

Repository-wide `cargo fmt --check` is not currently a merge gate because of
pre-existing formatting drift. Format only the Rust you meaningfully change;
avoid broad formatting-only diffs.

## Commit and review notes

Write commit subjects in the imperative mood (for example, `Handle missing
Codex authentication`). In the pull request, call out:

- the problem and chosen behavior;
- important tradeoffs or security boundaries;
- verification performed; and
- screenshots for meaningful visual changes.

Maintainers may ask to split unrelated changes or revise a proposal before
merging. Be respectful and follow the [Code of Conduct](CODE_OF_CONDUCT.md).
