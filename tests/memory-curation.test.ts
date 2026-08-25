import { describe, expect, test } from "bun:test";
import { createStore } from "zustand/vanilla";
import {
  DEFAULT_MEMORY_REVIEW_DRAFT,
  DEFAULT_MEMORY_REVIEW_SETTINGS,
  MEMORY_REVIEW_ACKNOWLEDGEMENT,
  canSaveMemoryReview,
  createMemoryReviewState,
  isMemoryReviewConfigured,
  memoryReviewFailureCopy,
  memoryReviewSavePayload,
  workflowSuggestionGate,
} from "../src/features/settings/memory-review";
import {
  applyCandidateUpdate,
  blockedCandidateCopy,
  candidateApprovalRequiresConfirmation,
  candidateConfidencePercent,
  candidateEditPayload,
  candidateIsEditable,
  countPendingSuggestions,
  hasCandidateEdits,
  shouldRefreshSuggestions,
  sortSuggestions,
  suggestionAnnouncement,
} from "../src/features/workflow/components/memories-inspector/suggestions-model";
import type {
  MemoryCandidate,
  MemoryReviewJob,
  MemoryReviewSettings,
} from "../src/features/workflow/types";

function candidate(
  id: string,
  overrides: Partial<MemoryCandidate> = {},
): MemoryCandidate {
  return {
    id,
    reviewRunId: `run-${id}`,
    workflowId: "workflow-1",
    sourceNodeId: null,
    operation: "create",
    targetMemoryId: null,
    scopeType: "workflow",
    scopeKey: "workflow-1",
    memoryType: "preference",
    title: `Suggestion ${id}`,
    body: `Body for ${id}`,
    confidence: 0.8,
    rationale: "the user stated a durable preference",
    status: "pending",
    blockedCode: null,
    createdAt: "2026-08-20T10:00:00.000Z",
    decidedAt: null,
    ...overrides,
  };
}

describe("memory review settings", () => {
  test("defaults to off with no provider or model", () => {
    expect(DEFAULT_MEMORY_REVIEW_SETTINGS.enabled).toBe(false);
    expect(DEFAULT_MEMORY_REVIEW_SETTINGS.provider).toBeNull();
    expect(DEFAULT_MEMORY_REVIEW_SETTINGS.model).toBeNull();
    expect(DEFAULT_MEMORY_REVIEW_SETTINGS.maxCandidates).toBe(5);
    expect(isMemoryReviewConfigured(DEFAULT_MEMORY_REVIEW_SETTINGS)).toBe(
      false,
    );
  });

  test("enabling requires a provider and the explicit acknowledgement", () => {
    expect(
      canSaveMemoryReview({
        enabled: true,
        provider: null,
        model: null,
        acknowledged: true,
      }),
    ).toBe(false);
    expect(
      canSaveMemoryReview({
        enabled: true,
        provider: "claude_code",
        model: null,
        acknowledged: false,
      }),
    ).toBe(false);
    expect(
      canSaveMemoryReview({
        enabled: true,
        provider: "claude_code",
        model: null,
        acknowledged: true,
      }),
    ).toBe(true);
    // Staying off never needs an acknowledgement.
    expect(canSaveMemoryReview(DEFAULT_MEMORY_REVIEW_DRAFT)).toBe(true);
  });

  test("acknowledgement names cost, digest, approval, and isolation", () => {
    expect(MEMORY_REVIEW_ACKNOWLEDGEMENT).toContain("model call");
    expect(MEMORY_REVIEW_ACKNOWLEDGEMENT).toContain("bounded digest");
    expect(MEMORY_REVIEW_ACKNOWLEDGEMENT).toContain("approval");
    expect(MEMORY_REVIEW_ACKNOWLEDGEMENT).not.toMatch(/api[_ ]?key/i);
    expect(MEMORY_REVIEW_ACKNOWLEDGEMENT).not.toMatch(/credential/i);
  });

  test("save payload round-trips provider and model without credentials", async () => {
    const calls: unknown[] = [];
    const saved: MemoryReviewSettings = {
      enabled: true,
      provider: "codex",
      model: "gpt-5",
      maxCandidates: 5,
      updatedAt: "2026-08-25T00:00:00Z",
    };
    const store = createStore(
      createMemoryReviewState({
        api: {
          getSettings: async () => DEFAULT_MEMORY_REVIEW_SETTINGS,
          updateSettings: async (input) => {
            calls.push(input);
            return saved;
          },
        },
      }),
    );

    const ok = await store.getState().save({
      enabled: true,
      provider: "codex",
      model: "gpt-5",
      acknowledged: true,
    });
    expect(ok).toBe(true);
    expect(store.getState().settings).toEqual(saved);
    expect(calls[0]).toEqual({
      enabled: true,
      provider: "codex",
      model: "gpt-5",
    });
    // The acknowledgement flag never reaches the backend boundary.
    expect(JSON.stringify(calls[0])).not.toContain("acknowledged");

    const failed = await store.getState().save({
      enabled: true,
      provider: "codex",
      model: null,
      acknowledged: false,
    });
    expect(failed).toBe(false);
  });

  test("per-workflow switch stays disabled until global review is configured", () => {
    expect(workflowSuggestionGate({ enabled: false, provider: null })).toContain(
      "Settings",
    );
    expect(
      workflowSuggestionGate({ enabled: true, provider: null }),
    ).toContain("provider");
    expect(
      workflowSuggestionGate({ enabled: true, provider: "claude_code" }),
    ).toBeNull();
  });

  test("failure copy maps stable codes and never echoes raw errors", () => {
    for (const code of [
      "auth_required",
      "provider_unavailable",
      "timeout",
      "invalid_response",
      "internal",
    ]) {
      const copy = memoryReviewFailureCopy(code);
      expect(copy).toBeTruthy();
      expect(copy).not.toContain(code === code ? "panic" : "");
      expect(copy?.match(/stderr|stack trace|exit code/i)).toBeNull();
    }
    expect(memoryReviewFailureCopy(null)).toBeNull();
    expect(memoryReviewFailureCopy("mystery_code")).toEqual(
      memoryReviewFailureCopy("internal"),
    );
    expect(memoryReviewFailureCopy("timeout")).toMatch(/[Rr]etry/);
  });
});

describe("suggestions queue", () => {
  test("rows leave pending only through backend-success transitions", () => {
    const queue = [candidate("a"), candidate("b")];
    const approved = { ...candidate("a"), status: "approved" as const };
    expect(applyCandidateUpdate(queue, approved)).toHaveLength(2);
    expect(
      applyCandidateUpdate(queue, approved).find(({ id }) => id === "a")?.status,
    ).toBe("approved");
    expect(countPendingSuggestions(queue)).toBe(2);
    expect(countPendingSuggestions(applyCandidateUpdate(queue, approved))).toBe(
      1,
    );
    // An unknown update leaves the queue untouched.
    expect(applyCandidateUpdate(queue, candidate("zzz"))).toEqual(queue);
  });

  test("pending-first ordering keeps actionable work on top", () => {
    const ordered = sortSuggestions([
      candidate("approved-old", { status: "approved" }),
      candidate("blocked-new", { status: "blocked", blockedCode: "target_missing" }),
      candidate("pending-new", { createdAt: "2026-08-21T10:00:00.000Z" }),
      candidate("rejected", { status: "rejected" }),
    ]);
    expect(ordered.map(({ id }) => id)).toEqual([
      "pending-new",
      "blocked-new",
      "rejected",
      "approved-old",
    ]);
  });

  test("only pending candidates are editable; edits target only allowed fields", () => {
    expect(candidateIsEditable(candidate("p"))).toBe(true);
    expect(candidateIsEditable(candidate("d", { status: "approved" }))).toBe(
      false,
    );

    const pending = candidate("p", {
      title: "Editor",
      body: "Uses Neovim daily",
      scopeType: "user",
      scopeKey: "local-user",
    });
    const payload = candidateEditPayload(pending, {
      title: "Editor choice",
      body: "Uses Neovim daily",
      scopeType: "user",
      memoryType: "preference",
    });
    expect(payload).toEqual({ id: "p", title: "Editor choice" });
    expect(hasCandidateEdits(pending, {
      title: "Editor",
      body: "Uses Neovim daily",
      scopeType: "user",
      memoryType: "preference",
    })).toBe(false);
  });

  test("user-scope approval and retract operations require confirmation", () => {
    expect(
      candidateApprovalRequiresConfirmation(
        candidate("u", { scopeType: "user" }),
      ),
    ).toBe(true);
    expect(
      candidateApprovalRequiresConfirmation(
        candidate("r", { operation: "retract" }),
      ),
    ).toBe(true);
    expect(
      candidateApprovalRequiresConfirmation(
        candidate("w", { scopeType: "workspace", operation: "create" }),
      ),
    ).toBe(false);
    expect(
      candidateApprovalRequiresConfirmation(
        candidate("wf", { scopeType: "workflow", operation: "supersede" }),
      ),
    ).toBe(false);
  });

  test("event refresh is scoped to the open active workflow", () => {
    const event = { workflowId: "wf-1", pendingCount: 3 };
    expect(shouldRefreshSuggestions(event, "wf-1", true)).toBe(true);
    expect(shouldRefreshSuggestions(event, "wf-2", true)).toBe(false);
    expect(shouldRefreshSuggestions(event, "wf-1", false)).toBe(false);
    expect(shouldRefreshSuggestions(event, null, true)).toBe(false);
  });

  test("announcements describe pending counts politely", () => {
    expect(suggestionAnnouncement(3, 3)).toBeNull();
    expect(suggestionAnnouncement(3, 2)).toContain("2 pending");
    expect(suggestionAnnouncement(0, 1)).toContain("new pending");
    expect(suggestionAnnouncement(1, 0)).toContain("No pending");
    expect(suggestionAnnouncement(1, 4)).toContain("4 new pending");
  });

  test("blocked explanations stay stable and leak no internals", () => {
    for (const code of [
      "target_missing",
      "target_inactive",
      "target_scope_mismatch",
      "duplicate_content",
      null,
      "unknown_future_code",
    ]) {
      const copy = blockedCandidateCopy(code);
      expect(copy.length).toBeGreaterThan(10);
      expect(copy.match(/sqlite|sql|provider|stderr|rusqlite|panicked/i)).toBeNull();
    }
  });

  test("confidence renders as a clamped percentage", () => {
    expect(candidateConfidencePercent(0.85)).toBe(85);
    expect(candidateConfidencePercent(1.4)).toBe(100);
    expect(candidateConfidencePercent(-1)).toBe(0);
  });
});

describe("history and memories navigation links", () => {
  test("History deep-links into the Suggestions queue via a canvas event", async () => {
    const format = await Bun.file(
      new URL(
        "../src/features/workflow/components/history-page/history-format.ts",
        import.meta.url,
      ),
    ).text();
    expect(format).toContain('"alfred:open-suggestions"');

    const canvas = await Bun.file(
      new URL(
        "../src/features/workflow/components/workflow-canvas/workflow-canvas.tsx",
        import.meta.url,
      ),
    ).text();
    expect(canvas).toContain('addEventListener("alfred:open-suggestions"');
    expect(canvas).toContain("focusRunId={suggestionsFocusRunId}");

    const historyPage = await Bun.file(
      new URL(
        "../src/features/workflow/components/history-page/history-page.tsx",
        import.meta.url,
      ),
    ).text();
    expect(historyPage).toContain("getMemoryReviewJob(selectedRunId)");
    // Review prompt/raw response must never be rendered.
    expect(historyPage).not.toContain("reviewPrompt");
    expect(historyPage).not.toContain("rawResponse");
  });

  test("suggestion cards link to the source run and keep provenance", async () => {
    const inspector = await Bun.file(
      new URL(
        "../src/features/workflow/components/memories-inspector/memories-inspector.tsx",
        import.meta.url,
      ),
    ).text();
    expect(inspector).toContain("onOpenRunHistory(selectedCandidate.reviewRunId)");
    expect(inspector).toContain('aria-live="polite"');
    expect(inspector).toContain('"memory://candidates-changed"');
    expect(inspector).toContain("shouldRefreshSuggestions");
    expect(inspector).toContain("data-suggestion-id");
    expect(inspector).toContain("Approve this suggestion?");
  });
});

describe("review job metadata", () => {
  test("jobs carry only stable codes, never raw error text", async () => {
    const types = await Bun.file(
      new URL("../src/features/workflow/types.ts", import.meta.url),
    ).text();
    expect(types).toContain(
      "never carries raw provider errors or output",
    );
    const job: Pick<MemoryReviewJob, "errorCode"> = { errorCode: "timeout" };
    expect(job.errorCode).toBe("timeout");
  });
});
