import type { CSSProperties } from "react";
import claudeLogo from "../../assets/agents/claude.svg?no-inline";
import cursorLogo from "../../assets/agents/cursor.svg?no-inline";
import codexLogo from "../../assets/agents/codex.svg?no-inline";
import opencodeLogo from "../../assets/agents/opencode.svg?no-inline";
import githubCopilotLogo from "../../assets/agents/github-copilot.svg?no-inline";
import geminiLogo from "../../assets/agents/gemini.svg?no-inline";
import grokLogo from "../../assets/agents/grok.svg?no-inline";
import piLogo from "../../assets/agents/pi.svg?no-inline";
import ompLogo from "../../assets/agents/omp.svg?no-inline";

const LOGOS = {
  claude_code: claudeLogo,
  cursor: cursorLogo,
  codex: codexLogo,
  opencode: opencodeLogo,
  github_copilot: githubCopilotLogo,
  gemini: geminiLogo,
  grok: grokLogo,
  pi: piLogo,
  omp: ompLogo,
} as const;

export type AgentMarkProvider = keyof typeof LOGOS;

const LABELS: Record<AgentMarkProvider, string> = {
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

type Props = {
  provider: string;
  label?: string;
  size?: number;
  running?: boolean;
};

export function isAgentMarkProvider(
  provider: string,
): provider is AgentMarkProvider {
  return Object.prototype.hasOwnProperty.call(LOGOS, provider);
}

/** Offline-safe agent mark for account, workflow, and activity surfaces. */
export function AgentMark({
  provider,
  label,
  size = 12,
  running = false,
}: Props) {
  const knownProvider = isAgentMarkProvider(provider);
  const resolvedLabel =
    label ?? (knownProvider ? LABELS[provider] : provider.trim() || "Agent");
  const accessibleLabel = running
    ? `${resolvedLabel}, working`
    : resolvedLabel;
  const src = knownProvider ? LOGOS[provider] : null;
  const glyphStyle: CSSProperties | undefined = src
    ? {
        WebkitMaskImage: `url("${src}")`,
        maskImage: `url("${src}")`,
      }
    : undefined;

  return (
    <span
      className={`agent-mark ${
        knownProvider ? `agent-mark-${provider}` : "agent-mark-unknown"
      }${running ? " is-running" : ""}`}
      title={accessibleLabel}
      aria-label={accessibleLabel}
      style={{ width: size, height: size }}
    >
      {src ? (
        <span className="agent-mark-glyph" style={glyphStyle} aria-hidden />
      ) : (
        <span className="agent-mark-fallback" aria-hidden>
          {resolvedLabel.charAt(0).toLocaleUpperCase() || "?"}
        </span>
      )}
    </span>
  );
}

export function agentLabel(provider: string): string {
  return isAgentMarkProvider(provider) ? LABELS[provider] : provider;
}
