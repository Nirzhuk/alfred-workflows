import {
  useEffect,
  useState,
  type PointerEvent as ReactPointerEvent,
} from "react";
import { AgentMark } from "../../../../components/agent-mark";
import type { AgentProviderId, Workflow, WorkflowNode } from "../../types";

type Props = {
  workflow: Workflow;
  /** Live graph for the active workflow (unsaved edits). */
  liveNodes?: WorkflowNode[];
  active: boolean;
  /** Unsaved edits on the live graph. */
  dirty?: boolean;
  scheduleLabel?: string;
  running?: boolean;
  runningProvider?: AgentProviderId | null;
  /** Slot left behind while the floating ghost is dragged. */
  dragging?: boolean;
  onSelect: () => void;
  onOpenMenu: (screen: { x: number; y: number }) => void;
  onDragPointerDown: (event: ReactPointerEvent<HTMLDivElement>) => void;
};

function providersInNodes(nodes: WorkflowNode[] | undefined): AgentProviderId[] {
  const seen = new Set<AgentProviderId>();
  const order: AgentProviderId[] = [];
  for (const node of nodes ?? []) {
    if (node.type !== "agent") continue;
    const provider = (node.data as { provider?: AgentProviderId } | undefined)
      ?.provider;
    if (!provider || seen.has(provider)) continue;
    seen.add(provider);
    order.push(provider);
  }
  return order;
}

function shortFolder(path: string | undefined): string {
  const trimmed = path?.trim() ?? "";
  if (!trimmed) return "No folder set";
  const normalized = trimmed.replace(/\\/g, "/");
  const parts = normalized.split("/").filter(Boolean);
  if (parts.length <= 2) {
    return normalized.startsWith("/") ? `/${parts.join("/")}` : parts.join("/");
  }
  return `…/${parts.slice(-2).join("/")}`;
}

function TransitioningScheduleText({ value }: { value: string }) {
  const [labels, setLabels] = useState<{
    current: string;
    previous: string | null;
  }>({ current: value, previous: null });

  useEffect(() => {
    setLabels((current) =>
      current.current === value
        ? current
        : { current: value, previous: current.current },
    );
  }, [value]);

  useEffect(() => {
    if (!labels.previous) return;
    const timeout = window.setTimeout(() => {
      setLabels((current) => ({ ...current, previous: null }));
    }, 200);
    return () => window.clearTimeout(timeout);
  }, [labels.current, labels.previous]);

  return (
    <span className="workflow-card-schedule-text-wrap" aria-live="polite">
      {labels.previous ? (
        <span
          className="workflow-card-schedule-text is-outgoing"
          aria-hidden
        >
          {labels.previous}
        </span>
      ) : null}
      <span
        key={labels.current}
        className={`workflow-card-schedule-text${labels.previous ? " is-incoming" : ""}`}
      >
        {labels.current}
      </span>
    </span>
  );
}

export function WorkflowListItem({
  workflow,
  liveNodes,
  active,
  dirty,
  scheduleLabel,
  running,
  runningProvider,
  dragging,
  onSelect,
  onOpenMenu,
  onDragPointerDown,
}: Props) {
  const nodes = liveNodes ?? workflow.graph?.nodes ?? [];
  const providers = providersInNodes(nodes);
  const folder = shortFolder(workflow.workingDirectory);
  const hasFolder = Boolean(workflow.workingDirectory?.trim());

  return (
    <li
      className={[
        "workflow-card",
        active ? "is-active" : "",
        dirty ? "is-dirty" : "",
        dragging ? "is-placeholder" : "",
      ]
        .filter(Boolean)
        .join(" ")}
      data-workflow-id={workflow.id}
    >
      <div
        className="workflow-card-button"
        role="button"
        tabIndex={dragging ? -1 : 0}
        aria-hidden={dragging || undefined}
        onPointerDown={onDragPointerDown}
        onClick={onSelect}
        onKeyDown={(e) => {
          if (e.key === "Enter" || e.key === " ") {
            e.preventDefault();
            onSelect();
          }
        }}
        onContextMenu={(e) => {
          e.preventDefault();
          e.stopPropagation();
          onSelect();
          onOpenMenu({ x: e.clientX, y: e.clientY });
        }}
      >
        {dirty ? (
          <svg className="workflow-card-dirty-border" aria-hidden>
            <rect width="100%" height="100%" />
          </svg>
        ) : null}
        {dirty ? (
          <span id={`${workflow.id}-unsaved`} className="sr-only">
            Unsaved changes
          </span>
        ) : null}
        <div className="workflow-card-content">
          <div className="workflow-card-top">
            <span className="workflow-card-dot" aria-hidden />
            <span className="workflow-card-name">{workflow.name}</span>
            {running ? (
              <span
                className="workflow-card-running"
                title="Workflow is running"
              >
                {runningProvider ? (
                  <AgentMark provider={runningProvider} size={14} running />
                ) : (
                  <span className="running-status-dot" aria-hidden />
                )}
                <span>Running</span>
              </span>
            ) : scheduleLabel ? (
              <span
                className="workflow-card-schedule"
                title={`Runs ${scheduleLabel}`}
              >
                <span className="workflow-card-schedule-dot" aria-hidden />
                <TransitioningScheduleText value={scheduleLabel} />
              </span>
            ) : null}
          </div>

          <div
            className={[
              "workflow-card-folder",
              hasFolder ? "" : "is-empty",
            ]
              .filter(Boolean)
              .join(" ")}
            title={
              workflow.workingDirectory?.trim() || "Set a working directory"
            }
          >
            <svg
              className="workflow-card-folder-icon"
              width="11"
              height="11"
              viewBox="0 0 16 16"
              fill="none"
              aria-hidden
            >
              <path
                d="M2.5 4.5h4l1.2 1.2H13.5v6.3a.9.9 0 0 1-.9.9h-9a.9.9 0 0 1-.9-.9V4.5Z"
                stroke="currentColor"
                strokeWidth="1.4"
                strokeLinejoin="round"
              />
            </svg>
            <span>{folder}</span>
          </div>

          <div className="workflow-card-agents">
            {providers.length > 0 ? (
              <>
                <span className="workflow-card-agent-logos">
                  {providers.map((provider) => (
                    <AgentMark key={provider} provider={provider} size={14} />
                  ))}
                </span>
                <span className="workflow-card-agent-count">
                  {providers.length} agent{providers.length === 1 ? "" : "s"}
                </span>
              </>
            ) : (
              <span className="workflow-card-agent-count is-empty">
                No agents yet
              </span>
            )}
          </div>
        </div>
      </div>
    </li>
  );
}
