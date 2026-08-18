import type { HistorySearchHit } from "../../types";

export type HistoryMode = "browse" | "search";
export type HistoryScope = "current" | "all";

export function historyMode(query: string): HistoryMode {
  return query.trim() ? "search" : "browse";
}

export function historyWorkflowId(
  scope: HistoryScope,
  activeWorkflowId: string | null,
): string | null {
  return scope === "current" ? activeWorkflowId : null;
}

export function historyHitLabel(kind: HistorySearchHit["kind"]): string {
  return kind === "run_step" ? "Run step" : "Memory";
}

export function isCurrentHistoryGeneration(
  requestGeneration: number,
  currentGeneration: number,
): boolean {
  return requestGeneration === currentGeneration;
}

export function literalHistorySnippet(snippet: string): string {
  return snippet;
}

export function formatHistoryJson(value: unknown): string {
  try {
    return JSON.stringify(value, null, 2) ?? "null";
  } catch {
    return "Unable to display persisted value.";
  }
}

export function formatHistoryWhen(value: string): string {
  const date = new Date(value);
  if (Number.isNaN(date.getTime())) return value;
  return date.toLocaleString(undefined, {
    dateStyle: "medium",
    timeStyle: "short",
  });
}
