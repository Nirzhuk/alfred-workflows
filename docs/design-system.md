# Alfred design system

This document is the source of truth for Alfred's visual language. It keeps the
desktop app quiet, legible, and consistent. The canonical implementation lives
in the semantic custom properties at the top of `src/App.css`.

## Principles

- **Quiet hierarchy:** spacing, color, and placement establish structure before
  font weight does. Most interactive text is regular weight.
- **One role, one scale:** components with the same purpose share typography,
  icons, shape, spacing, and interaction states.
- **Desktop-native density:** controls stay compact while remaining clear,
  keyboard accessible, and easy to target.
- **Flat by default:** borders and shadows communicate containment, not emphasis.
- **Tokens before literals:** repeated decisions use semantic tokens. A literal
  is acceptable only for unique geometry or a documented optical correction.
- **State never changes layout:** hover, selection, focus, and loading must not
  move surrounding content or change text metrics.

## Cross-platform contract

Alfred supports macOS, Windows, and Linux. Cohesion means the application-owned
interface shares the same metrics and behavior everywhere; it does not mean
recreating OS-owned window controls or system dialogs.

- `src/platform.ts` detects the desktop family once and writes
  `data-platform="macos|windows|linux|unknown"` on the root element before the
  application UI loads.
- Platform selectors are limited to genuine OS boundaries such as window-chrome
  safe areas. Do not create platform-specific typography, spacing, radii,
  colors, or component layouts.
- macOS reserves 78px at the title bar's leading edge for traffic lights, or
  72px in compact layouts. Windows, Linux, unknown platforms, and fullscreen
  windows use the standard 12px inset.
- Geist, tokenized control metrics, custom select chevrons, checkboxes, focus
  rings, and selection colors prevent webview and OS defaults from changing the
  application-owned UI.
- Keyboard labels may use macOS symbols where appropriate. Windows and Linux use
  the equivalent Ctrl/Super terminology.
- File pickers, notifications, permission prompts, window buttons, and other
  OS-owned surfaces remain native. Their content and trigger affordances should
  still follow Alfred's terminology and state model.
- Never infer platform independently inside a component. Reuse
  `detectDesktopPlatform` or the root `data-platform` attribute.

Review at 100% scale on every supported OS. Also check Windows at 125% and 150%
display scaling, macOS at standard and Retina scale, and Linux on at least one
Wayland or X11 desktop before a visual release.

## Typography

Geist, Fraunces, and Geist Mono are bundled under `src/assets/fonts/`; the UI
must never depend on a remote font request. Geist is the default interface face,
while Fraunces is reserved for display accents.

| Role | Family | Size | Weight | Line height |
| --- | --- | --- | --- | --- |
| Page title | `--font-display` | `--text-2xl` / 24px | 600 | 1.2 |
| Dialog title | `--font-display` | `--text-xl` / 20px | 600 | 1.2 |
| Section title | `--font-sans` | `--text-lg` / 16px | 600 | 1.2 |
| Body, item, control | `--font-sans` | `--text-md` / 14px | 400 | 1.4 |
| Supporting text | `--font-sans` | `--text-sm` / 12px | 400 | 1.4 |
| Micro/status text | `--font-sans` | `--text-xs` / 11px | 500–700 | 1.2 |
| Technical content | `--font-mono` | appropriate role size | 400–600 | 1.4–1.55 |

Rules:

- `--font-sans` resolves to Geist, `--font-display` resolves to Fraunces, and
  `--font-mono` resolves to Geist Mono.
- Use Fraunces only through `--font-display` for page, dialog, and other
  intentional display accents. Body copy, controls, and navigation stay in
  Geist.
- Use only weights 400, 500, 600, and 700 through the weight tokens. Fractional
  weights such as 550 or 650 are prohibited.
- Controls and navigation use weight 400. Selected state never adds weight.
- Use 600 for headings and meaningful hierarchy. Reserve 700 for short status
  labels or urgent emphasis.
- All caps and added letter spacing are limited to short status badges. Do not
  use them for navigation or section headings.
- Numeric logs, schedules, identifiers, shortcuts, and code use Geist Mono.

## Spacing

Use the four-pixel spacing grid. Half steps exist for dense desktop alignment:

| Token | Value | Typical use |
| --- | --- | --- |
| `--space-0-5` | 2px | optical separation |
| `--space-1` | 4px | tight internal gap |
| `--space-1-5` | 6px | compact control gap |
| `--space-2` | 8px | standard internal gap |
| `--space-2-5` | 10px | icon-to-label gap |
| `--space-3` | 12px | control/card padding |
| `--space-4` | 16px | component separation |
| `--space-5` | 20px | section padding |
| `--space-6` | 24px | page padding |
| `--space-8` | 32px | large separation |
| `--space-10` | 40px | page-level separation |

Prefer tokens in component CSS. Unique layout constraints such as a panel width
or graph coordinate may remain literal.

## Shape and control density

| Role | Token | Value |
| --- | --- | --- |
| Small detail | `--radius-sm` | 4px |
| Controls and rows | `--radius-md` | 8px |
| Cards and menus | `--radius-lg` | 12px |
| Dialogs and floating panels | `--radius-xl` | 16px |
| Pills, toggles, dots | `--radius-pill` | 999px |
| Compact control | `--control-height-compact` | 28px |
| Default control | `--control-height-default` | 32px |
| Comfortable input | `--control-height-comfortable` | 36px |
| Navigation row | `--control-height-navigation` | 38px |
| Large primary control | `--control-height-large` | 40px |

Do not invent intermediate radii or control heights. A visible pointer target is
at least 28px in dense chrome and 32px for ordinary controls.

## Iconography

- Icons are Phosphor Icons (regular weight) rendered through the shared
  `Icon` component; see `docs/iconography.md` for the rules, the add-an-icon
  workflow, and the mapping of legacy inline icons.
- Use one outline icon language per surface with consistent optical stroke.
- Standard interface icons are `--icon-size-default` / 18px.
- `--icon-size-compact` / 16px is for dense secondary controls only.
- `--icon-size-prominent` / 20px and `--icon-size-display` / 24px are for
  intentional emphasis, not ordinary rows.
- Icons in a repeated list use the same bounding box. Align the box, not the
  path's visual edges.
- Icon-only controls require an accessible name and visible tooltip or title.

## Color, surfaces, and elevation

- Use semantic color tokens such as `--ink`, `--muted`, `--surface-*`,
  `--accent-*`, and `--danger-*`; never branch component CSS by theme.
- Use `--control-*` tokens for shared control states.
- Navigation and list selection use a subtle background without a new border,
  shadow, or weight change.
- Use `--elevation-raised` for small floating controls,
  `--elevation-popover` for menus, and `--elevation-modal` for dialogs.
- Do not add a shadow to a flat card or selected row. Add elevation only when an
  element sits above another interaction plane.
- Use the `--layer-*` scale from content through toast; arbitrary z-index values
  are prohibited.

## Interaction state contract

Every interactive component supports the states that apply to it:

| State | Required behavior |
| --- | --- |
| Default | Uses the role's semantic foreground, background, and border. |
| Hover | Changes color or surface only; does not change layout or weight. |
| Pressed | Uses the pressed surface; buttons may translate by 1px. |
| Selected | Uses the selected surface and retains its default text metrics. |
| Focus visible | Shows the shared two-pixel focus ring with a two-pixel offset. |
| Disabled | Uses `--disabled-opacity`, blocks activation, and keeps its label readable. |
| Error/destructive | Uses `--danger-*`; color is never the only explanation. |
| Loading | Keeps the control's dimensions stable and exposes status accessibly. |

Do not remove focus indication. `:focus-visible` is preferred so pointer clicks
do not create unnecessary rings.

## Shared component contracts

### Sidebar navigation

Workflow navigation, Settings navigation, folders, and workflow rows share:

| Role | Token | Resolved value |
| --- | --- | --- |
| Item text | `--sidebar-item-font-size` | 14px |
| Item weight | `--sidebar-item-font-weight` | 400 |
| Item color | `--sidebar-item-color` | theme ink; `#dce5e1` in dark mode |
| Item icon | `--sidebar-icon-size` | 16px square |
| Section text | `--sidebar-section-font-size` | 16px |
| Section weight | `--sidebar-section-font-weight` | 600 |
| Item minimum height | `--sidebar-item-min-height` | 32px |
| Icon-to-label gap | `--sidebar-item-gap` | 4px |
| Row-to-row gap | `--sidebar-item-stack-gap` | 2px |
| Item inline padding | `--sidebar-item-inline-padding` | 8px |

Sidebar rows are borderless. Secondary metadata may use the supporting or micro
scale but must not compete with the item label. Keep only 2px between adjacent
rows so their hover and selected surfaces read as one compact navigation stack.

### Menus

- Menu items use 14px/400, 18px icons, 32px minimum height, and 8px radius.
- Menu labels use 12px/600 without all caps.
- Hover and open states use a flat selected surface without a visible border.

### Forms and buttons

- Ordinary inputs and buttons use 14px text and 8px radius.
- Field labels use 14px/500; descriptions and hints use 12px/400.
- Primary buttons may use 600 weight. Secondary and ghost buttons use 400.
- Validation stays next to the affected field and does not rely on color alone.

### Select controls

- New or restyled select fields use the shared `SelectControl` from
  `src/components/select-control`; feature code must not add its own native
  arrow treatment or field chrome.
- Keep native select semantics, keyboard behavior, and option menus. The shared
  wrapper owns only the surface, interaction states, and single chevron.
- Use the default density in forms and Settings. Compact density is reserved for
  dense desktop chrome such as the compact Quick Access window.
- The caller owns layout width; the component owns height, padding, typography,
  border, focus, hover, and disabled states.

### Cards and dialogs

- Cards use 12px radius. Borders communicate grouping; shadows are normally off.
- Dialogs and floating panels use 16px radius and the modal elevation token.
- Dialog titles use 20px/600; page titles use 24px/600.

## Motion

- Use `--duration-fast` / 120ms for direct feedback,
  `--duration-standard` / 180ms for component transitions, and
  `--duration-slow` / 280ms for panels.
- Use `--ease-standard` for fades and color changes and `--ease-emphasized` for
  spatial transitions.
- Prefer opacity and transform. Avoid animating layout during direct manipulation.
- The global reduced-motion rule must remain in place. Critical state changes
  must remain understandable when animation is effectively disabled.

## Accessibility

- Text and meaningful icons meet WCAG AA contrast in light and dark themes.
- Keyboard order follows visual order; all actions are operable without a mouse.
- Icon-only actions have accessible names. Form controls have programmatic labels.
- Do not use color alone for status, errors, or selection.
- Verify focus, disabled, loading, empty, and error states in both themes.
- Do not replace native OS chrome or dialogs solely for visual similarity; their
  platform behavior and accessibility are part of the interaction contract.

## Implementation and review

1. Reuse an existing token or shared component before adding a literal.
2. Add repeated decisions to `:root` and document them here.
3. Do not duplicate a token under a feature-specific name.
4. Compare related surfaces at the same viewport and theme.
5. Run `bun test`, `bun run build:frontend`, and `git diff --check`.

Exceptions must be intentional, narrowly scoped, and explained beside the CSS
or in this document. Canvas geometry and third-party component constraints are
the common legitimate exceptions; visual preference alone is not.
