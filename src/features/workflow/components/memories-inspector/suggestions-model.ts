import type {
  MemoryCandidate,
  MemoryCandidateStatus,
  MemoryReviewJob,
  MemoryScopeType,
  MemoryType,
} from "../../types";

/**
 * Pure contracts for the Memories inspector's Suggestions queue. The React
 * component owns state; everything decision-shaped lives here so the approval
 * boundary stays testable.
 */

export const CANDIDATE_OPERATION_LABELS: Record<
  MemoryCandidate["operation"],
  string
> = {
  create: "New memory",
  supersede: "Supersede",
  retract: "Retract",
};

export const CANDIDATE_STATUS_LABELS: Record<MemoryCandidateStatus, string> = {
  pending: "Pending",
  approved: "Approved",
  rejected: "Rejected",
  blocked: "Blocked",
};

export const SUGGESTION_SCOPE_TYPES: MemoryScopeType[] = [
  "workflow",
  "workspace",
  "user",
];

export function candidateConfidencePercent(confidence: number): number {
  return Math.round(Math.min(Math.max(confidence, 0), 1) * 100);
}

/**
 * Approving a user-scope suggestion or any retract touches memory visible
 * beyond this workflow, so the UI must confirm before calling the backend.
 */
export function candidateApprovalRequiresConfirmation(
  candidate: Pick<MemoryCandidate, "scopeType" | "operation">,
): boolean {
  return candidate.scopeType === "user" || candidate.operation === "retract";
}

/** Only pending candidates are editable; decided ones are final. */
export function candidateIsEditable(
  candidate: Pick<MemoryCandidate, "status">,
): boolean {
  return candidate.status === "pending";
}

/** Stable explanation for a blocked candidate; never provider or DB detail. */
export function blockedCandidateCopy(code: string | null | undefined): string {
  switch (code) {
    case "target_missing":
      return "The memory this suggestion wanted to change no longer exists.";
    case "target_inactive":
      return "The target memory is no longer active, so this suggestion cannot be applied.";
    case "target_scope_mismatch":
      return "The scope of the target memory changed after this review ran.";
    case "duplicate_content":
      return "An equivalent active memory already exists, so approving would duplicate it.";
    case "target_forbidden":
    case "target_required":
      return "This suggestion does not match its operation, so it cannot be applied as-is. Edit or reject it.";
    default:
      return "This suggestion no longer matches your current memories. Review it and try again, or reject it.";
  }
}

/** Queue order: actionable pending first, then blocked, then decided. */
const STATUS_ORDER: Record<MemoryCandidateStatus, number> = {
  pending: 0,
  blocked: 1,
  rejected: 2,
  approved: 3,
};

export function sortSuggestions(
  candidates: MemoryCandidate[],
): MemoryCandidate[] {
  return [...candidates].sort((a, b) => {
    const byStatus = STATUS_ORDER[a.status] - STATUS_ORDER[b.status];
    if (byStatus !== 0) return byStatus;
    return (
      new Date(b.createdAt).getTime() - new Date(a.createdAt).getTime()
    );
  });
}

export function countPendingSuggestions(
  candidates: Pick<MemoryCandidate, "status">[],
): number {
  return candidates.filter((candidate) => candidate.status === "pending")
    .length;
}

/**
 * Replace a candidate in place after a backend decision succeeds. Rows leave
 * Pending only through this transition — never optimistically.
 */
export function applyCandidateUpdate(
  candidates: MemoryCandidate[],
  updated: MemoryCandidate,
): MemoryCandidate[] {
  return candidates.map((candidate) =>
    candidate.id === updated.id ? updated : candidate,
  );
}

/** Edit payload for a pending suggestion; empty edits are omitted. */
export type CandidateEditDraft = {
  title: string;
  body: string;
  scopeType: MemoryScopeType;
  memoryType: MemoryType;
};

export function candidateEditPayload(
  candidate: MemoryCandidate,
  draft: CandidateEditDraft,
): {
  id: string;
  title?: string;
  body?: string;
  scopeType?: MemoryScopeType;
  memoryType?: MemoryType;
} {
  const title = draft.title.trim();
  const body = draft.body.trim();
  return {
    id: candidate.id,
    ...(title && title !== candidate.title ? { title } : {}),
    ...(body && body !== candidate.body ? { body } : {}),
    ...(draft.scopeType !== candidate.scopeType
      ? { scopeType: draft.scopeType }
      : {}),
    ...(draft.memoryType !== candidate.memoryType
      ? { memoryType: draft.memoryType }
      : {}),
  };
}

export function hasCandidateEdits(
  candidate: MemoryCandidate,
  draft: CandidateEditDraft,
): boolean {
  return Object.keys(candidateEditPayload(candidate, draft)).length > 1;
}

/**
 * Event-scoped refresh: `memory://candidates-changed` refreshes only when the
 * inspector is open AND the event belongs to the active workflow.
 */
export function shouldRefreshSuggestions(
  event: { workflowId?: string; pendingCount?: number },
  activeWorkflowId: string | null,
  open: boolean,
): boolean {
  return Boolean(open && activeWorkflowId && event.workflowId === activeWorkflowId);
}

export function suggestionAnnouncement(
  previousPending: number,
  nextPending: number,
): string | null {
  if (previousPending === nextPending) return null;
  if (nextPending === 0) return "No pending memory suggestions.";
  if (nextPending < previousPending)
    return `${nextPending} pending memory suggestion${
      nextPending === 1 ? "" : "s"
    } remaining.`;
  return `${nextPending} new pending memory suggestion${
    nextPending === 1 ? "" : "s"
  }.`;
}

/** Failed reviews surfaced at the top of the Suggestions queue. */
export function failedReviewJobs(
  jobs: MemoryReviewJob[],
): MemoryReviewJob[] {
  return jobs.filter((job) => job.status === "failed");
}

export function reviewStatusLabel(
  status: MemoryReviewJob["status"],
): string {
  switch (status) {
    case "pending":
      return "Queued";
    case "running":
      return "Reviewing";
    case "completed":
      return "Reviewed";
    case "failed":
      return "Review failed";
    case "skipped":
      return "Skipped";
    default:
      return status;
  }
}
