import { extractHtmlReport } from "../../html-report";
import type { MemoryKind, OutputMemory } from "../../types";

export type MemoryQuickFilter = "all" | "pinned" | "linked";
export type MemoryKindFilter = "all" | MemoryKind;

function previewText(text: string, max: number) {
  if (extractHtmlReport(text)) return "HTML report. Open to preview";
  const flat = text.replace(/\s+/g, " ").trim();
  if (flat.length <= max) return flat;
  return `${flat.slice(0, max)}…`;
}

export function memorySearchSnippet(
  memory: OutputMemory,
  query: string,
  max = 130,
) {
  const q = query.trim().toLowerCase();
  if (!q) return previewText(memory.body, max);
  if (extractHtmlReport(memory.body)) return "HTML report. Open to preview";

  const flat = memory.body.replace(/\s+/g, " ").trim();
  const bodyIndex = flat.toLowerCase().indexOf(q);
  if (bodyIndex >= 0) {
    const start = Math.max(0, bodyIndex - 36);
    const end = Math.min(flat.length, start + max);
    return `${start > 0 ? "…" : ""}${flat.slice(start, end)}${
      end < flat.length ? "…" : ""
    }`;
  }

  const provenance = memory.sourceWorkflowName
    ? `From ${memory.sourceWorkflowName}`
    : `Source: ${memory.source}`;
  if (provenance.toLowerCase().includes(q)) return provenance;
  return previewText(memory.body, max);
}

export function filterAndSortMemories(
  memories: OutputMemory[],
  query: string,
  quickFilter: MemoryQuickFilter,
  kindFilter: MemoryKindFilter,
) {
  const q = query.trim().toLowerCase();
  return memories
    .filter((memory) => {
      if (
        quickFilter === "pinned" &&
        (!memory.pinned || memory.origin === "linked")
      ) {
        return false;
      }
      if (quickFilter === "linked" && memory.origin !== "linked") return false;
      if (kindFilter !== "all" && memory.kind !== kindFilter) return false;
      if (!q) return true;
      return (
        memory.title.toLowerCase().includes(q) ||
        memory.body.toLowerCase().includes(q) ||
        memory.source.toLowerCase().includes(q) ||
        (memory.sourceWorkflowName ?? "").toLowerCase().includes(q)
      );
    })
    .sort(
      (a, b) =>
        new Date(b.updatedAt || b.createdAt).getTime() -
        new Date(a.updatedAt || a.createdAt).getTime(),
    );
}
