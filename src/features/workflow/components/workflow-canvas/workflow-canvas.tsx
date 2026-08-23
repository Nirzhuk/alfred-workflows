import { useEffect, useMemo, useState } from "react";
import { isTauri } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";
import { ReactFlowProvider } from "@xyflow/react";
import "@xyflow/react/dist/style.css";
import { AppTitlebar } from "../app-title-bar";
import { ConfirmDialog } from "../../../../components/confirm-dialog";
import { openLatestDownload } from "../../../licensing/download-latest";
import { SettingsPage } from "../../../settings/components/settings-page";
import {
  SettingsSidebar,
  type SettingsSectionId,
} from "../../../settings/components/settings-sidebar";
import { SchedulesPage } from "../schedules-page";
import { SidebarBottomBar } from "../sidebar-bottom-bar";
import { SidebarNav, type SidebarView } from "../sidebar-nav";
import { AgentUsageBar } from "../agent-usage-bar";
import { WorkflowContextMenu } from "../workflow-context-menu";
import { WorkflowFolderContextMenu } from "../workflow-folder-context-menu";
import { WorkflowFolderModal } from "../workflow-folder-modal";
import { WorkflowList } from "../workflow-list";
import { FlowEditor } from "../flow-editor";
import { MemoriesInspector } from "../memories-inspector";
import { OutputModal } from "../output-modal";
import { RunActivityPanel } from "../run-activity-panel";
import { RenameWorkflowModal } from "../rename-workflow-modal";
import { ScheduleModal } from "../schedule-modal";
import { TriggersModal } from "../triggers-modal";
import { useWorkflowStore } from "../../store";
import { getWorkflowStatusLabel } from "../../status-label";
import {
  isAgentNodeData,
  isPromptNodeData,
  type AgentProviderId,
} from "../../types";
import { formatScheduleLabel } from "../../schedule-label";

function FolderGlyph() {
  return (
    <svg width="13" height="13" viewBox="0 0 16 16" fill="none" aria-hidden>
      <path
        d="M2.5 4.25h4.1l1.15 1.25H13.5v6.25a1 1 0 0 1-1 1h-9a1 1 0 0 1-1-1V4.25Z"
        stroke="currentColor"
        strokeWidth="1.3"
        strokeLinejoin="round"
      />
    </svg>
  );
}

function FolderAddGlyph() {
  return (
    <svg viewBox="0 0 24 24" fill="none" aria-hidden>
      <path
        d="M3 6.5h6l2 2H21v8.75A1.75 1.75 0 0 1 19.25 19H4.75A1.75 1.75 0 0 1 3 17.25V6.5Z"
        stroke="currentColor"
        strokeWidth="1.75"
        strokeLinecap="round"
        strokeLinejoin="round"
      />
      <path
        d="M15 11.5v5M12.5 14h5"
        stroke="currentColor"
        strokeWidth="1.75"
        strokeLinecap="round"
      />
    </svg>
  );
}

export function WorkflowCanvas() {
  const {
    nodes,
    dirty,
    loading,
    workflowLoading,
    error,
    activeWorkflowId,
    workflows,
    workflowFolders,
    workflowSchedules,
    openWorkflowIds,
    createWorkflow,
    deleteWorkflowFolder,
    moveWorkflowToFolder,
    selectWorkflow,
    closeWorkflowTab,
    deleteWorkflow,
    saveActiveWorkflow,
    runActiveWorkflow,
    cancelActiveRun,
    setWorkingDirectory,
    loadSkills,
    loadProviderModels,
    loadAgentUsage,
    runPanelOpen,
    workflowRunStates,
    openRunPanel,
    closeRunPanel,
    openOutput,
    activeNodeId,
    stepStatuses,
    stepStats,
    agentUsage,
    usageLoading,
    activeRun,
    memories,
    runLogs,
  } = useWorkflowStore();

  const activeWorkflow = workflows.find(
    (workflow) => workflow.id === activeWorkflowId,
  );
  const activeWorkingDirectory =
    activeWorkflow?.workingDirectory?.trim() ?? "";
  const [showWorkflowLoading, setShowWorkflowLoading] = useState(false);
  const workflowStatusLabel = getWorkflowStatusLabel({
    activeWorkflowId,
    activeRunStatus: activeRun?.status,
    dirty,
    loading,
    workflowLoading,
    showWorkflowLoading,
  });

  const canOpenActivity =
    Boolean(activeRun) || memories.length > 0 || runLogs.length > 0;
  const activityRunning = activeRun?.status === "running";
  const activityEventCount = useMemo(
    () => runLogs.filter((line) => Boolean(line.activity)).length,
    [runLogs],
  );
  const runningProviderByWorkflowId = useMemo<
    Record<string, AgentProviderId | null>
  >(() => {
    const workflowsById = new Map(
      workflows.map((workflow) => [workflow.id, workflow]),
    );
    const providers: Record<string, AgentProviderId | null> = {};

    for (const runState of Object.values(workflowRunStates)) {
      if (runState.activeRun?.status !== "running") continue;
      const workflowId = runState.activeRun.workflowId;
      const workflow = workflowsById.get(workflowId);
      const graphNodes =
        workflowId === activeWorkflowId
          ? nodes
          : (workflow?.graph?.nodes ?? []);
      const activeNode = graphNodes.find(
        (node) => node.id === runState.activeNodeId,
      );
      providers[workflowId] =
        activeNode?.type === "agent" && isAgentNodeData(activeNode.data)
          ? activeNode.data.provider
          : null;
    }

    return providers;
  }, [activeWorkflowId, nodes, workflowRunStates, workflows]);

  const usedProviders = useMemo(() => {
    const seen = new Set<string>();
    return nodes.flatMap((node) => {
      if (!isAgentNodeData(node.data) || seen.has(node.data.provider)) return [];
      seen.add(node.data.provider);
      return [node.data.provider];
    });
  }, [nodes]);
  const usedProvidersKey = usedProviders.join(",");
  const openCodeUsageRefreshKey = useMemo(
    () =>
      Object.entries(stepStats)
        .filter(([, stats]) => stats.provider === "opencode")
        .map(
          ([nodeId, stats]) =>
            `${nodeId}:${stats.durationMs ?? ""}:${stats.totalCostUsd ?? ""}`,
        )
        .sort()
        .join("|"),
    [stepStats],
  );

  const [workflowMenu, setWorkflowMenu] = useState<{
    id: string;
    name: string;
    x: number;
    y: number;
  } | null>(null);
  const [folderMenu, setFolderMenu] = useState<{
    id: string;
    name: string;
    x: number;
    y: number;
  } | null>(null);
  const [folderModal, setFolderModal] = useState<
    | { mode: "create" }
    | { mode: "rename"; id: string; name: string }
    | null
  >(null);
  const [deleteFolderTarget, setDeleteFolderTarget] = useState<{
    id: string;
    name: string;
  } | null>(null);
  const [renameTarget, setRenameTarget] = useState<{
    id: string;
    name: string;
  } | null>(null);
  const [scheduleTarget, setScheduleTarget] = useState<{
    id: string;
    name: string;
  } | null>(null);
  const [triggersTarget, setTriggersTarget] = useState<{
    id: string;
    name: string;
  } | null>(null);
  const [deleteTarget, setDeleteTarget] = useState<{
    id: string;
    name: string;
  } | null>(null);
  const [memoriesOpen, setMemoriesOpen] = useState(false);
  const [memoriesFocusId, setMemoriesFocusId] = useState<string | null>(null);
  const [runPanelMounted, setRunPanelMounted] = useState(runPanelOpen);
  const [view, setView] = useState<SidebarView>("canvas");
  const [settingsSection, setSettingsSection] =
    useState<SettingsSectionId>("general");
  const [schedulesTick, setSchedulesTick] = useState(0);
  const [sidebarCollapsed, setSidebarCollapsed] = useState(() => {
    try {
      return localStorage.getItem("alfred:sidebar-collapsed") === "1";
    } catch {
      return false;
    }
  });

  const setSidebarOpen = (open: boolean) => {
    const collapsed = !open;
    setSidebarCollapsed(collapsed);
    try {
      localStorage.setItem("alfred:sidebar-collapsed", collapsed ? "1" : "0");
    } catch {
      /* ignore */
    }
  };

  useEffect(() => {
    if (runPanelOpen) {
      setRunPanelMounted(true);
      return;
    }
    // Keep the DOM only for the existing close transition, then release up to
    // 1,000 console rows, memory cards, and the compositor layer.
    const timeout = window.setTimeout(() => setRunPanelMounted(false), 200);
    return () => window.clearTimeout(timeout);
  }, [runPanelOpen]);

  useEffect(() => {
    if (!workflowLoading) {
      setShowWorkflowLoading(false);
      return;
    }
    const timeout = window.setTimeout(() => setShowWorkflowLoading(true), 180);
    return () => window.clearTimeout(timeout);
  }, [workflowLoading]);

  const chooseWorkingDirectory = async (workflowId: string) => {
    const picked = await open({
      directory: true,
      multiple: false,
      title: "Choose working directory",
    });
    if (typeof picked !== "string" || !picked) return;
    await setWorkingDirectory(workflowId, picked);
  };

  const requestDelete = (id: string, name?: string) => {
    const label =
      name ?? workflows.find((w) => w.id === id)?.name ?? "this workflow";
    setDeleteTarget({ id, name: label });
  };

  useEffect(() => {
    if (!isTauri()) return;
    void loadProviderModels();
  }, [loadProviderModels]);

  useEffect(() => {
    if (!isTauri()) return;
    void loadSkills(activeWorkingDirectory || undefined);
  }, [activeWorkingDirectory, loadSkills]);

  useEffect(() => {
    if (!isTauri()) return;
    void loadAgentUsage(usedProviders);
    if (usedProviders.length === 0) return;
    const refresh = window.setInterval(
      () => void loadAgentUsage(usedProviders),
      5 * 60_000,
    );
    return () => window.clearInterval(refresh);
    // The stable key prevents a refresh loop when node objects are recreated.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [usedProvidersKey, loadAgentUsage]);

  useEffect(() => {
    if (!isTauri()) return;
    if (!openCodeUsageRefreshKey || !usedProviders.includes("opencode")) return;
    void loadAgentUsage(usedProviders);
    // Refresh local OpenCode Go history immediately after a completed step.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [openCodeUsageRefreshKey, loadAgentUsage]);

  useEffect(() => {
    const openSchedule = () => {
      const { activeWorkflowId: id, workflows: list } =
        useWorkflowStore.getState();
      if (!id) {
        window.alert("Select a workflow first.");
        return;
      }
      const name = list.find((w) => w.id === id)?.name ?? "Workflow";
      setScheduleTarget({ id, name });
    };
    const openDelete = () => {
      const { activeWorkflowId: id, workflows: list } =
        useWorkflowStore.getState();
      if (!id) {
        window.alert("Select a workflow first.");
        return;
      }
      const name = list.find((w) => w.id === id)?.name ?? "this workflow";
      setDeleteTarget({ id, name });
    };
    const openMemories = (event: Event) => {
      const detail = (event as CustomEvent<{ memoryId?: string }>).detail;
      setMemoriesFocusId(detail?.memoryId ?? null);
      setMemoriesOpen(true);
    };
    const openSettings = (event: Event) => {
      const section = (
        event as CustomEvent<{ section?: SettingsSectionId }>
      ).detail?.section;
      if (section) setSettingsSection(section);
      setView("settings");
      setMemoriesOpen(false);
    };
    const openSchedules = () => {
      setView("schedules");
      setSchedulesTick((tick) => tick + 1);
    };
    const renameWorkflow = () => {
      const { activeWorkflowId: id, workflows: list } =
        useWorkflowStore.getState();
      if (!id) {
        window.alert("Select a workflow first.");
        return;
      }
      const name =
        list.find((workflow) => workflow.id === id)?.name ?? "Workflow";
      setRenameTarget({ id, name });
    };
    const toggleSidebar = () => {
      setSidebarCollapsed((collapsed) => {
        const next = !collapsed;
        try {
          localStorage.setItem(
            "alfred:sidebar-collapsed",
            next ? "1" : "0",
          );
        } catch {
          /* ignore */
        }
        return next;
      });
    };
    const toggleActivity = () => {
      setView("canvas");
      const state = useWorkflowStore.getState();
      if (state.runPanelOpen) {
        state.closeRunPanel();
      } else if (
        state.activeRun ||
        state.memories.length > 0 ||
        state.runLogs.length > 0
      ) {
        state.openRunPanel();
      } else {
        window.alert("Run a workflow first to open the activity panel.");
      }
    };
    const downloadLatest = () => {
      void openLatestDownload();
    };
    const openWorkflow = (event: Event) => {
      const workflowId = (event as CustomEvent<{ workflowId?: string }>).detail
        ?.workflowId;
      if (!workflowId) return;
      setView("canvas");
      void selectWorkflow(workflowId);
    };
    const openActivity = () => {
      setView("canvas");
      openRunPanel(null);
    };
    const openRunOutput = (event: Event) => {
      const detail = (
        event as CustomEvent<{
          workflowId?: string;
          title?: string;
          body?: string;
        }>
      ).detail;
      if (!detail?.workflowId || !detail.body) return;
      setView("canvas");
      setMemoriesOpen(false);
      void (async () => {
        await selectWorkflow(detail.workflowId!);
        openOutput({
          title: detail.title || "Final output",
          body: detail.body!,
          nodeId: null,
        });
      })();
    };
    window.addEventListener("alfred:open-schedule", openSchedule);
    window.addEventListener("alfred:delete-workflow", openDelete);
    window.addEventListener("alfred:open-memories", openMemories);
    window.addEventListener("alfred:open-settings", openSettings);
    window.addEventListener("alfred:open-schedules", openSchedules);
    window.addEventListener("alfred:rename-workflow", renameWorkflow);
    window.addEventListener("alfred:toggle-sidebar", toggleSidebar);
    window.addEventListener("alfred:toggle-activity", toggleActivity);
    window.addEventListener("alfred:download-latest", downloadLatest);
    window.addEventListener("alfred:open-workflow", openWorkflow);
    window.addEventListener("alfred:open-activity", openActivity);
    window.addEventListener("alfred:open-run-output", openRunOutput);
    return () => {
      window.removeEventListener("alfred:open-schedule", openSchedule);
      window.removeEventListener("alfred:delete-workflow", openDelete);
      window.removeEventListener("alfred:open-memories", openMemories);
      window.removeEventListener("alfred:open-settings", openSettings);
      window.removeEventListener("alfred:open-schedules", openSchedules);
      window.removeEventListener("alfred:rename-workflow", renameWorkflow);
      window.removeEventListener("alfred:toggle-sidebar", toggleSidebar);
      window.removeEventListener("alfred:toggle-activity", toggleActivity);
      window.removeEventListener("alfred:download-latest", downloadLatest);
      window.removeEventListener("alfred:open-workflow", openWorkflow);
      window.removeEventListener("alfred:open-activity", openActivity);
      window.removeEventListener("alfred:open-run-output", openRunOutput);
    };
  }, []);

  const openTabs = useMemo(() => {
    const byId = new Map(workflows.map((w) => [w.id, w]));
    const scheduleByWorkflow = new Map(
      workflowSchedules
        .filter((item) => item.enabled)
        .map((item) => [
          item.workflowId,
          formatScheduleLabel(item.cron, item.nextRunAt),
        ]),
    );
    return openWorkflowIds
      .map((id) => byId.get(id))
      .filter((w): w is NonNullable<typeof w> => Boolean(w))
      .map((w) => ({
        id: w.id,
        name: w.name,
        dirty: w.id === activeWorkflowId ? dirty : false,
        scheduleLabel: scheduleByWorkflow.get(w.id),
      }));
  }, [openWorkflowIds, workflows, workflowSchedules, activeWorkflowId, dirty]);

  const displayNodes = useMemo(() => {
    return nodes.map((node) => {
      const status = stepStatuses[node.id];
      const isActive = node.id === activeNodeId;
      const isBlocked =
        isPromptNodeData(node.data) && Boolean(node.data.blocked);
      const className = [
        node.className,
        status && (status !== "running" || isActive)
          ? `rf-node-${status}`
          : "",
        isActive ? "rf-node-active" : "",
        isBlocked ? "rf-node-blocked" : "",
      ]
        .filter(Boolean)
        .join(" ");
      return {
        ...node,
        className,
        draggable: isBlocked ? false : node.draggable,
      };
    });
  }, [nodes, stepStatuses, activeNodeId]);

  return (
    <div className="app-frame">
      <AppTitlebar
        tabs={openTabs}
        activeWorkflowId={activeWorkflowId}
        sidebarCollapsed={sidebarCollapsed}
        activityOpen={runPanelOpen}
        activityEnabled={canOpenActivity}
        activityRunning={activityRunning}
        activityEventCount={activityEventCount}
        onToggleSidebar={() => setSidebarOpen(sidebarCollapsed)}
        onToggleActivity={() => {
          if (runPanelOpen) closeRunPanel();
          else openRunPanel(null);
        }}
        onSelectTab={(id) => void selectWorkflow(id)}
        onCloseTab={(id) => void closeWorkflowTab(id)}
        onNewTab={() => void createWorkflow()}
        onRenameTab={(id, name) => setRenameTarget({ id, name })}
      />

      <div
        className={[
          "app-shell",
          sidebarCollapsed ? "sidebar-collapsed" : "",
          runPanelOpen ? "" : "run-collapsed",
        ]
          .filter(Boolean)
          .join(" ")}
      >
        <div className="sidebar-rail">
          <aside className="sidebar" aria-hidden={sidebarCollapsed}>
            {view === "settings" ? (
              <SettingsSidebar
                activeSection={settingsSection}
                onChange={setSettingsSection}
                onBack={() => setView("canvas")}
              />
            ) : (
              <div className="sidebar-scroll">
                <SidebarNav
                  view={view}
                  activityOpen={runPanelOpen && view === "canvas"}
                  activityEnabled={canOpenActivity}
                  memoriesOpen={memoriesOpen}
                  memoriesEnabled={Boolean(activeWorkflowId)}
                  onChange={setView}
                  onNewWorkflow={() => {
                    setView("canvas");
                    void createWorkflow();
                  }}
                  onToggleActivity={() => {
                    setView("canvas");
                    if (runPanelOpen) closeRunPanel();
                    else openRunPanel();
                  }}
                  onOpenMemories={() => {
                    setView("canvas");
                    setMemoriesFocusId(null);
                    setMemoriesOpen(true);
                  }}
                  onOpenConnectedApps={() => {
                    setSettingsSection("connected-apps");
                    setView("settings");
                  }}
                  onOpenSettings={() => {
                    setSettingsSection("general");
                    setView("settings");
                  }}
                />

                <div className="sidebar-header">
                  <h2>Workflows</h2>
                  <div className="sidebar-header-actions">
                    <button
                      type="button"
                      className="ghost sidebar-header-icon sidebar-folder-add"
                      title="New folder"
                      aria-label="New folder"
                      onClick={() => setFolderModal({ mode: "create" })}
                    >
                      <FolderAddGlyph />
                    </button>
                  </div>
                </div>
                <WorkflowList
                  workflows={workflows}
                  folders={workflowFolders}
                  activeWorkflowId={activeWorkflowId}
                  activeLiveNodes={nodes}
                  activeIsDirty={dirty}
                  schedules={workflowSchedules}
                  runningProviderByWorkflowId={runningProviderByWorkflowId}
                  onSelect={(id) => {
                    setView("canvas");
                    void selectWorkflow(id);
                  }}
                  onOpenMenu={({ id, name, x, y }) => {
                    setWorkflowMenu({ id, name, x, y });
                  }}
                  onOpenFolderMenu={({ id, name, x, y }) => {
                    setFolderMenu({ id, name, x, y });
                  }}
                  onMoveToFolder={(workflowId, folderId, beforeWorkflowId) =>
                    void moveWorkflowToFolder(
                      workflowId,
                      folderId,
                      beforeWorkflowId,
                    )
                  }
                />
              </div>
            )}

            <SidebarBottomBar />
          </aside>
        </div>

        {view === "settings" ? (
          <SettingsPage activeSection={settingsSection} />
        ) : view === "schedules" ? (
          <SchedulesPage
            key={schedulesTick}
            onClose={() => setView("canvas")}
            onOpenWorkflow={(id) => {
              setView("canvas");
              void selectWorkflow(id);
            }}
            onEditSchedule={(id, name) => {
              setScheduleTarget({ id, name });
            }}
          />
        ) : (
          <>
            <section className="canvas-pane">
              <header className="canvas-toolbar">
                <div className="toolbar-left">
                  <div>
                    <p className="status">
                      {workflowStatusLabel}
                    </p>
                  </div>
                  {error ? <span className="error">{error}</span> : null}
                </div>

                {activeWorkflowId ? (
                  <div className="toolbar-cwd">
                    <label
                      htmlFor="workflow-cwd-input"
                      className="toolbar-cwd-label"
                      title="Working directory — agent CLIs run in this folder"
                    >
                      <FolderGlyph />
                      <span>Folder</span>
                    </label>
                    <input
                      id="workflow-cwd-input"
                      type="text"
                      className="toolbar-cwd-input user-select-text"
                      spellCheck={false}
                      placeholder="/path/to/project"
                      defaultValue={activeWorkingDirectory}
                      key={`${activeWorkflowId}:${activeWorkingDirectory}`}
                      title={
                        activeWorkingDirectory || "Set a working directory"
                      }
                      onBlur={(e) => {
                        const next = e.target.value.trim();
                        if (next !== activeWorkingDirectory) {
                          void setWorkingDirectory(activeWorkflowId, next);
                        }
                      }}
                      onKeyDown={(e) => {
                        if (e.key === "Enter") {
                          (e.target as HTMLInputElement).blur();
                        }
                      }}
                    />
                    <button
                      type="button"
                      className="ghost toolbar-cwd-browse"
                      title="Choose folder"
                      aria-label="Choose working directory"
                      onClick={() => {
                        void chooseWorkingDirectory(activeWorkflowId);
                      }}
                    >
                      …
                    </button>
                  </div>
                ) : (
                  <div className="toolbar-cwd is-empty" aria-hidden />
                )}

                <div className="toolbar-right">
                  <button
                    type="button"
                    className="ghost"
                    disabled={!activeWorkflowId}
                    title="Inspect and organize memories"
                    onClick={() => {
                      setMemoriesFocusId(null);
                      setMemoriesOpen(true);
                    }}
                  >
                    Memories
                    {memories.length > 0 ? (
                      <span className="toolbar-count">{memories.length}</span>
                    ) : null}
                  </button>
                  <button
                    type="button"
                    className="ghost danger"
                    disabled={!activeWorkflowId || loading || workflowLoading}
                    onClick={() => {
                      if (activeWorkflowId) requestDelete(activeWorkflowId);
                    }}
                  >
                    Delete
                  </button>
                  <button
                    type="button"
                    className="ghost"
                    disabled={!activeWorkflowId || loading || workflowLoading}
                    onClick={() => void saveActiveWorkflow()}
                  >
                    Save
                  </button>
                  {activeRun?.status === "running" ? (
                    <button
                      type="button"
                      className="ghost danger"
                      onClick={() => void cancelActiveRun()}
                    >
                      Stop
                    </button>
                  ) : (
                    <button
                      type="button"
                      className="primary"
                      disabled={!activeWorkflowId || loading || workflowLoading}
                      onClick={() => void runActiveWorkflow()}
                    >
                      Run
                    </button>
                  )}
                </div>
              </header>

              <div className="canvas">
                <ReactFlowProvider>
                  <FlowEditor displayNodes={displayNodes} />
                </ReactFlowProvider>
                <div className="canvas-usage-dock">
                  <AgentUsageBar
                    workflowKey={activeWorkflowId ?? "no-workflow"}
                    providers={usedProviders}
                    usage={agentUsage}
                    refreshing={usageLoading}
                    onRefresh={() => void loadAgentUsage(usedProviders)}
                  />
                </div>
              </div>
            </section>

            {runPanelMounted ? <RunActivityPanel /> : null}
          </>
        )}
      </div>

      <OutputModal />
      <MemoriesInspector
        open={memoriesOpen}
        initialMemoryId={memoriesFocusId}
        onClose={() => {
          setMemoriesOpen(false);
          setMemoriesFocusId(null);
        }}
      />

      {workflowMenu ? (
        <WorkflowContextMenu
          key={`${workflowMenu.id}-${workflowMenu.x}-${workflowMenu.y}`}
          x={workflowMenu.x}
          y={workflowMenu.y}
          workflowName={workflowMenu.name}
          workflowFolderId={
            workflows.find((workflow) => workflow.id === workflowMenu.id)
              ?.folderId ?? null
          }
          folders={workflowFolders}
          running={
            workflowRunStates[workflowMenu.id]?.activeRun?.status === "running"
          }
          onClose={() => setWorkflowMenu(null)}
          onRun={() => {
            void runActiveWorkflow(workflowMenu.id);
          }}
          onStop={() => {
            void cancelActiveRun(workflowMenu.id);
          }}
          onRename={() => {
            setRenameTarget({
              id: workflowMenu.id,
              name: workflowMenu.name,
            });
          }}
          onEditFolder={() => {
            void chooseWorkingDirectory(workflowMenu.id);
          }}
          onMoveToFolder={(folderId) => {
            void moveWorkflowToFolder(workflowMenu.id, folderId);
          }}
          onSchedule={() => {
            void selectWorkflow(workflowMenu.id);
            setScheduleTarget({
              id: workflowMenu.id,
              name: workflowMenu.name,
            });
          }}
          onTriggers={() => {
            void selectWorkflow(workflowMenu.id);
            setTriggersTarget({
              id: workflowMenu.id,
              name: workflowMenu.name,
            });
          }}
          onDelete={() => {
            requestDelete(workflowMenu.id, workflowMenu.name);
          }}
        />
      ) : null}

      {folderMenu ? (
        <WorkflowFolderContextMenu
          key={`${folderMenu.id}-${folderMenu.x}-${folderMenu.y}`}
          x={folderMenu.x}
          y={folderMenu.y}
          folderName={folderMenu.name}
          onClose={() => setFolderMenu(null)}
          onCreateWorkflow={() => {
            setView("canvas");
            void createWorkflow("Untitled workflow", folderMenu.id);
          }}
          onRename={() => {
            setFolderModal({
              mode: "rename",
              id: folderMenu.id,
              name: folderMenu.name,
            });
          }}
          onDelete={() => {
            setDeleteFolderTarget({ id: folderMenu.id, name: folderMenu.name });
          }}
        />
      ) : null}

      {folderModal ? (
        <WorkflowFolderModal
          folder={
            folderModal.mode === "rename"
              ? { id: folderModal.id, name: folderModal.name }
              : null
          }
          onClose={() => setFolderModal(null)}
        />
      ) : null}

      {renameTarget ? (
        <RenameWorkflowModal
          workflowId={renameTarget.id}
          workflowName={renameTarget.name}
          onClose={() => setRenameTarget(null)}
        />
      ) : null}

      {triggersTarget ? (
        <TriggersModal
          workflowId={triggersTarget.id}
          workflowName={triggersTarget.name}
          onClose={() => setTriggersTarget(null)}
        />
      ) : null}

      {scheduleTarget ? (
        <ScheduleModal
          workflowId={scheduleTarget.id}
          workflowName={scheduleTarget.name}
          onClose={() => {
            setScheduleTarget(null);
            if (view === "schedules") {
              setSchedulesTick((n) => n + 1);
            }
          }}
        />
      ) : null}

      {deleteTarget ? (
        <ConfirmDialog
          title={`Delete “${deleteTarget.name}”?`}
          message="This permanently removes the workflow and its schedule. This cannot be undone."
          confirmLabel="Delete workflow"
          danger
          onCancel={() => setDeleteTarget(null)}
          onConfirm={() => {
            const id = deleteTarget.id;
            setDeleteTarget(null);
            void deleteWorkflow(id);
          }}
        />
      ) : null}

      {deleteFolderTarget ? (
        <ConfirmDialog
          title={`Delete “${deleteFolderTarget.name}”?`}
          message="The folder will be removed. Its workflows will stay available under Unfiled."
          confirmLabel="Delete folder"
          danger
          onCancel={() => setDeleteFolderTarget(null)}
          onConfirm={() => {
            const id = deleteFolderTarget.id;
            setDeleteFolderTarget(null);
            void deleteWorkflowFolder(id);
          }}
        />
      ) : null}
    </div>
  );
}
