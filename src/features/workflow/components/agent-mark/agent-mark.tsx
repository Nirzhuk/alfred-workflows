import type { CSSProperties } from "react";
import type { AgentProviderId } from "../../types";
import claudeLogo from "../../../../assets/agents/claude.svg";
import cursorLogo from "../../../../assets/agents/cursor.svg";
import codexLogo from "../../../../assets/agents/codex.svg";
import opencodeLogo from "../../../../assets/agents/opencode.svg";
import githubCopilotLogo from "../../../../assets/agents/github-copilot.svg";
import geminiLogo from "../../../../assets/agents/gemini.svg";
import grokLogo from "../../../../assets/agents/grok.svg";
import piLogo from "../../../../assets/agents/pi.svg";
import ompLogo from "../../../../assets/agents/omp.svg";

const LABELS: Record<AgentProviderId, string> = {
  claude_code: "Claude Code",
  cursor: "Cursor",
  codex: "Codex",
  opencode: "OpenCode",
  github_copilot: "GitHub Copilot",
  gemini: "Gemini",
  grok: "Grok",
  pi: "Pi",
  omp: "OMP",
};

const LOGOS: Record<AgentProviderId, string> = {
  claude_code: claudeLogo,
  cursor: cursorLogo,
  codex: codexLogo,
  opencode: opencodeLogo,
  github_copilot: githubCopilotLogo,
  gemini: geminiLogo,
  grok: grokLogo,
  pi: piLogo,
  omp: ompLogo,
};

type Props = {
  provider: AgentProviderId;
  size?: number;
  running?: boolean;
};

/** Compact brand mark for sidebar / list chips (default 12px). */
export function AgentMark({ provider, size = 12, running = false }: Props) {
  const label = LABELS[provider] ?? provider;
  const accessibleLabel = running ? `${label}, working` : label;
  const src = LOGOS[provider];
  const glyphStyle: CSSProperties = {
    WebkitMaskImage: `url("${src}")`,
    maskImage: `url("${src}")`,
  };
  return (
    <span
      className={`agent-mark agent-mark-${provider}${
        running ? " is-running" : ""
      }`}
      title={accessibleLabel}
      aria-label={accessibleLabel}
      style={{ width: size, height: size }}
    >
      <span className="agent-mark-glyph" style={glyphStyle} aria-hidden />
    </span>
  );
}

export function agentLabel(provider: AgentProviderId): string {
  return LABELS[provider] ?? provider;
}
