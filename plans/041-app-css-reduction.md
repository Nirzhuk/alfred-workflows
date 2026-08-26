# Plan: Split `src/App.css` into per-window and per-feature stylesheets

> **Executor instructions**: This is a mechanical refactor with no visual intent.
> No rule may change its declarations, its order relative to other rules, or its
> specificity. Every step is independently shippable and independently
> revertable. Run the verification commands for a step before starting the next
> one. Do not batch Step 3's blocks.
>
> **House note**: filed as Plan 041 in `plans/`, indexed under Track J in
> `plans/README.md`.
>
> **Drift check (run first)**:
> `git diff --stat -- src/App.css src/App.tsx src/main.tsx src/features/quick-access tests/design-system.test.ts docs/design-system.md .claude/rules/design-system.md .cursor/rules/design-system.mdc`
>
> `src/App.css` is a live merge surface. It was dirty when this plan was
> measured and Step 1 is already partly in flight (see Status). Re-measure
> before quoting any number back at a reviewer.

## Status

- **Priority**: P2
- **Effort**: M (1.5–2 days)
- **Risk**: LOW–MEDIUM — no behavior change, but 6 test files read
  `src/App.css` as a single string and will silently lose coverage if the split
  lands without updating them
- **Depends on**: nothing
- **Category**: maintainability / build structure
- **Measured at**: 2026-08-26, worktree dirty (`M src/App.css`,
  136 insertions / 60 deletions unstaged), `dist/` built the same day
- **Steps 1–2 status**: **IN PROGRESS — the CSS half has already landed** in
  the working tree (uncommitted) via a parallel change. Do not re-do it. As of
  2026-08-26 07:21 the tree contains:

  | File | Lines | Bytes |
  |---|---|---|
  | `src/styles/tokens.css` (new) | 345 | 13,498 |
  | `src/styles/base.css` (new) | 315 | 7,814 |
  | `src/features/quick-access/quick-access.css` (new) | 518 | 11,570 |
  | `src/App.css` (was 9,892 / 214,234) | 8,751 | 183,024 |

  `src/App.css:1-2` now `@import`s tokens and base; `quick-access.css:8-9`
  imports the same two and the popover imports only `./quick-access.css`.
  **The test and documentation half is not done and the suite is red.** Finish
  that before Step 3 — see Step 2.

## Why this matters

Not for the reason people assume. State the real costs:

**This is a local desktop app with no network.** The built stylesheet gzips to
26.5 KiB and is read off local disk by a bundled WebView. **26.5 KiB is not a
download problem and nobody should optimize it as one.** There is no CDN, no
cold cache, no 3G user. Any argument for this work that starts with "bundle
size" is the wrong argument.

The three real costs:

**1. The same 170.7 KiB stylesheet is a load dependency of both windows.**
`src/App.css` is imported at exactly two sites:

| Site | Window |
|---|---|
| `src/App.tsx:20` | main |
| `src/features/quick-access/quick-access-popover.tsx:29` | quick-access |

`src/main.tsx:14-40` splits the two roots behind `React.lazy`, so Vite emits two
independent lazy chunks — and attaches the same stylesheet to both. This is
visible in the built preload map (`dist/assets/index-s__XhevD.js`):

```
__vite__mapDeps = [
  0: assets/quick-access-popover-BQ5ohShO.js   10,287 B
  1: assets/App-Ccp31CFd.js                    10,155 B
  2: assets/App-Moog9Win.css                  174,839 B   <-- shared
  3: assets/App-CgaXVNjW.js                   587,635 B
  4: assets/App-BnuhLJ6X.css                   15,869 B   (react-flow)
]

quick-access root -> mapDeps([0, 1, 2])
main root         -> mapDeps([3, 1, 2, 4])
```

Index `2` appears in both. Each Tauri window is its own WebView with its own
document and its own CSSOM, so those 1,342 rules are tokenized, parsed, and
built into a style tree **twice per launch** — once per window. The Quick Access
popover renders **29 distinct class names**, every one of them prefixed
`quick-access-`. It parses 1,342 rules to use 64 of them.

The mechanism already works correctly elsewhere in this repo, which is the proof
that Step 3 pays off: `@xyflow/react/dist/style.css` is imported from
`src/features/workflow/components/workflow-canvas/workflow-canvas.tsx:5`, so
Vite attached it to chunk `3` only. The Quick Access window never loads those
15,869 bytes. That is exactly the outcome this plan reproduces for Alfred's own
CSS.

**2. One 9,892-line file is the merge surface for roughly 50 features.**
Every feature branch that touches UI edits the same file. `plans/README.md`
already records this: the Track G planning baseline instructs executors to
"preserve overlapping edits in `runner/mod.rs`, `lib.rs`, `App.css`, and this
index." App.css is named in the same breath as the two largest Rust files.

**3. `.claude/rules/design-system.md` requires reading it before any UI work,
and it no longer fits in working memory.** The rule says to "Reuse semantic CSS
custom properties from `src/App.css`" and to resolve every `font-size`,
`padding`, `margin`, and `gap` to a token. `docs/design-system.md:5` names
`src/App.css` as the canonical implementation. Following that rule literally
means reading 214 KB before changing a button. In practice nobody does, which is
how near-miss literals like `0.72rem` get in — the exact drift
`tests/design-system.test.ts` exists to catch.

## Current state

Two baselines. The **pre-split** column is the planning baseline every number in
this document is derived from; the **now** column is the working tree after
Steps 1–2's CSS half landed on 2026-08-26 07:21. Step 3's prefix analysis is
unaffected — none of its blocks has moved yet.

| Metric | Pre-split | Now |
|---|---|---|
| `src/App.css` | 9,892 lines, 214,234 B | 8,751 lines, 183,024 B |
| `src/styles/tokens.css` | — | 345 lines, 13,498 B |
| `src/styles/base.css` | — | 315 lines, 7,814 B |
| `src/features/quick-access/quick-access.css` | — | 518 lines, 11,570 B |
| Stylesheet files under `src/` | 1 | 4 |
| CSS-contract tests | green | **6 red** |

The wider suite reports `13 fail / 401 pass` on this tree, but only the 6 above
are attributable to the split; the rest belong to other uncommitted work in the
same worktree. Re-run on a clean branch before treating any other failure as
this plan's problem.

Pre-split detail, which is what Steps 3–4 plan against:

| Metric | Value |
|---|---|
| `src/App.css` | 9,892 lines, 214,234 bytes |
| Built (`dist/assets/App-Moog9Win.css`) | 174,839 B = 170.7 KiB |
| Gzipped | 27,147 B = 26.5 KiB |
| Rule-start lines at column 0 | 1,342 |
| Rule-start lines nested in `@media` | 153 |
| Selectors after comma-split | 1,548 (1,325 unique) |
| Custom properties in `:root` | 172 |
| `@media` queries | 26 |
| `@font-face` | 2 |
| Files | 1 |
| Import sites | 2 |
| Test files reading it as one string | 6 (9 read sites) |

Selector share by prefix. Reproduce with:

```
grep -oE '^\.[a-zA-Z][a-zA-Z0-9-]*' src/App.css | sed 's/^\.//' \
  | awk -F- '{if(NF>1)print $1"-"$2; else print $1}' | sort | uniq -c | sort -rn | head -25
```

```
quick-access          64  ################################
agent-usage           63  ###############################
connection-tutorial   54  ###########################
wf-node               53  ##########################
workflow-card         36  ##################
ui-menu               33  ################
settings-sidebar      31  ###############
memories-inspector    27  #############
react-flow            26  #############
run-memory            24  ############
workflow-folder       22  ###########
run-console           21  ##########
tutorial-wizard       20  ##########
model-select          20  ##########
wf-attach             18  #########
wf-input              17  ########
skill-picker          16  ########
memories-list         16  ########
workflow-tab          15  #######
memories-link         15  #######
memories-detail       15  #######
license-overview      14  #######
sidebar-header        13  ######
run-step              13  ######
field                 13  ######
                     ---
top 25 prefixes      659  of 1,342  (49%)
```

**Read that last line before planning anything.** The top 25 prefixes cover
under half the file. The remaining 683 rules are a long tail of one-to-five-rule
prefixes. **There is no single big chunk to delete.** Deletion is not the
lever here; relocation is. Any plan that starts with "find dead CSS" will spend
a day and remove a few kilobytes.

## Constraints

**Design-system rules move with the tokens.** `docs/design-system.md:5` states
the canonical implementation "lives in the semantic custom properties at the top
of `src/App.css`". `.claude/rules/design-system.md` says to reuse them "from
`src/App.css`" and to add new ones "to `:root`". Step 2 moves that `:root`
block. Both documents — plus `.cursor/rules/design-system.mdc`, which mirrors
the Claude rule and is asserted by `tests/design-system.test.ts:455-457` — must
be updated **in the same commit as the move**, not in a follow-up. `specs.md:273`
also names `App.css`.

**This constraint has already been violated once.** The landed Step 1–2 change
moved the `:root` block without touching any of the four documents. All four
still point a reader — or a coding agent following the rule literally — at a
file that now contains zero custom properties. Close that before Step 3.

**Tokens keep their names.** 172 custom properties, same spellings, same values,
same cascade position. A rename is a separate decision with a separate diff.

**The design-system tests read a single file and must be taught the new shape.**
Nine read sites across six files:

| File | Read sites |
|---|---|
| `tests/design-system.test.ts:4` | 1 (plus `src/App.css:` labels at `:170`, `:233`) |
| `tests/modal-system.test.ts:4` | 1 |
| `tests/platform.test.ts:8` | 1 |
| `tests/app-logo.test.tsx:63` | 1 |
| `tests/sidebar-folder-context.test.tsx:5` | 1 |
| `tests/workflow-list-item.test.tsx:38,56` | 2 |

`tests/design-system.test.ts:31-37` defines `cssBlock(selector)` by searching
that one string for `\n${selector} {`. Replace the single `Bun.file` read with a
shared helper that globs and concatenates every `src/**/*.css` file, preserving
the `src/<path>:<line>` labels in failure output. **If the tests keep reading
only `src/App.css`, they will pass while silently no longer linting the moved
rules.** That is a worse outcome than a red build.

**No new literals.** This is a move, not a rewrite. If a moved rule contains an
untagged literal that the linter now catches for the first time, that is a
pre-existing bug — fix it in a separate commit so the move stays reviewable as a
pure diff.

**Both themes verified per step**, per `.claude/rules/design-system.md`.

**Duplicated tokens are the accepted cost.** Vite inlines CSS `@import` at build
time. If `quick-access.css` imports `tokens.css`, those ~8.8 KiB are inlined
into both emitted stylesheets. Total emitted CSS across both sheets goes **up**
by roughly the size of tokens + base. That is the correct trade — see Expected
outcome.

## Commands you will need

| Purpose | Command | Expected |
|---|---|---|
| Frontend tests | `bun run test:frontend` | all pass |
| Typecheck + build | `bun run build:frontend` | exit 0 |
| Diff hygiene | `git diff --check` | no output, exit 0 |
| Full gate (before merge) | `bun run check` | exit 0 |
| Confirm chunk↔CSS mapping | `grep -o '__vite__mapDeps(\[[^]]*\])' dist/assets/index-*.js` | quick-access deps no longer include the main stylesheet |
| Confirm emitted sizes | `for f in dist/assets/*.css; do echo "$f $(wc -c < "$f")"; done` | per-window sizes as expected |

## Scope

**In scope**:

- `src/App.css`
- `src/styles/tokens.css` (new), `src/styles/base.css` (new)
- `src/features/quick-access/quick-access.css` (new)
- Per-feature `*.css` beside their components under
  `src/features/*/components/<kebab-name>/`
- `src/features/quick-access/quick-access-popover.tsx` (import line only)
- The 6 test files above
- `docs/design-system.md`, `.claude/rules/design-system.md`,
  `.cursor/rules/design-system.mdc`, `specs.md`

**Out of scope**:

- Any change to a declaration, value, selector, or rule order
- Token renames
- New tokens
- `src/App.tsx` (it keeps importing `src/App.css`)
- `vite.config.ts` (no `cssCodeSplit` or chunking config is needed; the lazy
  roots in `src/main.tsx` already produce the split)
- Rust, Tauri window config, or `src/main.tsx`

## Git workflow

- Branch: `chore/app-css-split`
- One commit per step, and within Step 3 one commit per block. Never batch.
- Do not push or open a pull request unless instructed.

## Steps

### Step 1 — Extract quick-access (~45 min) · CSS DONE, VERIFICATION OUTSTANDING

Move the 64 `.quick-access*` rule-start lines (7.4 KiB minified) out of
`src/App.css` into `src/features/quick-access/quick-access.css`, and point
`src/features/quick-access/quick-access-popover.tsx:29` at it instead of
`../../App.css`.

Lowest-risk block in the file and the reason to do it first: all 29 class names
the popover renders are `quick-access-*`. It is genuinely self-contained, which
makes it the honest proof of the pattern.

Three things a naive `grep quick-access` gets wrong. **The landed change got all
three right** — verify they survive any rebase:

- **Also move** the window-scoped rules that do not start with the class prefix:
  `html[data-window="quick-access"]` and
  `html[data-platform="macos"][data-window="quick-access"]`, now at
  `quick-access.css:11-21`. Miss them and the popover loses its root sizing.
- **Do not move** `.settings-quick-access-actions` (now `src/App.css:2527`).
  Despite the substring it belongs to the Settings page in the **main** window
  (`settings-page.tsx:285`). It correctly stayed put.
- **`tests/platform.test.ts` breaks.** It reads only `src/App.css` and asserts
  on `.quick-access-compact` / `.quick-access-panel` (`:120-123`). It is red
  right now. Fix the read, do not delete the assertion.

**Verify**: `bun run test:frontend && bun run build:frontend && git diff --check`,
then rebuild and read the preload map — the quick-access root's `mapDeps` array
must no longer contain the main stylesheet's index. Open the Quick Access
popover in both themes.

### Step 2 — Split into layers (~1 h) · CSS DONE, TESTS + DOCS OUTSTANDING

Pure move. No rule edits.

| New file | Contents | Minified est. | Landed |
|---|---|---|---|
| `src/styles/tokens.css` | `:root` (172 props), `[data-theme="dark"]`, `@media (prefers-color-scheme)` | 8.8 KiB | yes |
| `src/styles/base.css` | 2 `@font-face`, reset, element typography, `:focus-visible`, control normalization | 7.1 KiB | yes |
| `src/App.css` | everything else, plus `@import` of both at the top | ~155 KiB | yes |

`src/App.css` keeps the `@import`s so **no import site changes** — `src/App.tsx`
is untouched and every existing consumer keeps working. Vite inlines the
`@import`s at build time, so this adds no request and no runtime `@import`
waterfall.

**This is the outstanding half, and it is currently red.** Measured
2026-08-26 on the working tree:

```
bun test tests/design-system.test.ts tests/modal-system.test.ts \
         tests/platform.test.ts tests/app-logo.test.tsx \
         tests/sidebar-folder-context.test.tsx tests/workflow-list-item.test.tsx

  38 pass
   6 fail          <-- all six are the single-file read
```

Failing:

| Test | Broke because |
|---|---|
| `design-system foundations > prefers Infer and bundles its Geist fallbacks…` | `@font-face` moved to `base.css` |
| `shared component contracts > names the scrolled folder in the chrome…` | rule no longer in `App.css` |
| `desktop platform contract > reserves only the OS-owned title-bar safe area` | rule moved |
| `desktop platform contract > uses one macOS wallpaper-tint layer…` | rule moved |
| `desktop platform contract > gives Quick Access the same rounded macOS material…` | moved to `quick-access.css` |
| `SidebarFolderContext > keeps the exit timeout equal to the CSS exit duration` | reads `--duration-fast`, now in `tokens.css` |

Replace the 9 single-file reads with one shared helper that globs and
concatenates `src/**/*.css`, preserving the `src/<path>:<line>` labels at
`tests/design-system.test.ts:170,233`. Note that
`tests/design-system.test.ts:31-37`'s `cssBlock(selector)` scans for
`\n${selector} {` in that one string and needs the concatenated source too.

Then update `docs/design-system.md:5`, `.claude/rules/design-system.md:10`,
`.cursor/rules/design-system.mdc:16`, and `specs.md:273` to name
`src/styles/tokens.css`. **None of these four has been updated yet** — all still
point at `src/App.css`, which no longer contains a single custom property.

**Verify**: `bun run test:frontend` back to green, and prove coverage did not
silently narrow: plant a `font-size: 0.72rem` inside `src/styles/tokens.css` and
confirm the literal linter flags it. A green run against a shrunken corpus is
the failure mode this step exists to prevent.

### Step 3 — Move feature blocks to their components (~30–45 min each)

Per `.claude/rules/component-structure.md`, feature CSS belongs next to the
component. Largest first, one commit each, full test suite between each:

| # | Block | Rules | Minified |
|---|---|---|---|
| 1 | `agent-usage` | 63 | 7.7 KiB |
| 2 | `wf-node` + `wf-attach` + `wf-input` | 88 | 9.9 KiB |
| 3 | `connection-tutorial` + `tutorial-wizard` | 74 | 9.1 KiB |
| 4 | `memories-*` (21 sub-prefixes) | 108 | 11.9 KiB |
| 5 | `workflow-card` / `-folder` / `-tab` / `-list` | 76 | 9.6 KiB |

Total: 409 rules, ~48 KiB minified, out of `src/App.css`.

Each block needs the same three-way check Step 1 documents: pull in
window/attribute-scoped rules that lack the class prefix, leave behind
substring false positives owned by another feature, and fix the test that reads
the rule you moved.

**The mechanism, precisely**: Vite attaches each stylesheet to the JS chunk that
imports it and lists it in that chunk's `mapDeps` array. A window only fetches
and parses the stylesheets in its own root's dependency list. `react-flow`'s
15,869-byte sheet already demonstrates this — it is in the main window's deps
and absent from Quick Access's. Moving `agent-usage` next to its component means
the Quick Access WebView never tokenizes those 60 rules.

Never batch these. A single commit that moves 400 rules is unreviewable and
unbisectable, and the failure mode is a specificity change nobody spots until a
user reports a wrong border in dark mode.

### Step 4 — Hunt dead rules (~2 h)

**Only now.** Use DevTools CSS coverage or `css-analyzer`, exercising each
feature surface deliberately: canvas, settings, memories, history, connected
apps, tutorials, quick access, both themes.

**Why the order is load-bearing**: run coverage today, against the current
single file, and every rule the main window does not render is reported unused —
including all 57 quick-access rules, which are perfectly live in the other
window. One window never renders the other window's markup, so a whole-file
coverage run on either window produces false positives by construction. After
Steps 1–3, each stylesheet has exactly one rendering context and coverage means
something.

Expect a small yield. The long tail is long because it is real.

**Verify**: every deletion needs a `grep` for the class name across `src/`
returning nothing, plus a visual check of the surface that owned it.

## How to verify each step

Run all four, in order, at every step:

```
bun run test:frontend      # 6 test files lint tokens, literals, and contracts
bun run build:frontend     # tsc + vite build; must stay exit 0
git diff --check           # whitespace hygiene
bun run check              # full gate incl. cargo, before merge only
```

Then confirm the mechanism actually changed, which no test asserts:

```
grep -o '__vite__mapDeps(\[[^]]*\])' dist/assets/index-*.js
for f in dist/assets/*.css; do echo "$f $(wc -c < "$f")"; done
```

**The one thing only a human can check**: open every touched surface in **both
themes** and compare against `main`. The test suite verifies that tokens are
used; it cannot verify that a moved rule still wins its cascade. Specificity and
source-order regressions are invisible to every command above.

## Expected outcome

Be precise about what does and does not improve.

**The built CSS total does not shrink.** It grows slightly. A pure split emits
the same declarations plus a duplicated copy of tokens (~8.8 KiB) and base
(~7.1 KiB) in each window's stylesheet. Do not promise a bundle win.

What changes is **per-window parse work**:

| | Before | After Steps 1–3 |
|---|---|---|
| Main window CSS | 170.7 KiB | ~115 KiB |
| Quick Access CSS | 170.7 KiB | **~24 KiB** |
| Rules parsed twice | 1,342 | ~230 (tokens + base) |
| Largest single file | 9,892 lines | ~6,000 lines |

`quick-access.css` itself is **7.4 KiB minified — under 10 KB**. The Quick
Access window's total, including the tokens and base it must still import, is
~24 KiB, down from 170.7 KiB. That is an 86% cut in what that window parses, and
it is the only size number in this plan worth quoting.

The durable wins are the two that motivated the work: UI changes stop colliding
in one file, and `docs/design-system.md`'s "canonical implementation" becomes an
8.8 KiB token file a reviewer — or a coding agent following
`.claude/rules/design-system.md` — can actually read before touching a button.

## Not doing

**CSS Modules. Tailwind. Sass, Less, PostCSS-with-plugins, any preprocessor.**

`specs.md:322` carries this as an open question: "Prefer CSS modules / Tailwind
later, or stay global `App.css`?" This plan answers it: stay global, split by
file.

Reason: the token-based design system already works. 172 semantic custom
properties, enforced by `tests/design-system.test.ts` (14 tests, including
literal-drift, fractional-font-weight, surface-role, and stacking-token
linters), documented in `docs/design-system.md`, and mirrored into two
coding-agent rule files. Swapping the styling model means rewriting all 1,342
rules, rewriting every linter that reads raw CSS text, and re-verifying every
surface in both themes — for no measurable improvement to parse time, merge
conflicts, or readability that this split does not already deliver.

**The one signal that would change the answer**: Step 3 revealing the same rule
duplicated across features. If moving `agent-usage`, `wf-node`, and
`memories-*` surfaces three near-identical copies of the same card, row, or
badge treatment, the problem is missing shared components, and a scoping model
that makes duplication visible starts to earn its cost. Record any such
duplicate you find during Step 3 — that evidence is the input to a future
decision, and this plan is not it.

## Done criteria

- [ ] `src/App.css` is under 6,500 lines.
- [ ] `src/styles/tokens.css` holds all 172 custom properties, unrenamed.
- [ ] The 6 CSS-contract tests are green again *and* a planted
      `font-size: 0.72rem` in `src/styles/tokens.css` is still flagged.
- [ ] `src/App.tsx:20` is unchanged.
- [ ] The Quick Access root's `mapDeps` array does not include the main
      window's stylesheet.
- [ ] `dist/assets/` shows a quick-access stylesheet under 30 KiB.
- [ ] All 6 test files read the concatenated stylesheet set, and the literal
      linter demonstrably still flags a planted violation in a moved file.
- [ ] `docs/design-system.md`, `.claude/rules/design-system.md`,
      `.cursor/rules/design-system.mdc`, and `specs.md` name the new token
      location.
- [ ] No declaration, value, selector, or rule order changed in any step.
- [ ] Every touched surface verified in light and dark.
- [ ] `bun run check` passes and `git diff --check` is clean.
- [ ] `specs.md:322`'s open question is resolved in favor of split-global CSS.

## STOP conditions

Stop and report if:

- a moved rule changes appearance, meaning it depended on source order — record
  the selector pair and stop rather than patching specificity;
- the design-system linters cannot be made to see multiple files without
  weakening an assertion;
- Vite emits a stylesheet into a chunk you did not expect, or the `mapDeps`
  arrays do not change as predicted after Step 1;
- a feature block cannot be moved because its selectors reach across feature
  boundaries (record which, and leave it in `src/App.css`);
- Step 3 reveals broad rule duplication — that is the Not-doing trigger above;
  stop and raise it as a separate decision;
- the parallel Step 1 / Step 2 change conflicts materially with this plan's
  contract;
- a verification fails twice after a reasonable correction.

## Maintenance notes

- New feature CSS goes beside its component from day one, per
  `.claude/rules/component-structure.md`. `src/App.css` is a shrinking legacy
  bucket, not a destination.
- New tokens go in `src/styles/tokens.css` and get documented in
  `docs/design-system.md`, unchanged from today's rule apart from the path.
- Re-measure the `mapDeps` arrays after any change to `src/main.tsx`'s lazy
  roots. That file is what makes the split effective; a static import there
  would quietly re-merge both windows' CSS.
- If a third window is ever added, it inherits this structure for free — it
  imports `tokens.css`, `base.css`, and only its own feature sheets.
