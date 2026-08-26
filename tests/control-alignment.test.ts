import { describe, expect, test } from "bun:test";

const root = new URL("../", import.meta.url);
const css = await Bun.file(new URL("src/App.css", root)).text();
const titlebar = await Bun.file(
  new URL(
    "src/features/workflow/components/app-title-bar/app-title-bar.tsx",
    root,
  ),
).text();

describe("compact control alignment", () => {
  test("keeps memories copy spacing off the nested switch knob", () => {
    expect(css).toContain(".memory-recall-control > div > p,");
    expect(css).toContain(".memory-recall-control > div > span {");
    expect(css).not.toContain(
      ".memory-recall-control p,\n.memory-recall-control span {",
    );
  });

  test("centers a geometry-based icon in workflow close buttons", () => {
    expect(css).toMatch(
      /\.workflow-tab-close\s*\{[^}]*display:\s*inline-grid;[^}]*place-items:\s*center;/s,
    );
    expect(titlebar).toContain('<Icon name="x" size={14} />');
    expect(titlebar).not.toMatch(/className="ghost workflow-tab-close"[\s\S]*?>\s*×/);
  });
});
