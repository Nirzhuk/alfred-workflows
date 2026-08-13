import type { OutputMemory } from "./types";

const KEY = (workflowId: string) => `agentflow:memories:${workflowId}`;

/** Legacy localStorage shape (pre-SQLite memories). */
type LegacyMemory = {
  id: string;
  workflowId: string;
  runId?: string | null;
  nodeId?: string | null;
  title: string;
  body: string;
  createdAt: string;
};

export function loadLegacyMemories(workflowId: string): LegacyMemory[] {
  try {
    const raw = localStorage.getItem(KEY(workflowId));
    if (!raw) return [];
    const parsed = JSON.parse(raw) as LegacyMemory[];
    return Array.isArray(parsed) ? parsed : [];
  } catch {
    return [];
  }
}

export function clearLegacyMemories(workflowId: string) {
  try {
    localStorage.removeItem(KEY(workflowId));
  } catch {
    /* ignore */
  }
}

export function sortMemories(memories: OutputMemory[]): OutputMemory[] {
  return [...memories].sort((a, b) => {
    const aLinked = a.origin === "linked" ? 1 : 0;
    const bLinked = b.origin === "linked" ? 1 : 0;
    if (aLinked !== bLinked) return aLinked - bLinked;
    if (a.pinned !== b.pinned) return a.pinned ? -1 : 1;
    return (b.updatedAt || b.createdAt).localeCompare(
      a.updatedAt || a.createdAt,
    );
  });
}

export function asOwnedMemory(memory: OutputMemory): OutputMemory {
  return { ...memory, origin: memory.origin ?? "owned" };
}
