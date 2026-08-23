import { describe, expect, test } from "bun:test";
import { getWorkflowStatusLabel } from "../src/features/workflow/status-label";

describe("workflow status label", () => {
  test("keeps a fast workflow switch on the stable saved label", () => {
    expect(
      getWorkflowStatusLabel({
        activeWorkflowId: "previous-workflow",
        activeRunStatus: null,
        dirty: false,
        loading: false,
        workflowLoading: true,
        showWorkflowLoading: false,
      }),
    ).toBe("All changes saved");
  });

  test("shows loading copy only after the switch has earned it", () => {
    expect(
      getWorkflowStatusLabel({
        activeWorkflowId: "previous-workflow",
        activeRunStatus: null,
        dirty: false,
        loading: false,
        workflowLoading: true,
        showWorkflowLoading: true,
      }),
    ).toBe("Loading workflow…");
  });

  test("keeps ordinary workflow and run states in their existing order", () => {
    expect(
      getWorkflowStatusLabel({
        activeWorkflowId: "workflow",
        activeRunStatus: "running",
        dirty: true,
        loading: true,
        workflowLoading: false,
      }),
    ).toBe("Automation running…");
    expect(
      getWorkflowStatusLabel({
        activeWorkflowId: "workflow",
        activeRunStatus: null,
        dirty: true,
        loading: false,
        workflowLoading: false,
      }),
    ).toBe("Unsaved changes");
    expect(
      getWorkflowStatusLabel({
        activeWorkflowId: null,
        activeRunStatus: null,
        dirty: false,
        loading: false,
        workflowLoading: false,
      }),
    ).toBe("No workflow open");
  });
});
