import { describe, expect, mock, test } from "bun:test";

/**
 * Saved-workflow safety for the pro perks (Plan 008 Step 4): a workflow that
 * already uses schedules or triggers must load completely under a locked
 * licence — nothing dropped, nothing silently disabled, and no mutation
 * issued to persisted data while merely loading.
 */

let mutations: string[] = [];

const WORKFLOW = {
  id: "wf-1",
  name: "Nightly report",
  graph: { nodes: [], edges: [] },
};

const SAVED_SCHEDULE = {
  id: "sched-1",
  workflowId: "wf-1",
  cron: "0 0 7 * * *",
  enabled: true,
  nextRunAt: null,
  createdAt: "2026-01-01T00:00:00Z",
  updatedAt: "2026-01-01T00:00:00Z",
};

const SAVED_TRIGGERS = [
  {
    id: "trg-file",
    workflowId: "wf-1",
    source: "file",
    label: "Repo saves",
    config: { path: "/tmp/repo", pattern: "", debounceMs: 2000 },
    secret: null,
    enabled: true,
    lastFiredAt: null,
    createdAt: "2026-01-01T00:00:00Z",
    updatedAt: "2026-01-01T00:00:00Z",
  },
  {
    id: "trg-hook",
    workflowId: "wf-1",
    source: "webhook",
    label: "Local hook",
    config: {},
    secret: "tok",
    enabled: true,
    lastFiredAt: null,
    createdAt: "2026-01-01T00:00:00Z",
    updatedAt: "2026-01-01T00:00:00Z",
  },
];

function scheduleRow() {
  return { ...SAVED_SCHEDULE, workflowName: WORKFLOW.name };
}

mock.module("./api", () => ({
  listWorkflows: async () => [WORKFLOW],
  listWorkflowFolders: async () => [],
  listSchedules: async () => [scheduleRow()],
  listActiveRuns: async () => [],
  getWorkflow: async () => WORKFLOW,
  getWorkflowSchedule: async () => SAVED_SCHEDULE,
  listWorkflowTriggers: async (workflowId: string) =>
    workflowId === "wf-1" ? SAVED_TRIGGERS : [],
  webhookBaseUrl: async () => "http://127.0.0.1:8787",
  listAppTriggerStatuses: async () => [],
  listMemories: async () => [],
  upsertWorkflowSchedule: async () => {
    mutations.push("upsertWorkflowSchedule");
    return SAVED_SCHEDULE;
  },
  deleteWorkflowSchedule: async () => {
    mutations.push("deleteWorkflowSchedule");
  },
  upsertWorkflowTrigger: async () => {
    mutations.push("upsertWorkflowTrigger");
    return null;
  },
  deleteWorkflowTrigger: async () => {
    mutations.push("deleteWorkflowTrigger");
  },
}));

// Import after the mock so the store binds the fake api.
const { useWorkflowStore } = await import("./store");
const { resolveCapability } = await import("../licensing");

describe("loading a workflow that uses pro capabilities under a locked licence", () => {
  test("the distribution-no-license cell is actually reachable", () => {
    const decision = resolveCapability(
      {
        buildKind: "distribution",
        licenseState: "unlicensed",
        inWindow: false,
      },
      "schedules",
    );
    expect(decision).toEqual({ available: false, reason: "noLicense" });
    expect(
      resolveCapability(
        { buildKind: "distribution", licenseState: "unlicensed", inWindow: false },
        "triggers",
      ).available,
    ).toBe(false);
  });

  test("load keeps every saved schedule and trigger intact, mutating nothing", async () => {
    await useWorkflowStore.getState().loadWorkflows();

    const state = useWorkflowStore.getState();
    expect(state.error).toBeNull();
    expect(state.workflows.map((workflow) => workflow.id)).toEqual(["wf-1"]);

    // The full schedule row survived load, still enabled, untouched.
    expect(state.workflowSchedules).toHaveLength(1);
    expect(state.workflowSchedules[0]).toMatchObject({
      workflowId: "wf-1",
      cron: "0 0 7 * * *",
      enabled: true,
    });

    // selectWorkflow ran inside loadWorkflows, so triggers loaded too.
    expect(state.activeWorkflowId).toBe("wf-1");
    expect(state.triggers.map((trigger) => trigger.id)).toEqual([
      "trg-file",
      "trg-hook",
    ]);
    expect(state.triggers.every((trigger) => trigger.enabled)).toBe(true);

    // Loading only ever read; no persisted schedule or trigger was rewritten,
    // cleared, or toggled behind our back.
    expect(mutations).toEqual([]);
  });

  test("re-loading under an explicitly locked decision changes nothing stored", async () => {
    // The resolver says both capabilities are locked on this hypothetical
    // build; the store has no licence input at all and must behave the same.
    const locked = resolveCapability(
      { buildKind: "distribution", licenseState: "unlicensed", inWindow: false },
      "schedules",
    );
    expect(locked.available).toBe(false);

    await useWorkflowStore.getState().loadSchedule("wf-1");
    await useWorkflowStore.getState().loadTriggers("wf-1");

    const state = useWorkflowStore.getState();
    expect(state.schedule).toMatchObject({ cron: "0 0 7 * * *", enabled: true });
    expect(state.triggers).toHaveLength(2);
    expect(mutations).toEqual([]);
  });
});
