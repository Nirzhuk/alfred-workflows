import { listen } from "@tauri-apps/api/event";
import { isTauri } from "@tauri-apps/api/core";
import { useEffect } from "react";
import { ToastViewport } from "./components/toast";
import { syncQuickAccessPreference } from "./features/quick-access/preferences";
import { installThemeListeners } from "./features/settings/theme";
import type { OpenRunOutputPayload } from "./features/settings/notifications";
import {
  getShortcutHelpLines,
  syncShortcutPreferences,
  useShortcutPreferences,
} from "./features/settings/shortcuts";
import { WorkflowCanvas } from "./features/workflow/components/workflow-canvas";
import { NativeHostApproval } from "./features/agent-accounts/components/native-host-approval";
import {
  installRunEventBridge,
  useWorkflowStore,
} from "./features/workflow/store";
import { installAppMenu } from "./menu";
import { prepareNotifications } from "./native";
import "./App.css";

function App() {
  const loadWorkflows = useWorkflowStore((s) => s.loadWorkflows);
  const createWorkflow = useWorkflowStore((s) => s.createWorkflow);
  const saveActiveWorkflow = useWorkflowStore((s) => s.saveActiveWorkflow);
  const runActiveWorkflow = useWorkflowStore((s) => s.runActiveWorkflow);
  const shortcuts = useShortcutPreferences((s) => s.shortcuts);

  useEffect(() => {
    if (!isTauri()) return;
    void loadWorkflows();
    void prepareNotifications();
    void installRunEventBridge();
    void syncQuickAccessPreference();
    void syncShortcutPreferences();
  }, [loadWorkflows]);

  useEffect(() => installThemeListeners(), []);

  useEffect(() => {
    if (!isTauri()) return;
    const unsubs: Array<() => void> = [];
    void listen("app://open-settings", () => {
      window.dispatchEvent(new Event("alfred:open-settings"));
    }).then((u) => unsubs.push(u));
    void listen("app://open-schedules", () => {
      window.dispatchEvent(new Event("alfred:open-schedules"));
    }).then((u) => unsubs.push(u));
    void listen("app://download-latest", () => {
      window.dispatchEvent(new Event("alfred:download-latest"));
    }).then((u) => unsubs.push(u));
    void listen<string>("app://open-workflow", (event) => {
      window.dispatchEvent(
        new CustomEvent("alfred:open-workflow", {
          detail: { workflowId: event.payload },
        }),
      );
    }).then((u) => unsubs.push(u));
    void listen("app://open-activity", () => {
      window.dispatchEvent(new Event("alfred:open-activity"));
    }).then((u) => unsubs.push(u));
    void listen<OpenRunOutputPayload>("app://open-run-output", (event) => {
      window.dispatchEvent(
        new CustomEvent("alfred:open-run-output", {
          detail: event.payload,
        }),
      );
    }).then((u) => unsubs.push(u));
    return () => {
      for (const u of unsubs) u();
    };
  }, []);

  useEffect(() => {
    if (!isTauri()) return;
    void installAppMenu(
      {
        onOpenSettings: () => {
          window.dispatchEvent(new Event("alfred:open-settings"));
        },
        onOpenSchedules: () => {
          window.dispatchEvent(new Event("alfred:open-schedules"));
        },
        onNewWorkflow: () => void createWorkflow(),
        onSaveWorkflow: () => void saveActiveWorkflow(),
        onRenameWorkflow: () => {
          window.dispatchEvent(new Event("alfred:rename-workflow"));
        },
        onRunWorkflow: () => void runActiveWorkflow(),
        onDeleteWorkflow: () => {
          window.dispatchEvent(new Event("alfred:delete-workflow"));
        },
        onScheduleWorkflow: () => {
          window.dispatchEvent(new Event("alfred:open-schedule"));
        },
        onToggleSidebar: () => {
          window.dispatchEvent(new Event("alfred:toggle-sidebar"));
        },
        onToggleActivity: () => {
          window.dispatchEvent(new Event("alfred:toggle-activity"));
        },
        onFitCanvas: () => {
          window.dispatchEvent(new Event("alfred:fit-canvas"));
        },
        onDownloadLatest: () => {
          window.dispatchEvent(new Event("alfred:download-latest"));
        },
        onShowShortcuts: () => {
          window.alert(
            [
              "Alfred keyboard shortcuts",
              "",
              ...getShortcutHelpLines(shortcuts),
            ].join("\n"),
          );
        },
      },
      shortcuts,
    ).catch((err) => {
      console.warn("Native menu unavailable", err);
    });
  }, [createWorkflow, saveActiveWorkflow, runActiveWorkflow, shortcuts]);

  useEffect(() => {
    const isEditable = (target: EventTarget | null) => {
      const el = target as HTMLElement | null;
      if (!el?.closest) return false;
      return Boolean(
        el.closest(
          'input, textarea, select, [contenteditable="true"], .user-select-text, .output-modal-body',
        ),
      );
    };

    // Kill browser-y selection gestures outside editable/output surfaces.
    const onSelectStart = (e: Event) => {
      if (!isEditable(e.target)) e.preventDefault();
    };

    // Avoid accidental image/text drags looking like a webpage.
    const onDragStart = (e: DragEvent) => {
      const el = e.target as HTMLElement | null;
      if (!el?.closest) return;
      if (isEditable(e.target)) return;
      if (el.closest(".workflow-card-grip")) return;
      if (el.closest(".react-flow__node, .react-flow__edge, .react-flow__handle")) {
        return;
      }
      e.preventDefault();
    };

    // Disable the browser context menu on chrome; canvas uses its own menu.
    const onContextMenu = (e: MouseEvent) => {
      if (isEditable(e.target)) return;
      e.preventDefault();
    };

    document.addEventListener("selectstart", onSelectStart);
    document.addEventListener("dragstart", onDragStart);
    document.addEventListener("contextmenu", onContextMenu);
    return () => {
      document.removeEventListener("selectstart", onSelectStart);
      document.removeEventListener("dragstart", onDragStart);
      document.removeEventListener("contextmenu", onContextMenu);
    };
  }, []);

  return (
    <>
      <WorkflowCanvas />
      <NativeHostApproval />
      <ToastViewport />
    </>
  );
}

export default App;
