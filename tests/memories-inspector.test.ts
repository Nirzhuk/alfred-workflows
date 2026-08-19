import { describe, expect, test } from "bun:test";
import {
  filterAndSortMemories,
  memorySearchSnippet,
} from "../src/features/workflow/components/memories-inspector/memory-list-model";
import type { OutputMemory } from "../src/features/workflow/types";

function memory(
  id: string,
  overrides: Partial<OutputMemory> = {},
): OutputMemory {
  return {
    id,
    workflowId: "workflow-1",
    kind: "note",
    source: "manual",
    title: `Memory ${id}`,
    body: `Content for ${id}`,
    pinned: false,
    createdAt: "2026-08-01T10:00:00.000Z",
    updatedAt: "2026-08-01T10:00:00.000Z",
    origin: "owned",
    ...overrides,
  };
}

describe("Memories inspector list model", () => {
  test("sorts all results by recent activity", () => {
    const results = filterAndSortMemories(
      [
        memory("older"),
        memory("newer", { updatedAt: "2026-08-10T10:00:00.000Z" }),
      ],
      "",
      "all",
      "all",
    );

    expect(results.map((item) => item.id)).toEqual(["newer", "older"]);
  });

  test("combines next-run and kind filters while excluding linked pins", () => {
    const results = filterAndSortMemories(
      [
        memory("note", { pinned: true }),
        memory("output", { pinned: true, kind: "text" }),
        memory("linked", {
          pinned: true,
          origin: "linked",
          kind: "note",
        }),
      ],
      "",
      "pinned",
      "note",
    );

    expect(results.map((item) => item.id)).toEqual(["note"]);
  });

  test("searches provenance and returns a matching snippet", () => {
    const linked = memory("linked", {
      origin: "linked",
      sourceWorkflowName: "Research workflow",
      body: "A reusable finding",
    });

    expect(
      filterAndSortMemories([linked], "research", "all", "all"),
    ).toHaveLength(1);
    expect(memorySearchSnippet(linked, "research")).toBe(
      "From Research workflow",
    );
  });

  test("centers content snippets around the matching text", () => {
    const item = memory("long", {
      body: `${"Earlier context ".repeat(8)}important phrase${" later".repeat(
        12,
      )}`,
    });

    const snippet = memorySearchSnippet(item, "important", 90);
    expect(snippet).toContain("important phrase");
    expect(snippet.startsWith("…")).toBe(true);
    expect(snippet.endsWith("…")).toBe(true);
  });
});
