import { describe, expect, test } from "bun:test";
import { renderToStaticMarkup } from "react-dom/server";
import { SidebarFolderContext } from "../src/features/workflow/components/sidebar-folder-context";

// Tokens live in src/styles/tokens.css and App.css imports them; the duration
// this component mirrors is a token, so both files are read here.
const css = (
  await Promise.all(
    ["../src/styles/tokens.css", "../src/App.css"].map((path) =>
      Bun.file(new URL(path, import.meta.url)).text(),
    ),
  )
).join("\n");
const component = await Bun.file(
  new URL(
    "../src/features/workflow/components/sidebar-folder-context/sidebar-folder-context.tsx",
    import.meta.url,
  ),
).text();

describe("SidebarFolderContext", () => {
  test("renders nothing when the list is scrolled above every folder", () => {
    expect(renderToStaticMarkup(<SidebarFolderContext folder={null} />)).toBe(
      "",
    );
  });

  test("announces the folder it names and enters without a leaving twin", () => {
    const markup = renderToStaticMarkup(
      <SidebarFolderContext folder={{ name: "Potato", count: 2 }} />,
    );
    expect(markup).toContain('aria-live="polite"');
    expect(markup).toContain("Potato");
    expect(markup).toContain(
      '<span class="sidebar-header-context-count">2</span>',
    );
    // The entrance is the label's mount animation, so a first appearance needs
    // no outgoing node.
    expect(markup).not.toContain("is-leaving");
  });

  test("keeps the exit timeout equal to the CSS exit duration", () => {
    // The outgoing label is unmounted on a timer, not on animationend. Too
    // short cuts the exit off; too long strands the old folder in the header.
    expect(component).toContain("const EXIT_MS = 120;");
    expect(css).toContain("--duration-fast: 120ms;");
    expect(css).toContain(
      "animation: sidebar-context-out var(--duration-fast)",
    );
  });
});
