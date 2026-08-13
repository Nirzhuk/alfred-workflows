# 020 — Give the active node a living signal

**Implementation status:** DONE — 2026-08-11. Automated and static motion
verification is checked below; live recording/theme/zoom feel checks remain
unchecked.

- **Commit:** unavailable — this workspace has no usable `HEAD`; use the drift
  hashes below
- **Severity:** MEDIUM
- **Category:** Cohesion, hierarchy & spatial consistency
- **Estimated scope:** 8 files, ~180–280 lines
- **Depends on:** Plans 018 and 019

## Problem

The current execution hierarchy is backwards: the real working node receives a
static border, while a detached dot in the activity panel performs an infinite
expanding pulse. That makes the user search away from the canvas and gives the
node no provider identity or inspection affordance.

Do not add a beam along edges. Edges represent dependencies, and a traveling
beam would imply measurable transfer/progress the runner does not have. Use a
node-local “living signal”: a compositor-only orbit around the active provider
mark, a quiet opacity cadence around the node perimeter, and stable text that
opens the console from Plan 019.

## Where

| File | Lines | What's there |
| --- | --- | --- |
| `src/features/workflow/components/workflow-canvas/workflow-canvas.tsx` | 380–400 | Adds precise `rf-node-running` and `rf-node-active` classes |
| `src/features/workflow/components/agent-node/agent-node.tsx` | 6–22 | Renders `AgentMark` without run state |
| `src/features/workflow/components/agent-mark/agent-mark.tsx` | 22–43 | Reusable masked provider mark has no activity variant |
| `src/App.css` | 2327–2352 | Provider mark is static and clips overflow |
| `src/App.css` | 3732–3739 | Active/running node is a static border/shadow |
| `src/App.css` | 3800–3830 | Detached panel dot repaints an expanding box-shadow forever |
| `src/features/workflow/components/workflow-list/workflow-list.tsx` | 308–322 | Running state is reduced to a workflow boolean |
| `src/features/workflow/components/workflow-list-item/workflow-list-item.tsx` | 137–181 | Sidebar renders every provider mark with no active-provider identity |
| `src/features/workflow/components/workflow-list/workflow-list.tsx` | 316–321 | “running” replaces a schedule label but keeps the clock metaphor |

### Current code

```tsx
// src/features/workflow/components/agent-mark/agent-mark.tsx:22
type Props = {
  provider: AgentProviderId;
  size?: number;
};
```

```css
/* src/App.css:3732 */
.react-flow__node.rf-node-active .wf-node {
  border-color: var(--accent);
  box-shadow: var(--shadow-node), 0 0 0 3px var(--accent-mid);
}

.react-flow__node.rf-node-running .wf-node {
  border-color: var(--accent-edge-strong);
}
```

```css
/* src/App.css:3811 */
.run-pulse {
  width: 0.7rem;
  height: 0.7rem;
  border-radius: 999px;
  background: var(--accent);
  box-shadow: 0 0 0 0 rgba(31, 111, 99, 0.45);
  animation: pulse 1.4s ease-out infinite;
}
```

## Drift gate

Before implementing, run:

```bash
sha256sum src/features/workflow/components/agent-mark/agent-mark.tsx \
  src/features/workflow/components/agent-node/agent-node.tsx \
  src/features/workflow/components/workflow-canvas/workflow-canvas.tsx \
  src/features/workflow/components/workflow-list/workflow-list.tsx \
  src/features/workflow/components/workflow-list-item/workflow-list-item.tsx \
  src/features/workflow/components/run-activity-panel/run-activity-panel.tsx \
  src/features/workflow/components/node-output-preview/node-output-preview.tsx \
  src/App.css
```

Reviewed-at hashes:

```text
d031ede22c6fd26e7361b8806b9b492fd74e59a036838ec79341ab930fda1cec  src/features/workflow/components/agent-mark/agent-mark.tsx
3dc949d43274013f8cc215f09a83879d38a31cda4f67d20c1365201cff745e44  src/features/workflow/components/agent-node/agent-node.tsx
8b59e142a242fbd559096f78a0dc9a91641375dc1379ec2f2dc2f84937a5289e  src/features/workflow/components/workflow-canvas/workflow-canvas.tsx
7225180a238d45a0c5d5c4ce462200d5d84df671b551213da68b15c9b630028c  src/features/workflow/components/workflow-list/workflow-list.tsx
3e7f03aeb28b7c8736b975a1acc3731c15b0ed567a94b263cd98b55621e5aa44  src/features/workflow/components/workflow-list-item/workflow-list-item.tsx
3b6d87b2a9fcd9e9140031cfb10915e26339411801302a6ff7f255f2780ccf88  src/features/workflow/components/run-activity-panel/run-activity-panel.tsx
b145e6ff2935c6b6fe9c05019d6e5195b861ca2ab89410dfa93e061679b1e23c  src/features/workflow/components/node-output-preview/node-output-preview.tsx
f2892ff59cc2c7fc174a4fb5cf38e07a3cceec711497e14fc524574b2b44a47f  src/App.css
```

Plans 018–019 intentionally change the workflow canvas, run panel, node output,
and CSS hashes. Reconcile against their completed console/activity state; do not
restore old code.

## Target

### Activity-aware provider mark

Add `running?: boolean` to `AgentMark` and produce an accurate accessible label:

```tsx
const accessibleLabel = running ? `${label}, working` : label;

<span
  className={`agent-mark agent-mark-${provider}${running ? " is-running" : ""}`}
  title={accessibleLabel}
  aria-label={accessibleLabel}
  style={{ width: size, height: size }}
>
  <span className="agent-mark-glyph" style={glyphStyle} aria-hidden />
</span>
```

Do not rotate, scale, or distort the brand glyph. Animate a separate ring around
it:

```css
.agent-mark.is-running {
  position: relative;
  overflow: visible;
}

.agent-mark.is-running::after {
  content: "";
  position: absolute;
  inset: -3px;
  border: 1.5px solid transparent;
  border-top-color: currentColor;
  border-right-color: color-mix(in srgb, currentColor 42%, transparent);
  border-radius: 999px;
  pointer-events: none;
  animation: agent-mark-orbit 900ms linear infinite;
}

@keyframes agent-mark-orbit {
  to {
    transform: rotate(360deg);
  }
}
```

**Why these values:** 900ms is fast enough to read as active computation rather
than a sleepy loader; `linear` is correct for constant rotation; `inset: -3px`
keeps the mark legible at 14–16px; a separate layer preserves every provider's
brand shape and uses only a compositor transform.

### Active-node perimeter

Keep the existing strong active border, but replace the static 3px box-shadow
and detached pulse with one contained perimeter layer:

```css
.react-flow__node.rf-node-running .wf-node::before {
  content: "";
  position: absolute;
  inset: -4px;
  border: 1px solid var(--accent);
  border-radius: calc(var(--radius) + 4px);
  opacity: 0.28;
  pointer-events: none;
  animation: active-node-cadence 1800ms
    cubic-bezier(0.645, 0.045, 0.355, 1) infinite alternate;
}

@keyframes active-node-cadence {
  to {
    opacity: 0.72;
  }
}
```

Use the on-screen back-and-forth curve
`cubic-bezier(0.645, 0.045, 0.355, 1)` from the audit catalog. The 1800ms
cadence is deliberately slower than the 900ms provider orbit: the icon conveys
continuous work; the perimeter only helps locate the node and must not compete
with text. Both animate transform/opacity only—never box-shadow, width, or a CSS
variable on the React Flow parent.

When the node completes or fails, both loops stop immediately. Existing static
completed/failed borders remain the outcome signal. Do not add a celebration or
completion bounce.

### Correct provider identity in every surface

1. In `AgentNode`, select only `stepStatuses[id]` from Zustand and pass
   `running={status === "running"}` to its `AgentMark`.
2. In `WorkflowCanvas`, derive
   `runningProviderByWorkflowId: Record<string, AgentProviderId | null>` from
   each `workflowRunStates[workflowId].activeNodeId` and that workflow's graph.
   Only an active node of type `agent` produces a provider. A shell, HTTP, or
   other active node produces `null`.
3. Pass that map through `WorkflowList` to `WorkflowListItem`.
4. In the sidebar, apply `running` to exactly the mark whose provider matches
   the active provider. Never animate every agent icon in a running workflow;
   that falsely communicates parallel execution.
5. Replace the clock glyph/label substitution for active execution. Normal
   schedules keep the clock and schedule label. A running workflow displays a
   dedicated `Running` chip beside the correctly animated provider mark; if the
   active node is not an agent, show a static accent dot plus `Running`.
6. In the activity panel's “Now running” card, remove `.run-pulse` and its
   paint-heavy box-shadow keyframes. Show the active provider's `AgentMark
   running` when applicable, otherwise the same static accent dot.

The console trigger from Plan 019 remains visible text—`Working · Open console
→`—so motion is never the only status indicator.

### Reduced-motion variant

Add this to the existing `prefers-reduced-motion: reduce` block:

```css
.agent-mark.is-running::after,
.react-flow__node.rf-node-running .wf-node::before {
  animation: none;
}

.agent-mark.is-running::after {
  transform: rotate(135deg);
  opacity: 0.8;
}

.react-flow__node.rf-node-running .wf-node::before {
  opacity: 0.62;
}
```

Reduced motion keeps a representative ring and perimeter plus the stable
`Working` text. Nothing moves, and the meaning survives.

## Conventions to follow

- `AgentMark` is already shared by canvas nodes and sidebar cards; extend it
  instead of copying provider-specific icons.
- `WorkflowCanvas.displayNodes` already computes the authoritative
  `rf-node-running` class. Do not add a parallel timer or local active-node
  state.
- `src/App.css:1924` already uses
  `cubic-bezier(0.645, 0.045, 0.355, 1)` for on-screen motion; reuse the exact
  curve.
- Keep all React Flow edges `animated: false` as configured in
  `flow-editor.tsx`. The node owns processing; the edge owns dependency.

## Steps

1. Extend `AgentMark` with the accessible `running` state and add its orbit
   layer plus reduced-motion treatment.
2. Wire the canvas agent mark to the node's exact running status with a narrow
   Zustand selector.
3. Add the contained active-node perimeter and remove the old detached
   `.run-pulse` animation/keyframes.
4. Derive and pass the exact active provider through the workflow sidebar; split
   scheduled and running chips so clock and execution states no longer collide.
5. Reuse `AgentMark running` in the “Now running” card and preserve the stable
   text/console trigger from Plan 019.
6. Verify light/dark themes, all four provider marks, non-agent active nodes,
   completion/failure, and reduced motion.

## Out of scope

- Animated edges, beams, particles traveling between nodes, or progress
  percentages.
- Rotating/scaling provider glyphs themselves.
- Parallel-node animation; the current runner is topological and exposes one
  `activeNodeId`.
- Celebration, bounce, confetti, blur, or large glow effects.
- Changing provider colors, node layout, or graph zoom behavior.
- Do not introduce a new animation library.
- Do not change any unrelated component's timing.

## Verification

**Build**

- [x] `bun test tests/store.test.ts`
- [x] `bun run build:frontend`
- [x] Plans 018–019 verification remains green.

**Behavior**

- [x] Exactly one canvas node has the living perimeter at a time.
- [x] Exactly one provider mark animates in a multi-agent sidebar workflow, and
  it matches the active agent node.
- [x] Non-agent steps show the node perimeter and `Running` chip without
  animating an unrelated provider.
- [x] Clicking `Working · Open console` still opens the correct node stream.
- [x] Completion/cancellation/failure stops both loops immediately and retains
  the correct static outcome border.
- [x] The Activity panel no longer renders the old expanding box-shadow pulse.

**Feel**

- [ ] Record a 30-second run at 60fps and scrub frame by frame. The provider
  orbit must stay crisp; the node text and border must not shift by 1px.
- [ ] Watch a five-node handoff at normal speed. Attention should move because
  the old node becomes static and the new node becomes living—not because a
  beam traverses the edge.
- [ ] Leave a node running for two minutes. The 1800ms perimeter must remain
  quiet enough to ignore while reading; if it dominates, lower only its target
  opacity from 0.72 to 0.58, not its duration.
- [x] With `prefers-reduced-motion: reduce`, neither ring nor perimeter moves;
  the active node remains unmistakable from static ring, border, and text.
- [ ] Check the real Tauri app in both themes at 0.6× and 1× React Flow zoom.

## Notes

The two motion layers serve different purposes: the fast orbit says “this
provider is alive,” while the slow contained perimeter says “this is the node
to look at.” If they read as one busy spinner in the feel-check, keep the orbit
and make the perimeter static at opacity 0.52; do not add more motion.
