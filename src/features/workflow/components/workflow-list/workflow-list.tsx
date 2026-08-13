import {
  useEffect,
  useMemo,
  useRef,
  useState,
  type PointerEvent as ReactPointerEvent,
} from "react";
import { createPortal } from "react-dom";
import { WorkflowListItem } from "../workflow-list-item";
import { formatScheduleLabel } from "../../schedule-label";
import type {
  AgentProviderId,
  Schedule,
  Workflow,
  WorkflowFolder,
  WorkflowNode,
} from "../../types";

type Props = {
  workflows: Workflow[];
  folders: WorkflowFolder[];
  activeWorkflowId: string | null;
  activeLiveNodes?: WorkflowNode[];
  schedules: Schedule[];
  runningProviderByWorkflowId?: Record<string, AgentProviderId | null>;
  onSelect: (id: string) => void;
  onOpenMenu: (target: { id: string; name: string; x: number; y: number }) => void;
  onOpenFolderMenu: (target: {
    id: string;
    name: string;
    x: number;
    y: number;
  }) => void;
  onMoveToFolder: (
    workflowId: string,
    folderId: string | null,
    beforeWorkflowId?: string,
  ) => void;
};

type DragGhost = {
  id: string;
  width: number;
  x: number;
  y: number;
  offsetX: number;
  offsetY: number;
};

type DropTarget = {
  folderId: string | null;
  beforeWorkflowId?: string;
};

type WorkflowGroup = {
  key: string;
  folder: WorkflowFolder | null;
  workflows: Workflow[];
};

const DRAG_THRESHOLD_PX = 4;
const COLLAPSED_FOLDERS_KEY = "alfred:collapsed-workflow-folders";
const UNFILED_KEY = "__unfiled__";

function loadCollapsedFolders(): string[] {
  try {
    const value = JSON.parse(
      localStorage.getItem(COLLAPSED_FOLDERS_KEY) ?? "[]",
    ) as unknown;
    return Array.isArray(value)
      ? value.filter((item): item is string => typeof item === "string")
      : [];
  } catch {
    return [];
  }
}

function folderIdFromKey(key: string): string | null {
  return key === UNFILED_KEY ? null : key;
}

function folderIcon() {
  return (
    <svg width="13" height="13" viewBox="0 0 16 16" fill="none" aria-hidden>
      <path
        d="M2.5 4.5h4l1.2 1.2H13.5v6.3a.9.9 0 0 1-.9.9h-9a.9.9 0 0 1-.9-.9V4.5Z"
        stroke="currentColor"
        strokeWidth="1.35"
        strokeLinejoin="round"
      />
    </svg>
  );
}

function dropTargetAtPoint(clientX: number, clientY: number): DropTarget | null {
  const element = document.elementFromPoint(clientX, clientY);
  const group = element?.closest<HTMLElement>(".workflow-folder-group");
  const key = group?.dataset.folderKey;
  if (!group || !key) return null;

  const cards = [
    ...group.querySelectorAll<HTMLElement>(".workflow-card[data-workflow-id]"),
  ].filter((card) => !card.classList.contains("is-placeholder"));
  for (const card of cards) {
    const rect = card.getBoundingClientRect();
    if (clientY < rect.top + rect.height / 2) {
      return {
        folderId: folderIdFromKey(key),
        beforeWorkflowId: card.dataset.workflowId,
      };
    }
  }
  return { folderId: folderIdFromKey(key) };
}

export function WorkflowList({
  workflows,
  folders,
  activeWorkflowId,
  activeLiveNodes,
  schedules,
  runningProviderByWorkflowId = {},
  onSelect,
  onOpenMenu,
  onOpenFolderMenu,
  onMoveToFolder,
}: Props) {
  const [collapsedFolders, setCollapsedFolders] = useState(loadCollapsedFolders);
  const [draggingId, setDraggingId] = useState<string | null>(null);
  const [ghost, setGhost] = useState<DragGhost | null>(null);
  const [dropTarget, setDropTarget] = useState<DropTarget | null>(null);
  const dropTargetRef = useRef<DropTarget | null>(null);
  const suppressClickRef = useRef(false);

  const scheduleLabels = useMemo(
    () =>
      new Map(
        schedules
          .filter((schedule) => schedule.enabled)
          .map((schedule) => [
            schedule.workflowId,
            formatScheduleLabel(schedule.cron, schedule.nextRunAt),
          ]),
      ),
    [schedules],
  );
  const groups = useMemo<WorkflowGroup[]>(() => {
    if (folders.length === 0) return [];
    const known = new Set(folders.map((folder) => folder.id));
    const folderGroups = folders.map((folder) => ({
      key: folder.id,
      folder,
      workflows: workflows.filter((workflow) => workflow.folderId === folder.id),
    }));
    const unfiled = workflows.filter(
      (workflow) => !workflow.folderId || !known.has(workflow.folderId),
    );
    return [
      ...folderGroups,
      { key: UNFILED_KEY, folder: null, workflows: unfiled },
    ];
  }, [folders, workflows]);

  useEffect(() => {
    if (!activeWorkflowId) return;
    const active = workflows.find((workflow) => workflow.id === activeWorkflowId);
    if (!active) return;
    const known = folders.some((folder) => folder.id === active.folderId);
    const key = known && active.folderId ? active.folderId : UNFILED_KEY;
    setCollapsedFolders((current) =>
      current.includes(key) ? current.filter((item) => item !== key) : current,
    );
  }, [activeWorkflowId, folders, workflows]);

  useEffect(() => {
    try {
      localStorage.setItem(
        COLLAPSED_FOLDERS_KEY,
        JSON.stringify(collapsedFolders),
      );
    } catch {
      /* ignore */
    }
  }, [collapsedFolders]);

  if (workflows.length === 0 && folders.length === 0) {
    return <p className="workflow-list-empty muted">Create a workflow to start.</p>;
  }

  const toggleFolder = (key: string) => {
    setCollapsedFolders((current) =>
      current.includes(key)
        ? current.filter((item) => item !== key)
        : [...current, key],
    );
  };

  const onCardPointerDown = (
    event: ReactPointerEvent<HTMLDivElement>,
    id: string,
  ) => {
    if (event.button !== 0) return;

    const card = event.currentTarget.closest(".workflow-card") as HTMLElement | null;
    if (!card) return;

    const rect = card.getBoundingClientRect();
    const pointerId = event.pointerId;
    const startX = event.clientX;
    const startY = event.clientY;
    const offsetX = startX - rect.left;
    const offsetY = startY - rect.top;
    const dragSurface = event.currentTarget;
    let active = false;

    try {
      dragSurface.setPointerCapture(pointerId);
    } catch {
      /* ignore */
    }

    const onMove = (moveEvent: PointerEvent) => {
      if (moveEvent.pointerId !== pointerId) return;
      if (!active) {
        const distance = Math.hypot(
          moveEvent.clientX - startX,
          moveEvent.clientY - startY,
        );
        if (distance < DRAG_THRESHOLD_PX) return;
        moveEvent.preventDefault();
        active = true;
        setDraggingId(id);
        setGhost({
          id,
          width: rect.width,
          x: moveEvent.clientX - offsetX,
          y: moveEvent.clientY - offsetY,
          offsetX,
          offsetY,
        });
        document.body.classList.add("is-workflow-reordering");
      }

      moveEvent.preventDefault();

      setGhost((current) =>
        current
          ? {
              ...current,
              x: moveEvent.clientX - current.offsetX,
              y: moveEvent.clientY - current.offsetY,
            }
          : current,
      );

      const scroller = card.closest(".sidebar-scroll") as HTMLElement | null;
      if (scroller) {
        const scrollerRect = scroller.getBoundingClientRect();
        const edge = 36;
        if (moveEvent.clientY < scrollerRect.top + edge) scroller.scrollTop -= 14;
        else if (moveEvent.clientY > scrollerRect.bottom - edge) scroller.scrollTop += 14;
      }

      const nextTarget = dropTargetAtPoint(moveEvent.clientX, moveEvent.clientY);
      dropTargetRef.current = nextTarget;
      setDropTarget(nextTarget);
    };

    const onUp = (upEvent: PointerEvent) => {
      if (upEvent.pointerId !== pointerId) return;
      window.removeEventListener("pointermove", onMove);
      window.removeEventListener("pointerup", onUp);
      window.removeEventListener("pointercancel", onUp);
      try {
        dragSurface.releasePointerCapture(pointerId);
      } catch {
        /* ignore */
      }

      document.body.classList.remove("is-workflow-reordering");
      const target = dropTargetRef.current;
      dropTargetRef.current = null;
      setDraggingId(null);
      setGhost(null);
      setDropTarget(null);

      if (!active || !target || target.beforeWorkflowId === id) return;
      suppressClickRef.current = true;
      const key = target.folderId ?? UNFILED_KEY;
      setCollapsedFolders((current) => current.filter((item) => item !== key));
      onMoveToFolder(id, target.folderId, target.beforeWorkflowId);
    };

    window.addEventListener("pointermove", onMove);
    window.addEventListener("pointerup", onUp);
    window.addEventListener("pointercancel", onUp);
  };

  const draggingWorkflow = draggingId
    ? workflows.find((workflow) => workflow.id === draggingId)
    : null;
  const dropKey = dropTarget?.folderId ?? (dropTarget ? UNFILED_KEY : null);

  const renderWorkflow = (workflow: Workflow) => {
    const running = Object.prototype.hasOwnProperty.call(
      runningProviderByWorkflowId,
      workflow.id,
    );
    return (
      <WorkflowListItem
        key={workflow.id}
        workflow={workflow}
        liveNodes={
          workflow.id === activeWorkflowId ? activeLiveNodes : undefined
        }
        active={workflow.id === activeWorkflowId}
        scheduleLabel={scheduleLabels.get(workflow.id)}
        running={running}
        runningProvider={runningProviderByWorkflowId[workflow.id] ?? null}
        dragging={draggingId === workflow.id}
        onSelect={() => {
          if (suppressClickRef.current) {
            suppressClickRef.current = false;
            return;
          }
          onSelect(workflow.id);
        }}
        onOpenMenu={({ x, y }) =>
          onOpenMenu({ id: workflow.id, name: workflow.name, x, y })
        }
        onDragPointerDown={(event) => onCardPointerDown(event, workflow.id)}
      />
    );
  };

  return (
    <div className={draggingId ? "workflow-groups is-reordering" : "workflow-groups"}>
      {folders.length === 0 ? (
        <section
          className={[
            "workflow-folder-group",
            "workflow-folder-group--flat",
            dropKey === UNFILED_KEY ? "is-drop-target" : "",
          ]
            .filter(Boolean)
            .join(" ")}
          data-folder-key={UNFILED_KEY}
        >
          <ul className="workflow-list is-flat">
            {workflows.map(renderWorkflow)}
          </ul>
        </section>
      ) : groups.map((group) => {
        const collapsed = collapsedFolders.includes(group.key);
        const folderName = group.folder?.name ?? "Unfiled";
        const groupId = `workflow-folder-${group.key}`;
        return (
          <section
            key={group.key}
            className={[
              "workflow-folder-group",
              collapsed ? "is-collapsed" : "",
              dropKey === group.key ? "is-drop-target" : "",
            ]
              .filter(Boolean)
              .join(" ")}
            data-folder-key={group.key}
          >
            <div
              className="workflow-folder-header"
              onContextMenu={(event) => {
                if (!group.folder) return;
                event.preventDefault();
                onOpenFolderMenu({
                  id: group.folder.id,
                  name: group.folder.name,
                  x: event.clientX,
                  y: event.clientY,
                });
              }}
            >
              <button
                type="button"
                className="workflow-folder-toggle"
                aria-expanded={!collapsed}
                aria-controls={groupId}
                title={`${collapsed ? "Expand" : "Collapse"} ${folderName}`}
                onClick={() => toggleFolder(group.key)}
              >
                <span className="workflow-folder-chevron" aria-hidden>›</span>
                <span className="workflow-folder-icon">{folderIcon()}</span>
                <span className="workflow-folder-name">{folderName}</span>
                <span className="workflow-folder-count">{group.workflows.length}</span>
              </button>
              {group.folder ? (
                <button
                  type="button"
                  className="workflow-folder-options"
                  aria-label={`Folder options for ${group.folder.name}`}
                  title="Folder options"
                  onClick={(event) => {
                    const rect = event.currentTarget.getBoundingClientRect();
                    onOpenFolderMenu({
                      id: group.folder!.id,
                      name: group.folder!.name,
                      x: rect.right,
                      y: rect.bottom + 4,
                    });
                  }}
                >
                  ···
                </button>
              ) : null}
            </div>

            <div
              id={groupId}
              className="workflow-folder-content"
              aria-hidden={collapsed}
              inert={collapsed}
            >
              <ul className="workflow-list">
                {group.workflows.map(renderWorkflow)}
                {group.workflows.length === 0 ? (
                  <li className="workflow-folder-empty">Drop workflows here</li>
                ) : null}
              </ul>
            </div>
          </section>
        );
      })}

      {ghost && draggingWorkflow
        ? createPortal(
            <div
              className="workflow-drag-ghost"
              style={{
                width: ghost.width,
                transform: `translate3d(${ghost.x}px, ${ghost.y}px, 0) scale(1.02)`,
              }}
            >
              <WorkflowListItem
                workflow={draggingWorkflow}
                liveNodes={
                  draggingWorkflow.id === activeWorkflowId
                    ? activeLiveNodes
                    : undefined
                }
                active={draggingWorkflow.id === activeWorkflowId}
                scheduleLabel={scheduleLabels.get(draggingWorkflow.id)}
                running={Object.prototype.hasOwnProperty.call(
                  runningProviderByWorkflowId,
                  draggingWorkflow.id,
                )}
                runningProvider={
                  runningProviderByWorkflowId[draggingWorkflow.id] ?? null
                }
                dragging={false}
                onSelect={() => {}}
                onOpenMenu={() => {}}
                onDragPointerDown={(event) => event.preventDefault()}
              />
            </div>,
            document.body,
          )
        : null}
    </div>
  );
}
