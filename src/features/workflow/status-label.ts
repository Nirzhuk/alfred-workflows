export type WorkflowStatusLabelState = {
  activeWorkflowId: string | null;
  activeRunStatus?: string | null;
  dirty: boolean;
  loading: boolean;
  workflowLoading: boolean;
  showWorkflowLoading?: boolean;
};

export function getWorkflowStatusLabel({
  activeWorkflowId,
  activeRunStatus,
  dirty,
  loading,
  workflowLoading,
  showWorkflowLoading = false,
}: WorkflowStatusLabelState): string {
  if (workflowLoading && showWorkflowLoading) return "Loading workflow…";
  if (activeRunStatus === "running") return "Automation running…";
  if (workflowLoading) {
    if (dirty) return "Unsaved changes";
    return activeWorkflowId ? "All changes saved" : "No workflow open";
  }
  if (loading) return "Working…";
  if (dirty) return "Unsaved changes";
  return activeWorkflowId ? "All changes saved" : "No workflow open";
}
