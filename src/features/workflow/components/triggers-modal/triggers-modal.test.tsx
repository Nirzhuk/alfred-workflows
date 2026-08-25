import { describe, expect, test } from "bun:test";
import { renderToStaticMarkup } from "react-dom/server";
import { LOCKED_CAPABILITY_EXPLANATION } from "../../../licensing/components/locked-capability/locked-capability";
import { TriggersLockedContent } from "./triggers-modal";
import type { Trigger } from "../../types";

function trigger(overrides: Partial<Trigger>): Trigger {
  return {
    id: "trg-1",
    workflowId: "wf-1",
    source: "file",
    label: "Repo saves",
    config: {},
    secret: null,
    enabled: true,
    lastFiredAt: null,
    createdAt: "2026-01-01T00:00:00Z",
    updatedAt: "2026-01-01T00:00:00Z",
    ...overrides,
  };
}

function render(triggers: Trigger[]): string {
  return renderToStaticMarkup(<TriggersLockedContent triggers={triggers} />);
}

describe("trigger locked content", () => {
  test("uses the shared locked-capability treatment and names the perk", () => {
    const markup = render([]);
    expect(markup).toContain(">Triggers</h3>");
    expect(markup).toContain(LOCKED_CAPABILITY_EXPLANATION);
    expect(markup).toContain('data-locked-capability="triggers"');
  });

  test("lists every saved trigger read-only instead of hiding it", () => {
    const markup = render([
      trigger({}),
      trigger({
        id: "trg-2",
        source: "webhook",
        label: "",
        enabled: false,
      }),
    ]);
    expect(markup).toContain("<li>Repo saves — file (enabled)</li>");
    expect(markup).toContain("<li>webhook — webhook (paused)</li>");
  });

  test("says when there is nothing saved yet", () => {
    expect(render([])).toContain("No triggers yet.");
  });
});
