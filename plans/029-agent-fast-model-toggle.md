# Plan 029: Fast model toggle across agent providers

> **Executor instructions**: Add a consistent **Fast** option to Alfred's agent
> model picker wherever the installed provider exposes a paired fast variant.
> Preserve native CLI model IDs, keep backward compatibility for saved
> workflows, and show the toggle only when pairing confidence is high. Run every
> verification command and update the status in `plans/README.md` when complete.
>
> **Drift check (run first)**:
> `git diff --stat HEAD -- src-tauri/src/agents/models.rs src-tauri/src/agents/cursor.rs src-tauri/src/agents/claude_code.rs src-tauri/src/agents/codex.rs src-tauri/src/agents/opencode.rs src/features/workflow/models.ts src/features/workflow/components tests`

## Status

- **Priority**: P2
- **Effort**: M (2–4 days)
- **Risk**: LOW — additive metadata and UI; execution already passes `--model`
- **Depends on**: — (independent of Connected Apps, licensing, memory tracks)
- **Category**: agent UX / model discovery
- **Planned at**: 2026-08-19

## Why this matters

Several agent providers expose faster model variants that cost more usage but
respond quicker. Cursor already documents this in its model catalog (`fast`
parameter on variants, `*-fast` legacy slugs). Codex and other CLIs often expose
similar slugs. Today Alfred either hides those variants or lists them as
separate rows, which makes the picker noisy and makes it hard to flip between
base and fast for the same model family.

Users expect one model row (e.g. **Cursor Grok 4.5**) with an optional **Fast**
control, matching Cursor's own picker semantics.

## Product decisions

- **One row per base model** when base ↔ fast pairing is confident.
- **Fast toggle/chip** appears only when `supportsFastToggle === true`.
- Toggle swaps the effective model ID between `baseId` and `fastVariantId`.
- **Persist the resolved concrete ID** in workflow JSON (e.g.
  `cursor-grok-4.5-high-fast`), not an abstract boolean flag alone.
- **Custom model entry** (`allowCustom`) is unchanged; no toggle when the user
  types an arbitrary model string.
- **No silent migration** of existing workflows: saved IDs keep working; on load,
  infer toggle state from the saved id (ends with `-fast` or matches
  `fastVariantId`).
- **Strict pairing**: if pairing is uncertain, fall back to today's flat list
  (safe default).
- **Parameterized overrides** (e.g. `claude-opus-4-8[effort=high,fast=false]`)
  remain custom-entry only in v1; the toggle uses discovered slugs.

## Current state (2026-08-19 audit)

| Provider | Discovery source | Fast signal | Gap |
|----------|------------------|-------------|-----|
| **Cursor** | `agent models` CLI; IDE `availableDefaultModels2.variants` | `variants[].legacySlug`, `parameterValues` with `id: "fast"`, `value: "true"`; many `*-fast` slugs | Variant IDs partially surfaced in discovery; no UI toggle |
| **Codex** | `~/.codex/models_cache.json` | Slugs like `gpt-5.2-fast`, `gpt-5.3-codex-high-fast` | Not paired with base models |
| **Claude Code** | Static aliases (`sonnet`, `opus`, …) | Unclear / likely none in v1 | Defer until CLI exposes fast slugs |
| **OpenCode** | `opencode models` | Possible `-fast` suffix in provider/model ids | Needs live output audit |

Verified locally on macOS:

- `agent models` lists 70+ `*-fast` entries including
  `cursor-grok-4.5-high-fast`, `composer-2.5-fast`, `gpt-5.2-fast`.
- Cursor IDE state stores per-model `variants` with `parameterDefinitions` for
  `fast` (boolean: "Significantly faster but consumes more usage").

## Target behavior

### Backend catalog shape

Extend `ModelOption` (Rust + frontend types):

```ts
{
  id: string
  label: string
  description: string
  baseId?: string
  fastVariantId?: string
  isFastVariant?: boolean
  supportsFastToggle?: boolean
}
```

Rules:

- Catalog rows shown in the picker are **base models** when paired.
- `supportsFastToggle: true` implies both `baseId` and `fastVariantId` are set.
- Do not emit duplicate picker rows for base and fast when paired.
- serde defaults keep old clients working if fields are absent.

### UI

In the workflow agent step model picker:

```
[ Model ▼ Cursor Grok 4.5 ]   [ Fast ☐ ]
```

- Toggle visible only for the selected model when `supportsFastToggle`.
- Off → `baseId`; On → `fastVariantId`.
- Optional "Fast" badge on the row when enabled.
- Grouped list still sorted as today; no second "Fast" duplicate entry.

### Execution

No adapter changes required if agents already receive `--model <id>`. Verify
Cursor, Codex, Claude, and OpenCode adapters pass the stored id unchanged.

## Implementation phases

### Phase 1 — Schema and shared pairing helper

**Files**

- `src-tauri/src/agents/models.rs`
- `src/features/workflow/models.ts`

**Tasks**

1. Add optional fast metadata fields to `ModelOption` / `ModelOption` TS type.
2. Implement `pair_fast_variants(models: Vec<ModelOption>) -> Vec<ModelOption>`:
   - Detect fast ids: suffix `-fast`, or explicit `isFastVariant` from mapper.
   - Match base ↔ fast by stripping `-fast` or using provider-specific keys.
   - Set `supportsFastToggle` only when exactly one confident pair exists.
3. Unit tests: pairing, deduplication, ambiguous cases, missing half-pair.

### Phase 2 — Provider mappers

#### Cursor (highest confidence)

**Source**: IDE `availableDefaultModels2` (primary), CLI `agent models` (fallback).

**Mapper rules**

- For each agent-capable model, read `variants[]`.
- For each variant, read `legacySlug` or `variantStringRepresentation`.
- Identify fast variants via `parameterValues` where `id == "fast"` and
  `value == "true"`, or slug ends with `-fast`.
- Pair with the non-fast variant sharing the same base name / effort tier.
- Prefer IDE variant labels for display (`displayNameOutsidePicker`).

**Default model resolution**

- IDE may store base id (`grok-4.5`) while picker exposes variant slug
  (`cursor-grok-4.5-high-fast`). Map selected default to best matching catalog
  row without rewriting stored workflow ids.

#### Codex

**Source**: `~/.codex/models_cache.json`.

**Mapper rules**

- Pair `slug` with `slug-fast` when both exist and labels align.
- Handle nested patterns: `gpt-5.3-codex-high` ↔ `gpt-5.3-codex-high-fast`.

#### OpenCode (Phase 2b)

**Source**: `opencode models` stdout.

**Mapper rules**

- Scan for `-fast` in `provider/model` ids.
- Pair only when stripping `-fast` yields an existing base id in the same list.
- If pattern is inconsistent across providers, leave flat list.

#### Claude Code (Phase 2b / optional)

**Source**: static aliases today.

**Decision**

- Ship v1 with `supportsFastToggle: false` unless discovery finds explicit
  fast slugs.
- Document "not supported" in catalog error/description if users expect it.

### Phase 3 — Frontend picker

**Files** (exact component paths to confirm during implementation):

- Workflow step / agent config component that renders the model `<select>`
- `src/features/workflow/models.ts` helpers

**Tasks**

1. When rendering options, use paired catalog rows (base only).
2. Add Fast toggle beside model select when selected option has
   `supportsFastToggle`.
3. On toggle change, update step's stored `model` field to `baseId` or
   `fastVariantId`.
4. On load, set toggle checked when saved `model` equals `fastVariantId` or
   ends with `-fast` and matches the pair.
5. Hide toggle for custom/free-text model ids.

### Phase 4 — Verification

**Automated**

- Rust: `cargo test --manifest-path src-tauri/Cargo.toml models:: --lib`
- Frontend: tests for toggle visibility, id swap, restore from saved id
- Full gate: `bun run check` on integration branch

**Manual smoke**

1. Refresh agent models in Alfred.
2. Cursor: select Grok 4.5 → enable Fast → confirm stored id is `*-fast`.
3. Run a Cursor agent step → confirm CLI invocation uses fast slug.
4. Codex: repeat for a known fast pair from cache.
5. Open existing workflow with non-fast id → toggle off, run unchanged.
6. Open existing workflow with fast id → toggle on after reload.

## Rollout order

1. **Cursor + Codex** — explicit `*-fast` slugs and IDE variants.
2. **OpenCode** — after one live `opencode models` audit on macOS/Linux/Windows.
3. **Claude Code** — only if fast slugs are confirmed; otherwise document deferral.

## Success criteria

- Cursor models like **Cursor Grok 4.5** expose Fast without hunting `*-fast`
  in a long dropdown.
- Codex fast variants pair cleanly where cache provides them.
- Workflows saved before this change run with the same model id.
- Providers without fast support behave exactly as today.
- No duplicate base/fast rows when pairing succeeds.

## Explicitly out of scope (v1)

- Effort / thinking / context parameter toggles (Cursor `effort=high`, etc.).
- Auto-enabling Fast based on usage or latency heuristics.
- Showing Fast as a separate top-level model row when pairing exists.
- Rewriting all saved workflows to normalize ids.

## Explicitly deferred

- Claude Code fast variants until CLI/catalog documents them.
- OpenCode pairing until provider naming is validated across installs.
- Usage-bar integration (Fast affects usage cost; separate from picker UX).

## Verification commands

```bash
# Discovery sanity (host with agents installed)
agent models | rg -i 'fast|grok-4.5'
test -f ~/.codex/models_cache.json && jq -r '.models[].slug' ~/.codex/models_cache.json | rg fast

# Unit tests
cargo test --manifest-path src-tauri/Cargo.toml models:: --lib
bun test

# Full gate
bun run check
```

## README index entry

Add under **Track H — Agent model picker** in `plans/README.md`:

| Plan | Title | Priority | Effort | Depends on | Status |
|------|-------|----------|--------|------------|--------|
| 029 | Fast model toggle across agent providers | P2 | M | — | TODO |
