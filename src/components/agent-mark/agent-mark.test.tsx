import { expect, test } from "bun:test";
import { renderToStaticMarkup } from "react-dom/server";
import {
  AgentMark,
  type AgentMarkProvider,
  isAgentMarkProvider,
} from "./agent-mark";

const PROVIDERS: AgentMarkProvider[] = [
  "claude_code",
  "cursor",
  "codex",
  "opencode",
  "github_copilot",
  "gemini",
  "grok",
  "pi",
  "omp",
];

test("renders a local logo for every agent provider", () => {
  for (const provider of PROVIDERS) {
    const markup = renderToStaticMarkup(
      <AgentMark provider={provider} size={20} />,
    );

    expect(isAgentMarkProvider(provider)).toBe(true);
    expect(markup).toContain(`agent-mark-${provider}`);
    expect(markup).toContain("agent-mark-glyph");
    expect(markup).not.toContain("agent-mark-fallback");
  }
});

test("keeps a deterministic initial for an unknown future provider", () => {
  const markup = renderToStaticMarkup(
    <AgentMark provider="future_agent" label="Future Agent" size={20} />,
  );

  expect(isAgentMarkProvider("future_agent")).toBe(false);
  expect(markup).toContain("agent-mark-unknown");
  expect(markup).toContain("agent-mark-fallback");
  expect(markup).toContain(">F<");
});
