import { afterEach, describe, expect, spyOn, test } from "bun:test";
import * as api from "../src/features/workflow/api";
import {
  memoryUseReasonLabel,
  openHistoryMemory,
} from "../src/features/workflow/components/history-page/history-format";
import { useWorkflowStore } from "../src/features/workflow/store";
import type { Workflow } from "../src/features/workflow/types";

const inspectorSource = await Bun.file(
  new URL(
    "../src/features/workflow/components/memories-inspector/memories-inspector.tsx",
    import.meta.url,
  ),
).text();
const historySource = await Bun.file(
  new URL(
    "../src/features/workflow/components/history-page/history-page.tsx",
    import.meta.url,
  ),
).text();
const workflowRust = await Bun.file(
  new URL("../src-tauri/src/db/workflows.rs", import.meta.url),
).text();
const migrationRust = await Bun.file(
  new URL("../src-tauri/src/db/migrate.rs", import.meta.url),
).text();

function workflow(enabled: boolean): Workflow {
  return {
    id: "workflow-1",
    name: "Recall",
    description: "",
    workingDirectory: "/projects/alfred",
    memoryRetrievalEnabled: enabled,
    graph: { nodes: [], edges: [] },
    createdAt: "2026-08-18T10:00:00Z",
    updatedAt: "2026-08-18T10:00:00Z",
  };
}

afterEach(() => {
  useWorkflowStore.setState({ workflows: [], error: null });
});

describe("automatic recall rollout and control", () => {
  test("keeps migrated workflows off and explicitly creates new workflows on", () => {
    expect(migrationRust).toContain('"INTEGER NOT NULL DEFAULT 0"');
    expect(workflowRust).toContain(
      "sort_order, memory_retrieval_enabled, graph_json",
    );
    expect(workflowRust).toContain("VALUES (?1, ?2, ?3, ?4, ?5, ?6, 1, ?7");
  });

  test("saves the workflow toggle through the API", async () => {
    const original = workflow(false);
    const updated = { ...original, memoryRetrievalEnabled: true };
    const update = spyOn(api, "updateWorkflow").mockResolvedValue(updated);
    useWorkflowStore.setState({ workflows: [original], error: null });

    await useWorkflowStore
      .getState()
      .setMemoryRetrievalEnabled(original.id, true);

    expect(update).toHaveBeenCalledWith({
      id: original.id,
      memoryRetrievalEnabled: true,
    });
    expect(useWorkflowStore.getState().workflows[0].memoryRetrievalEnabled).toBe(
      true,
    );
    update.mockRestore();
  });

  test("rolls back an optimistic toggle and exposes the store error", async () => {
    const original = workflow(false);
    const update = spyOn(api, "updateWorkflow").mockRejectedValue(
      new Error("recall_update_failed"),
    );
    useWorkflowStore.setState({ workflows: [original], error: null });

    await useWorkflowStore
      .getState()
      .setMemoryRetrievalEnabled(original.id, true);

    expect(useWorkflowStore.getState().workflows[0].memoryRetrievalEnabled).toBe(
      false,
    );
    expect(useWorkflowStore.getState().error).toContain("recall_update_failed");
    update.mockRestore();
  });

  test("states the local exact-search budget without semantic claims", () => {
    expect(inspectorSource).toContain("Automatic recall");
    expect(inspectorSource).toContain("local exact FTS5 search + recency");
    expect(inspectorSource).toContain("not an embedding service");
    expect(inspectorSource).toContain("8 items / 6,000 bytes");
    expect(inspectorSource).not.toContain("AI memory");
    expect(inspectorSource).not.toContain("semantic search");
  });
});

describe("memory-use explanations", () => {
  test("maps persisted reasons without treating score as confidence", () => {
    expect(memoryUseReasonLabel("pinned")).toBe("Pinned core");
    expect(memoryUseReasonLabel("lexical")).toBe(
      "Matched this step's prompt",
    );
    expect(memoryUseReasonLabel("recent")).toBe("Recent fallback");
    expect(historySource).toContain("score {memoryUse.score.toFixed(2)}");
    expect(historySource).not.toContain("score confidence");
  });

  test("links a trace row to the Memories inspector event", () => {
    let received: { memoryId?: string } | undefined;
    const previousWindow = globalThis.window;
    const previousCustomEvent = globalThis.CustomEvent;
    class TestCustomEvent<T> {
      type: string;
      detail: T;
      constructor(type: string, init: { detail: T }) {
        this.type = type;
        this.detail = init.detail;
      }
    }
    Object.defineProperty(globalThis, "CustomEvent", {
      configurable: true,
      value: TestCustomEvent,
    });
    Object.defineProperty(globalThis, "window", {
      configurable: true,
      value: {
        dispatchEvent(event: TestCustomEvent<{ memoryId?: string }>) {
          if (event.type === "alfred:open-memories") received = event.detail;
          return true;
        },
      },
    });

    openHistoryMemory("memory-42");

    expect(received).toEqual({ memoryId: "memory-42" });
    Object.defineProperty(globalThis, "window", {
      configurable: true,
      value: previousWindow,
    });
    Object.defineProperty(globalThis, "CustomEvent", {
      configurable: true,
      value: previousCustomEvent,
    });
  });

  test("renders an explicit empty trace state and never renders a query", () => {
    expect(historySource).toContain(
      "No pinned or recalled memory was recorded for this run.",
    );
    expect(historySource).toContain("detail.memoryUses ?? []");
    expect(historySource).not.toContain("searchQuery");
    expect(historySource).not.toContain("queryText");
  });
});
