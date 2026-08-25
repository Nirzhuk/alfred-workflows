import { describe, expect, test } from "bun:test";
import type { createMemory } from "../src/features/workflow/api";
import {
  canPinMemory,
  isMemoryPromptEligible,
  sortMemories,
  withMemoryDefaults,
  workspaceScopeAvailable,
} from "../src/features/workflow/memories";

const memory = (
  id: string,
  overrides: Parameters<typeof withMemoryDefaults>[0] = {
    id,
    title: id,
    body: id,
    createdAt: "2026-08-18T10:00:00Z",
    updatedAt: "2026-08-18T10:00:00Z",
  },
) =>
  withMemoryDefaults({
    id,
    title: id,
    body: id,
    createdAt: "2026-08-18T10:00:00Z",
    updatedAt: "2026-08-18T10:00:00Z",
    workflowId: "workflow-1",
    ...overrides,
  });

describe("scoped atomic memory model", () => {
  test("legacy workflow output receives stable defaults", () => {
    const legacy = withMemoryDefaults({
      id: "legacy",
      workflowId: "workflow-1",
      title: "Output",
      body: "Body",
      createdAt: "2026-08-18T10:00:00Z",
      updatedAt: "2026-08-18T10:00:00Z",
    });

    expect(legacy.scopeType).toBe("workflow");
    expect(legacy.scopeKey).toBe("workflow-1");
    expect(legacy.memoryType).toBe("output");
    expect(legacy.status).toBe("active");
    expect(legacy.confidence).toBe(1);
    expect(legacy.salience).toBe(50);

    const legacyCreate = {
      workflowId: "workflow-1",
      title: "Output",
      body: "Body",
    } satisfies Parameters<typeof createMemory>[0];
    expect(legacyCreate.body).toBe("Body");
  });

  test("sort is immutable and prioritizes active pins, specificity, then inactive", () => {
    const source = [
      memory("inactive", {
        id: "inactive",
        title: "inactive",
        body: "inactive",
        createdAt: "2026-08-18T14:00:00Z",
        updatedAt: "2026-08-18T14:00:00Z",
        status: "retracted",
        pinned: true,
      }),
      memory("user", {
        id: "user",
        title: "user",
        body: "user",
        createdAt: "2026-08-18T13:00:00Z",
        updatedAt: "2026-08-18T13:00:00Z",
        scopeType: "user",
        scopeKey: "local-user",
      }),
      memory("workflow"),
      memory("workspace-pin", {
        id: "workspace-pin",
        title: "workspace-pin",
        body: "workspace-pin",
        createdAt: "2026-08-18T09:00:00Z",
        updatedAt: "2026-08-18T09:00:00Z",
        scopeType: "workspace",
        scopeKey: "/projects/alfred",
        pinned: true,
      }),
    ];
    const snapshot = [...source];

    expect(sortMemories(source).map(({ id }) => id)).toEqual([
      "workspace-pin",
      "workflow",
      "user",
      "inactive",
    ]);
    expect(source).toEqual(snapshot);
  });

  test("workspace scope needs a configured directory and inactive records cannot pin", () => {
    expect(workspaceScopeAvailable(undefined)).toBe(false);
    expect(workspaceScopeAvailable("   ")).toBe(false);
    expect(workspaceScopeAvailable("/projects/alfred")).toBe(true);
    expect(canPinMemory(memory("active"))).toBe(true);
    expect(
      canPinMemory(
        memory("inactive", {
          id: "inactive",
          title: "inactive",
          body: "inactive",
          createdAt: "2026-08-18T10:00:00Z",
          updatedAt: "2026-08-18T10:00:00Z",
          status: "superseded",
        }),
      ),
    ).toBe(false);
  });

  test("prompt eligibility requires active, unexpired memory", () => {
    const now = new Date("2026-08-18T12:00:00Z");
    expect(isMemoryPromptEligible(memory("active"), now)).toBe(true);
    expect(
      isMemoryPromptEligible(
        memory("future", {
          id: "future",
          title: "future",
          body: "future",
          createdAt: "2026-08-18T10:00:00Z",
          updatedAt: "2026-08-18T10:00:00Z",
          expiresAt: "2026-08-18T13:00:00Z",
        }),
        now,
      ),
    ).toBe(true);
    expect(
      isMemoryPromptEligible(
        memory("expired", {
          id: "expired",
          title: "expired",
          body: "expired",
          createdAt: "2026-08-18T10:00:00Z",
          updatedAt: "2026-08-18T10:00:00Z",
          expiresAt: "2026-08-18T11:59:59Z",
        }),
        now,
      ),
    ).toBe(false);
    expect(
      isMemoryPromptEligible(
        memory("superseded", {
          id: "superseded",
          title: "superseded",
          body: "superseded",
          createdAt: "2026-08-18T10:00:00Z",
          updatedAt: "2026-08-18T10:00:00Z",
          status: "superseded",
        }),
        now,
      ),
    ).toBe(false);
    expect(
      isMemoryPromptEligible(
        memory("retracted", {
          id: "retracted",
          title: "retracted",
          body: "retracted",
          createdAt: "2026-08-18T10:00:00Z",
          updatedAt: "2026-08-18T10:00:00Z",
          status: "retracted",
        }),
        now,
      ),
    ).toBe(false);
  });
});
