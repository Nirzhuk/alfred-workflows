# 019 — Open a node-scoped live console

**Implementation status:** DONE — 2026-08-11. Automated and static verification
is checked below; live motion recordings remain unchecked.

- **Commit:** unavailable — this workspace has no usable `HEAD`; use the drift
  hashes below
- **Severity:** HIGH
- **Category:** Cohesion, hierarchy & spatial consistency
- **Estimated scope:** 8 files, ~300–450 lines
- **Depends on:** Plan 018

## Problem

The canvas knows exactly which node is active, but inspection lives in a
generic “Live log” at the bottom of the fourth section in a side panel. The
panel also opens itself on every run, animating a 340px layout reflow while the
user is trying to locate the working node.

The working node should be the origin of inspection. A user who wants detail
should open a terminal-like, node-filtered live console from that node; users
who only want spatial status should be able to keep watching the canvas without
the panel forcing itself open.

## Where

| File | Lines | What's there |
| --- | --- | --- |
| `src/features/workflow/components/workflow-canvas/workflow-canvas.tsx` | 380–400 | Adds active/running CSS classes but no inspection action |
| `src/features/workflow/components/node-output-preview/node-output-preview.tsx` | 14–35 | Shared seam present in every node, but renders only after output exists |
| `src/features/workflow/components/run-activity-panel/run-activity-panel.tsx` | 121–155, 170–181 | Panel state and unconditional smooth auto-scroll |
| `src/features/workflow/components/run-activity-panel/run-activity-panel.tsx` | 421–480 | Global log is last, 220px high, and not node-scoped |
| `src/features/workflow/store.ts` | 892–968, 1002–1014, 1334–1343 | Visible runs forcibly open the panel; opener cannot select a node |
| `src/features/workflow/components/app-title-bar/app-title-bar.tsx` | 217–237 | Activity button reflects panel open/count, not a live running state |
| `src/App.css` | 613–645, 1011–1052 | Rail opens by animating layout width and panel transform symmetrically |
| `src/App.css` | 4239–4293 | Log is styled as article cards rather than a dense live console |

### Current code

```ts
// src/features/workflow/store.ts:919
set((state) => ({
  loading: true,
  error: null,
  workflowRunStates: {
    ...state.workflowRunStates,
    [targetWorkflowId]: nextRunState,
  },
  ...(state.activeWorkflowId === targetWorkflowId
    ? {
        runPanelOpen: true,
        selectedOutput: null,
        ...visibleRunFields(nextRunState),
      }
    : {}),
}));
```

```tsx
// src/features/workflow/components/run-activity-panel/run-activity-panel.tsx:152
useEffect(() => {
  if (!logOpen) return;
  bottomRef.current?.scrollIntoView({ behavior: "smooth", block: "end" });
}, [runLogs.length, logOpen]);
```

```tsx
// src/features/workflow/components/node-output-preview/node-output-preview.tsx:19
if (!output?.trim()) return null;
```

## Drift gate

Before implementing, run:

```bash
sha256sum src/features/workflow/store.ts \
  src/features/workflow/types.ts \
  src/features/workflow/components/run-activity-panel/run-activity-panel.tsx \
  src/features/workflow/components/node-output-preview/node-output-preview.tsx \
  src/features/workflow/components/workflow-canvas/workflow-canvas.tsx \
  src/features/workflow/components/app-title-bar/app-title-bar.tsx \
  src/App.css tests/store.test.ts
```

Reviewed-at hashes:

```text
3f131b29b097ab000ae4b6c6edeaa9c84cf9ecc57465eaf081969174b71706c3  src/features/workflow/store.ts
7fabeafd5ff790bb4e0a9c0ba2bb53b88a9c3927d188003fde9363c93f48f58c  src/features/workflow/types.ts
3b6d87b2a9fcd9e9140031cfb10915e26339411801302a6ff7f255f2780ccf88  src/features/workflow/components/run-activity-panel/run-activity-panel.tsx
b145e6ff2935c6b6fe9c05019d6e5195b861ca2ab89410dfa93e061679b1e23c  src/features/workflow/components/node-output-preview/node-output-preview.tsx
8b59e142a242fbd559096f78a0dc9a91641375dc1379ec2f2dc2f84937a5289e  src/features/workflow/components/workflow-canvas/workflow-canvas.tsx
40a5c63866afca3c40c7ec5f07a630afeeb7bc525f0d8987b37ba340b8ca088c  src/features/workflow/components/app-title-bar/app-title-bar.tsx
f2892ff59cc2c7fc174a4fb5cf38e07a3cceec711497e14fc524574b2b44a47f  src/App.css
ae7929f74ff0ce791d077d769ea8d50d9edfd66eb971e6b7aa3dce08006ae847  tests/store.test.ts
```

Plan 018 intentionally changes the first three hashes and the event model.
Reconcile against its final `AgentActivity`/`RunLogLine` contract rather than
restoring these hashes. Stop only if the panel/opening seams no longer exist.

## Target

### Explicit panel state

Add `inspectedNodeId: string | null` to the visible workflow run state and the
store. Change the actions to:

```ts
openRunPanel: (nodeId?: string | null) => void;
closeRunPanel: () => void;
```

Exact semantics:

- `openRunPanel(nodeId)` opens the panel filtered to that node.
- `openRunPanel(null)` opens the whole-run console.
- Calling `openRunPanel()` preserves the current filter if the panel is already
  open; otherwise it defaults to the active node, then whole run.
- `closeRunPanel()` closes the panel but preserves `inspectedNodeId` so reopening
  feels continuous.
- Selecting another workflow restores that workflow's own filter.
- Run start (`runActiveWorkflow` and `event.kind === "started"`) must not open
  the panel.
- Keep explicit opens for clicking Activity, clicking a working node, opening
  output, retrying an already-running workflow, and run-start failures.

### Node-origin trigger

Extend `NodeOutputPreview`, the shared component already mounted by every node,
into a run surface:

```tsx
const status = useWorkflowStore((s) => s.stepStatuses[nodeId]);
const openRunPanel = useWorkflowStore((s) => s.openRunPanel);

{status === "running" ? (
  <button
    type="button"
    className="wf-node-activity nodrag nopan"
    aria-label={`Open live activity for ${title}`}
    onClick={(event) => {
      event.stopPropagation();
      openRunPanel(nodeId);
    }}
    onPointerDown={(event) => event.stopPropagation()}
  >
    <TerminalGlyph />
    <span>Working</span>
    <span aria-hidden>Open console →</span>
  </button>
) : null}
```

Retain the existing completed-output preview beneath it. The visible trigger
may be 28px high to fit the node, but add an absolutely positioned `::before`
hitbox centered on it at exactly 44×44px. Keep it inside the node bounds and
above the drag surface. `data-prevent-node-double-click` must be present so the
action does not open node settings.

### Console hierarchy

Move the live console directly below the “Now running” card, before “This run,”
“Result,” and “Library.” Rename it from **Live log** to **Console** and render:

1. Header: `Console · <node label>` when filtered, or `Console · Whole run`.
2. Two compact filter buttons: `This node` and `Whole run`. `This node` is
   disabled when there is neither an inspected nor active node.
3. A scroll region, `role="log"`, `aria-live="off"`, `aria-label="Live agent
   activity"`. Do not announce every tool event.
4. Dense rows with tabular `HH:mm:ss`, activity-kind label, summary, and an
   expandable `<pre>` for `activity.detail`.
5. A sticky `Jump to latest` button shown only when the user is not following
   the bottom.

The “Now running” summary itself gets `aria-live="polite"` and
`aria-atomic="true"`; that is the only live-region announcement when steps
change.

### Follow behavior

Replace unconditional `scrollIntoView({ behavior: "smooth" })` with exact
scroll ownership:

```ts
const FOLLOW_THRESHOLD_PX = 24;
const distanceFromBottom =
  element.scrollHeight - element.scrollTop - element.clientHeight;
const following = distanceFromBottom <= FOLLOW_THRESHOLD_PX;
```

- Track `following` from the console element's `scroll` event.
- When a new row arrives and `following` is true, set
  `element.scrollTop = element.scrollHeight` directly—no smooth scrolling.
- When `following` is false, never change `scrollTop`; show `Jump to latest`.
- `Jump to latest` performs one direct jump and restores following.
- Filtering to another node jumps once to that filtered stream's latest row.
- Do not drive scroll position through React state on every frame.

### Overlay panel motion

Stop animating `width`. Keep the console from shifting the React Flow canvas:

```css
.app-shell {
  position: relative;
}

.run-rail {
  position: absolute;
  inset: 0 0 0 auto;
  z-index: 20;
  width: var(--run-w);
  overflow: hidden;
  pointer-events: auto;
}

.run-panel {
  transform: translateX(0);
  opacity: 1;
  transition:
    transform 280ms cubic-bezier(0.23, 1, 0.32, 1),
    opacity 180ms cubic-bezier(0.23, 1, 0.32, 1);
}

.app-shell.run-collapsed .run-rail {
  pointer-events: none;
}

.app-shell.run-collapsed .run-panel {
  transform: translateX(100%);
  opacity: 0;
  transition-duration: 180ms, 120ms;
}
```

Keep the existing 340px `--run-w`. The enter uses the house
`cubic-bezier(0.23, 1, 0.32, 1)` and 280ms because a large drawer travels its
full width; dismissal uses 180ms because the user has already decided to close.
Only `transform` and `opacity` animate, so opening does not reflow the canvas.

While closed, set both `aria-hidden="true"` and the native `inert` attribute on
the rail/aside; no hidden button may remain tabbable.

Under `prefers-reduced-motion: reduce`, set `transform: none` in both states and
transition opacity only for 100ms with `ease`. The panel still communicates
open/closed state without travel.

### Titlebar state

Give `AppTitlebar` separate `activityRunning` and `activityEventCount` props.
While any visible workflow run is active, the button title is `Open live
console`, its text is `Live`, and the badge counts current-run activity rows—not
memories. When idle it returns to `Activity`; memory count remains in its
existing memory surfaces.

Make the visible button at least 32px high and add an invisible centered
44×44px `::before` hitbox without overlapping native window controls.

## Conventions to follow

- `NodeOutputPreview` is the shared node-local output seam; extend it rather
  than editing every node type.
- `RunActivityPanel` already owns run step/output/library presentation. Keep the
  console there; do not create a second floating terminal window.
- `--panel-ease: cubic-bezier(0.23, 1, 0.32, 1)` at `src/App.css:616` is the
  house drawer curve.
- Use the existing monospace stack from `.workflow-card-schedule` for console
  time/kind text.

## Steps

1. Add per-workflow `inspectedNodeId` state and the optional node-aware panel
   opener; update store tests before touching UI.
2. Stop automatic panel opening on successful run start while preserving every
   explicit/error open path.
3. Extend `NodeOutputPreview` with the active-run console trigger and accessible
   hitbox.
4. Reorder and rebuild the log section as the filtered console with stable row
   keys from Plan 018's activity IDs.
5. Implement user-owned follow behavior and `Jump to latest`; delete all smooth
   per-line auto-scrolling.
6. Convert the run panel rail to a transform/opacity overlay with asymmetric
   timings, `inert`, and the reduced-motion variant.
7. Give the titlebar button a real running/event state and test both whole-run
   and node-origin opens.

## Out of scope

- Raw provider chain-of-thought or reasoning text.
- An interactive shell, stdin forwarding, or sending commands to the agent.
- Launching Terminal.app, iTerm, Windows Terminal, or a Linux terminal emulator.
  A new terminal cannot attach to the already-running child process; the
  in-app console is the attached stream.
- Persisting the console across app restarts.
- Replacing Result, Library, or output modals.
- Do not introduce a new animation, terminal-emulation, or virtualization
  library.
- Do not change unrelated component timings.

## Verification

**Build**

- [x] `bun test tests/store.test.ts`
- [x] `bun run build:frontend`
- [x] Plan 018's Rust and frontend tests remain green.

**Behavior**

- [x] Starting a run leaves the panel closed and the canvas stationary.
- [x] Clicking `Working · Open console` on the active node opens a console
  filtered to that node.
- [x] Clicking the titlebar Activity/Live control opens the whole-run console.
- [x] Closing and reopening preserves the last node filter.
- [x] Scrolling upward stops auto-follow; new rows do not steal the viewport;
  `Jump to latest` restores follow.
- [x] Hidden panel controls cannot receive Tab focus.
- [x] The active-step summary is announced politely once; each terminal row is
  not announced.

**Feel**

- [ ] Record and scrub panel open/close. The canvas must not change size or
  position; only the drawer moves.
- [ ] Open and close rapidly. The transition must retarget from its current
  transform without jumping; it must not restart from an `@keyframes` origin.
- [ ] A user reading an older command result can stay there through at least 20
  incoming events without any scroll movement.
- [ ] With `prefers-reduced-motion: reduce`, the panel fades for 100ms and never
  translates.

## Notes

The app calls this surface “Console,” not “Thinking.” It exposes observable
agent actions and safe output. That naming is both more accurate and less likely
to promise hidden reasoning providers do not expose.
