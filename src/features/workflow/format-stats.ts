import type { AgentStepStats } from "./types";

function formatTokens(n: number) {
  return n >= 1000 ? `${(n / 1000).toFixed(1)}k` : String(n);
}

/** Compact one-line summary: "4.2s · 1.2k in / 340 out · $0.0042". Omits
 * whatever a given CLI doesn't report. */
export function formatStats(stats: AgentStepStats): string | null {
  const parts: string[] = [];
  if (stats.durationMs != null) {
    parts.push(`${(stats.durationMs / 1000).toFixed(1)}s`);
  }
  if (stats.inputTokens != null || stats.outputTokens != null) {
    parts.push(
      `${formatTokens(stats.inputTokens ?? 0)} in / ${formatTokens(stats.outputTokens ?? 0)} out`,
    );
  }
  if (stats.totalCostUsd != null) {
    parts.push(`$${stats.totalCostUsd.toFixed(4)}`);
  }
  return parts.length > 0 ? parts.join(" · ") : null;
}

/** Same as `formatStats`, prefixed with provider/model — for contexts (like
 * the output dialog) that aren't already showing which agent ran. */
export function formatStatsWithSource(stats: AgentStepStats): string | null {
  const lead = [stats.provider, stats.model].filter(Boolean).join(" · ");
  const rest = formatStats(stats);
  if (lead && rest) return `${lead} · ${rest}`;
  return lead || rest || null;
}
