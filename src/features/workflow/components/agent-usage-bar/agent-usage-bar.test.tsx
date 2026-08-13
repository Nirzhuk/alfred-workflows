import { expect, test } from "bun:test";
import { renderToStaticMarkup } from "react-dom/server";
import { AgentUsageBar, primaryUsageWindow } from "./agent-usage-bar";
import type { AgentUsageSnapshot } from "../../types";

test("shows only the 5-hour window in the footer", () => {
  const usage: AgentUsageSnapshot[] = [
    {
      provider: "claude_code",
      connected: true,
      source: "claude /usage",
      updatedAt: "2026-08-11T15:00:00Z",
      windows: [
        { label: "5-hour", usedPercent: 3, resetDescription: "resets in 4h" },
        { label: "7-day", usedPercent: 17, resetDescription: "resets in 6d" },
      ],
    },
  ];

  const markup = renderToStaticMarkup(
    <AgentUsageBar
      workflowKey="workflow-a"
      providers={["claude_code"]}
      usage={usage}
      refreshing={false}
      onRefresh={() => {}}
    />,
  );

  expect(markup).toContain("5-hour");
  expect(markup).toContain("97% left");
  expect(markup).toContain(
    'aria-label="Claude Code 5-hour subscription remaining"',
  );
  expect(markup).toContain('aria-valuenow="97"');
  expect(markup).not.toContain("7-day");
  expect(markup).not.toContain("83% left");
  expect(markup).toContain('aria-haspopup="dialog"');
  expect(markup).toContain('aria-expanded="false"');
  expect(markup).toContain('data-workflow-key="workflow-a"');
});

test("selects the 5-hour window even when it is not first", () => {
  const windows = [
    { label: "7-day", usedPercent: 17 },
    { label: "5-hour", usedPercent: 3 },
  ];

  expect(primaryUsageWindow(windows)).toEqual(windows[1]);
});
