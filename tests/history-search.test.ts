import { describe, expect, test } from "bun:test";
import {
  formatHistoryJson,
  historyHitLabel,
  historyNavigation,
  historyMode,
  historyWorkflowId,
  isCurrentHistoryGeneration,
  literalHistorySnippet,
} from "../src/features/workflow/components/history-page/history-format";

const historyPage = await Bun.file(
  new URL(
    "../src/features/workflow/components/history-page/history-page.tsx",
    import.meta.url,
  ),
).text();

describe("history query state", () => {
  test("blank queries browse runs and non-blank queries search", () => {
    expect(historyMode("  \n ")).toBe("browse");
    expect(historyMode("release decision")).toBe("search");
  });

  test("maps current and all-workflow scopes to request filters", () => {
    expect(historyWorkflowId("current", "workflow-1")).toBe("workflow-1");
    expect(historyWorkflowId("all", "workflow-1")).toBeNull();
    expect(historyWorkflowId("current", null)).toBeNull();
  });

  test("labels result kinds and rejects stale generations", () => {
    expect(historyHitLabel("run_step")).toBe("Run step");
    expect(historyHitLabel("memory")).toBe("Memory");
    expect(isCurrentHistoryGeneration(4, 4)).toBe(true);
    expect(isCurrentHistoryGeneration(3, 4)).toBe(false);
  });

  test("carries an exact run into History and clears stale external selection", () => {
    expect(historyNavigation({ type: "open-run", runId: "run-42" })).toEqual({
      view: "history",
      runId: "run-42",
    });
    expect(historyNavigation({ type: "open-history" })).toEqual({
      view: "history",
      runId: null,
    });
    expect(historyNavigation({ type: "close-history" })).toEqual({
      view: "canvas",
      runId: null,
    });
  });
});

describe("history text rendering", () => {
  test("preserves snippets as literal text", () => {
    const hostile = '<img src=x onerror="alert(1)"> [decision]';
    expect(literalHistorySnippet(hostile)).toBe(hostile);
  });

  test("formats persisted JSON without HTML conversion", () => {
    expect(formatHistoryJson({ answer: "<script>no</script>" })).toBe(
      '{\n  "answer": "<script>no</script>"\n}',
    );
  });

  test("keeps snippets and exact step payloads on text-only render paths", () => {
    expect(historyPage).toContain("literalHistorySnippet(hit.snippet)");
    expect(historyPage).toContain("<details>");
    expect(historyPage).toContain("<pre>");
    expect(historyPage).not.toContain("dangerouslySetInnerHTML");
  });
});
