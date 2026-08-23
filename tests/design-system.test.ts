import { describe, expect, test } from "bun:test";

const root = new URL("../", import.meta.url);
const css = await Bun.file(new URL("src/App.css", root)).text();
const designSystem = await Bun.file(new URL("docs/design-system.md", root)).text();
const specs = await Bun.file(new URL("specs.md", root)).text();
const claudeRule = await Bun.file(
  new URL(".claude/rules/design-system.md", root),
).text();
const cursorRule = await Bun.file(
  new URL(".cursor/rules/design-system.mdc", root),
).text();
const selectControl = await Bun.file(
  new URL("src/components/select-control/select-control.tsx", root),
).text();
const quickAccessPopover = await Bun.file(
  new URL("src/features/quick-access/quick-access-popover.tsx", root),
).text();
const settingsPage = await Bun.file(
  new URL(
    "src/features/settings/components/settings-page/settings-page.tsx",
    root,
  ),
).text();
const flowEditor = await Bun.file(
  new URL(
    "src/features/workflow/components/flow-editor/flow-editor.tsx",
    root,
  ),
).text();

function cssBlock(selector: string): string {
  const marker = `${selector} {`;
  const lineStart = css.indexOf(`\n${marker}`);
  const start = lineStart >= 0 ? lineStart + 1 : css.startsWith(marker) ? 0 : -1;
  expect(start).toBeGreaterThan(-1);
  return css.slice(start, css.indexOf("}", start));
}

describe("design-system foundations", () => {
  test("prefers Infer and bundles its Geist fallbacks without a runtime font request", async () => {
    expect(css).not.toContain("@import url(");
    expect(css).toContain('font-family: "Geist";');
    expect(css).toContain('font-family: "Geist Mono";');
    expect(css).toContain('--font-sans: "Infer", "Geist"');
    expect(css).toContain('--font-display: var(--font-sans)');
    expect(css).toContain('--font-mono: "Geist Mono"');

    for (const path of [
      "src/assets/fonts/geist-variable.woff2",
      "src/assets/fonts/geist-mono-variable.woff2",
    ]) {
      const font = Bun.file(new URL(path, root));
      expect(await font.exists()).toBe(true);
      expect(font.size).toBeGreaterThan(10_000);
    }

    for (const path of ["src/assets/fonts/GEIST-LICENSE.txt"]) {
      const license = await Bun.file(new URL(path, root)).text();
      expect(license).toContain("SIL Open Font License");
      expect(license).not.toContain("404: Not Found");
    }
  });

  test("defines the canonical type, spacing, shape, icon, and motion scales", () => {
    for (const declaration of [
      "--text-xs: 11px;",
      "--text-sm: 12px;",
      "--text-md: 14px;",
      "--text-lg: 16px;",
      "--text-xl: 20px;",
      "--text-2xl: 24px;",
      "--space-1: 4px;",
      "--space-2: 8px;",
      "--space-4: 16px;",
      "--radius-md: 8px;",
      "--radius-lg: 12px;",
      "--icon-size-default: 18px;",
      "--duration-fast: 120ms;",
      "--duration-standard: 180ms;",
      "--layer-popover: 50;",
    ]) {
      expect(css).toContain(declaration);
    }
  });

  test("locks the release palette to neutral surfaces and one emerald accent", () => {
    const dark = cssBlock('[data-theme="dark"]');
    for (const declaration of [
      "--bg: #101010;",
      "--surface-card: #202020;",
      "--surface-raised: #262626;",
      "--ink: #fcfcfc;",
      "--muted: #aaaaaa;",
      "--icon: #fcfcfc;",
      "--icon-disabled: #8e8f8d;",
      "--accent: #38c99b;",
      "--btn-primary: #137a5f;",
      "--btn-primary-hover: #168266;",
    ]) {
      expect(dark).toContain(declaration);
    }

    for (const role of [
      "--prompt",
      "--agent",
      "--choose",
      "--memory",
      "--template",
      "--file-inject",
      "--git-status",
      "--shell",
      "--http",
      "--notify",
      "--write-file",
      "--git-host",
      "--custom-agent",
    ]) {
      expect(dark).toMatch(new RegExp(`${role}: #[0-9a-f]{6};`));
    }
  });

  test("uses a dot-only workflow canvas pattern", () => {
    expect(flowEditor).toContain('id="canvas-dots"');
    expect(flowEditor).toContain("variant={BackgroundVariant.Dots}");
    expect(flowEditor).toContain("gap={22}");
    expect(flowEditor).not.toContain("variant={BackgroundVariant.Lines}");
    expect(css).not.toContain("--canvas-guide:");
  });

  test("does not allow fractional font-weight drift", () => {
    expect(css).not.toMatch(/font-weight:\s*(?:550|560|620|650|750)\b/);
    expect(css).not.toContain("var(--fg)");

    const allowedFamilies = new Set([
      '"Geist"',
      '"Geist Mono"',
      "var(--font-sans)",
      "var(--font-display)",
      "var(--font-mono)",
      "inherit",
    ]);
    const families = [...css.matchAll(/^\s*font-family:\s*([^;]+);/gm)].map(
      ([, family]) => family,
    );
    expect(families.length).toBeGreaterThan(2);
    for (const family of families) expect(allowedFamilies.has(family)).toBe(true);
  });

  test("routes shape and stacking decisions through tokens", () => {
    expect(css).not.toMatch(/^\s*z-index:\s*\d+/m);
    expect(css).not.toMatch(
      /^\s*border-radius:\s*(?:[1-9]\d*px|0?\.\d+rem|50%)/m,
    );
  });

  test("routes type and spacing decisions through tokens", () => {
    // A literal here is drift, not a decision: values like 0.72rem (11.5px)
    // and 0.45rem (7.2px) sit between steps, so the UI grows a private scale
    // that no token describes. Unique geometry stays literal but must say so
    // with a `geometry:` comment on the line above.
    const lines = css.split("\n");
    const offenders = (label: string, pattern: RegExp) =>
      lines
        .map((line, index) => ({ line, index }))
        .filter(
          ({ line, index }) =>
            pattern.test(line) && !/geometry:/.test(lines[index - 1] ?? ""),
        )
        .map(({ line, index }) => `${label} src/App.css:${index + 1}: ${line.trim()}`);

    const found = [
      ...offenders("type", /^\s*font-size:\s*[0-9.]+(rem|px|em)/),
      // The `font:` shorthand sets size and family at once, so it slips past a
      // font-size check while doing the same damage. Only `font: inherit`.
      ...offenders("type", /^\s*font:(?!\s*inherit\s*;)/),
      ...offenders(
        "spacing",
        /^\s*(padding|padding-\w+|margin|margin-\w+|gap|row-gap|column-gap):\s*[^;]*(?<![-\w.])[0-9.]+(rem|px)/,
      ),
    ];
    expect(found).toEqual([]);
  });

  test("fills come from the surface role scale, not a hand-picked color", () => {
    // Fills are chosen by what an element sits ON. A raw color here is how a
    // control ends up brighter than its own card: the value looks fine alone
    // and wrong the moment it is stacked.
    const roles = [
      "--surface-panel",
      "--surface-inset",
      "--surface-card",
      "--surface-raised",
      "--surface",
    ];
    for (const role of roles) {
      expect(cssBlock(":root")).toContain(`${role}: `);
    }

    // One definition per role per theme — the sprawl this scale replaced had
    // three near-identical alphas all meaning "raised".
    for (const role of roles) {
      const defined = [...css.matchAll(new RegExp(`^\\s*${role}: `, "gm"))];
      expect([role, defined.length]).toEqual([role, 2]);
    }

    // Retired names must not come back.
    for (const dead of [
      "--surface-soft",
      "--surface-strong",
      "--surface-glass",
      "--surface-faint",
      "--surface-muted",
      "--panel-solid",
      "--panel-2",
    ]) {
      expect([dead, css.includes(dead)]).toEqual([dead, false]);
    }

    // Raw color fills outside the token blocks. A fill that genuinely must not
    // follow the theme (a scannable QR plate, an always-white logo tile) says
    // so with a `theme-exempt:` comment, the same escape hatch as `geometry:`.
    const lines = css.split("\n");
    const rawFills = lines
      .map((line, index) => ({ line, index }))
      .filter(
        ({ line, index }) =>
          /^\s*background(-color)?:\s*(#|rgba?\()/.test(line) &&
          !/theme-exempt:/.test(
            lines.slice(Math.max(0, index - 4), index).join("\n"),
          ),
      )
      .map(({ line, index }) => `src/App.css:${index + 1}: ${line.trim()}`);
    expect(rawFills).toEqual([]);
  });
});

describe("shared component contracts", () => {
  test("keeps sidebar navigation on the shared scale", () => {
    for (const declaration of [
      "--sidebar-item-font-size: var(--text-md);",
      "--sidebar-item-font-weight: var(--font-weight-regular);",
      "--sidebar-item-color: color-mix(in srgb, var(--ink) 60%, var(--muted));",
      "--sidebar-icon-size: var(--icon-size-compact);",
      "--sidebar-section-font-size: var(--text-lg);",
      "--sidebar-section-font-weight: var(--font-weight-regular);",
      "--sidebar-item-min-height: var(--control-height-default);",
      "--sidebar-item-stack-gap: 2px;",
    ]) {
      expect(css).toContain(declaration);
    }

    // Navigation stays quiet: item ink is softened off full --ink.
    expect(css).not.toContain("--sidebar-item-color: var(--ink);");
    expect(css).toContain("--sidebar-section-color: color-mix(");

    for (const selector of [".sidebar-nav-item", ".settings-sidebar-item"]) {
      const block = cssBlock(selector);
      expect(block).toContain("font-size: var(--sidebar-item-font-size)");
      expect(block).toContain("font-weight: var(--sidebar-item-font-weight)");
      expect(block).toContain("gap: var(--sidebar-item-gap)");
    }

    expect(cssBlock(".sidebar-nav")).toContain(
      "gap: var(--sidebar-item-stack-gap)",
    );
    expect(css).toContain(
      ".settings-sidebar-group-items {\n  gap: var(--sidebar-item-stack-gap);\n}",
    );

    for (const selector of [
      ".sidebar-header h2",
      ".settings-sidebar-heading h2",
      ".settings-sidebar-group h3",
    ]) {
      const block = cssBlock(selector);
      expect(block).toContain("font-size: var(--sidebar-section-font-size)");
      expect(block).toContain("font-weight: var(--sidebar-section-font-weight)");
    }
  });

  test("quiet hover, pressed, and selected states share one neutral scale", () => {
    // The scale itself: transparent (tints the panel beneath rather than
    // painting an opaque tile) and mixed against --ink so it inverts by theme.
    for (const token of [
      "--surface-hover",
      "--surface-selected",
      "--surface-pressed",
    ]) {
      expect(css).toContain(`${token}: color-mix(in srgb, var(--ink)`);
      expect(cssBlock(":root")).toContain(`${token}: color-mix`);
    }

    // Every borderless row, nav item, ghost icon button, and list row consumes
    // the scale. A feature-local wash here is the drift this test exists to
    // catch: it looks fine on one panel and invisible or heavy on the next.
    for (const selector of [
      ".sidebar-nav-item:hover",
      ".sidebar-nav-item.is-active",
      ".settings-sidebar-item:hover:not(:disabled)",
      ".settings-sidebar-item.is-active",
      ".settings-sidebar-back:hover:not(:disabled)",
      ".workflow-folder-options:hover:not(:disabled)",
      ".integration-provider-row:hover",
      ".ui-menu-item:hover",
      ".theme-switch-option:hover:not(:disabled)",
      ".skill-picker-row:hover",
      ".memory-picker-row:hover",
    ]) {
      const block = cssBlock(selector);
      expect(block).toMatch(
        /background: var\(--surface-(hover|selected|pressed)\)/,
      );
      // Hover is never an accent tint; accent marks a real selection.
      expect(block).not.toContain("background: var(--accent");
    }

    // Settings selection is communicated by its neutral fill, without a
    // decorative leading rule competing with the content hierarchy.
    expect(cssBlock(".settings-sidebar-item.is-active")).not.toContain(
      "box-shadow",
    );
  });

  test("menus, controls, forms, and modal shells consume semantic tokens", () => {
    expect(cssBlock("button")).toContain("border-radius: var(--radius-md)");
    expect(cssBlock(".ui-menu")).toContain("border-radius: var(--radius-lg)");
    expect(cssBlock(".ui-menu-item")).toContain("font-size: var(--text-md)");
    expect(cssBlock(".settings-card")).toContain("border-radius: var(--radius-lg)");
    expect(css).toContain("border-radius: var(--radius-xl);");
    expect(css).toContain("box-shadow: var(--elevation-modal);");
    expect(css).toContain('@media (prefers-reduced-motion: reduce)');
    expect(css).toContain('@media (prefers-reduced-transparency: reduce)');
    expect(css).toContain("backdrop-filter: blur(32px)");
  });

  test("routes styled select fields through the shared native control", () => {
    expect(selectControl).toContain("SelectHTMLAttributes<HTMLSelectElement>");
    expect(selectControl).toContain("<select");
    expect(selectControl).toContain('className="ui-select-chevron"');
    expect(cssBlock(".ui-select-input")).toContain(
      "height: var(--control-height-comfortable)",
    );
    expect(cssBlock(".ui-select.is-compact .ui-select-input")).toContain(
      "height: var(--control-height-default)",
    );

    for (const feature of [quickAccessPopover, settingsPage]) {
      expect(feature).toContain("<SelectControl");
      expect(feature).not.toContain("<select");
    }
  });
});

describe("design-system governance", () => {
  test("keeps product specs and coding-agent rules pointed at the source of truth", () => {
    expect(designSystem).toContain("## Interaction state contract");
    expect(designSystem).toContain("## Accessibility");
    expect(designSystem).toContain("Infer");
    expect(designSystem).toContain("Geist Mono");
    expect(designSystem).toContain("### Select controls");
    expect(designSystem).toContain("shared `SelectControl`");
    expect(specs).toContain("type scale is 11/12/14/16/20/24px");
    expect(claudeRule).toContain("## Prohibited drift");
    expect(claudeRule).toContain("shared `SelectControl`");
    expect(cursorRule).toContain("## Prohibited drift");
    expect(cursorRule).toContain("shared `SelectControl`");
  });
});
