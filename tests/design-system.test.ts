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

function cssBlock(selector: string): string {
  const marker = `${selector} {`;
  const lineStart = css.indexOf(`\n${marker}`);
  const start = lineStart >= 0 ? lineStart + 1 : css.startsWith(marker) ? 0 : -1;
  expect(start).toBeGreaterThan(-1);
  return css.slice(start, css.indexOf("}", start));
}

describe("design-system foundations", () => {
  test("bundles Geist, Fraunces, and Geist Mono without a runtime font request", async () => {
    expect(css).not.toContain("@import url(");
    expect(css).toContain('font-family: "Geist";');
    expect(css).toContain('font-family: "Fraunces";');
    expect(css).toContain('font-family: "Geist Mono";');
    expect(css).toContain('--font-sans: "Geist"');
    expect(css).toContain('--font-display: "Fraunces"');
    expect(css).toContain('--font-mono: "Geist Mono"');

    for (const path of [
      "src/assets/fonts/geist-variable.woff2",
      "src/assets/fonts/fraunces-variable.woff2",
      "src/assets/fonts/geist-mono-variable.woff2",
    ]) {
      const font = Bun.file(new URL(path, root));
      expect(await font.exists()).toBe(true);
      expect(font.size).toBeGreaterThan(10_000);
    }

    for (const path of [
      "src/assets/fonts/GEIST-LICENSE.txt",
      "src/assets/fonts/FRAUNCES-LICENSE.txt",
    ]) {
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

  test("does not allow fractional font-weight drift", () => {
    expect(css).not.toMatch(/font-weight:\s*(?:550|560|620|650|750)\b/);

    const allowedFamilies = new Set([
      '"Geist"',
      '"Fraunces"',
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
});

describe("shared component contracts", () => {
  test("keeps sidebar navigation on the shared scale", () => {
    for (const declaration of [
      "--sidebar-item-font-size: var(--text-md);",
      "--sidebar-item-font-weight: var(--font-weight-regular);",
      "--sidebar-item-color: var(--ink);",
      "--sidebar-icon-size: var(--icon-size-compact);",
      "--sidebar-section-font-size: var(--text-lg);",
      "--sidebar-section-font-weight: var(--font-weight-semibold);",
      "--sidebar-item-min-height: var(--control-height-default);",
      "--sidebar-item-stack-gap: 2px;",
    ]) {
      expect(css).toContain(declaration);
    }

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

  test("menus, controls, forms, and modal shells consume semantic tokens", () => {
    expect(cssBlock("button")).toContain("border-radius: var(--radius-md)");
    expect(cssBlock(".ui-menu")).toContain("border-radius: var(--radius-lg)");
    expect(cssBlock(".ui-menu-item")).toContain("font-size: var(--text-md)");
    expect(cssBlock(".settings-card")).toContain("border-radius: var(--radius-lg)");
    expect(css).toContain("border-radius: var(--radius-xl);");
    expect(css).toContain("box-shadow: var(--elevation-modal);");
    expect(css).toContain('@media (prefers-reduced-motion: reduce)');
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
    expect(designSystem).toContain("Fraunces");
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
