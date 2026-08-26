import { expect, test } from "bun:test";
import { getSubmenuPlacement } from "./menu-sub";

function visibleBounds(input: {
  contentTop: number;
  contentHeight: number;
  viewportHeight: number;
}) {
  const placement = getSubmenuPlacement({
    triggerRight: 400,
    contentWidth: 280,
    contentTop: input.contentTop,
    contentHeight: input.contentHeight,
    viewportWidth: 900,
    viewportHeight: input.viewportHeight,
  });

  return {
    ...placement,
    top: input.contentTop + placement.verticalShift,
    bottom:
      input.contentTop +
      placement.verticalShift +
      Math.min(input.contentHeight, placement.maxHeight),
  };
}

test("constrains a submenu taller than the viewport and keeps it reachable", () => {
  const bounds = visibleBounds({
    contentTop: 304,
    contentHeight: 906,
    viewportHeight: 824,
  });

  expect(bounds.maxHeight).toBe(808);
  expect(bounds.top).toBe(8);
  expect(bounds.bottom).toBe(816);
});

test("shifts a submenu above the bottom viewport edge", () => {
  const bounds = visibleBounds({
    contentTop: 600,
    contentHeight: 300,
    viewportHeight: 824,
  });

  expect(bounds.top).toBe(516);
  expect(bounds.bottom).toBe(816);
});

test("preserves trigger alignment when the submenu already fits", () => {
  const bounds = visibleBounds({
    contentTop: 252,
    contentHeight: 906,
    viewportHeight: 1200,
  });

  expect(bounds.maxHeight).toBe(1184);
  expect(bounds.top).toBe(252);
  expect(bounds.bottom).toBe(1158);
});
