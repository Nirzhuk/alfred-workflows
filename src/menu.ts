import { Menu, MenuItem, PredefinedMenuItem, Submenu } from "@tauri-apps/api/menu";

export type MenuActions = {
  onOpenSettings: () => void;
  onOpenSchedules: () => void;
  onNewWorkflow: () => void;
  onSaveWorkflow: () => void;
  onRenameWorkflow: () => void;
  onRunWorkflow: () => void;
  onDeleteWorkflow: () => void;
  onScheduleWorkflow: () => void;
  onToggleSidebar: () => void;
  onToggleActivity: () => void;
  onFitCanvas: () => void;
  onCheckUpdates: () => void;
  onShowShortcuts: () => void;
};

/** Native macOS / Windows / Linux application menu. */
export async function installAppMenu(actions: MenuActions): Promise<void> {
  const about = await Submenu.new({
    text: "Agentflow",
    items: [
      await PredefinedMenuItem.new({
        text: "About Agentflow",
        item: {
          About: {
            name: "Agentflow",
            version: "0.1.0",
            copyright: "Local multi-agent workflow automations",
            comments:
              "Build automations across Claude Code, Cursor, Codex, and OpenCode.",
          },
        },
      }),
      await MenuItem.new({
        id: "app-settings",
        text: "Settings…",
        accelerator: "CmdOrCtrl+,",
        action: () => actions.onOpenSettings(),
      }),
      await PredefinedMenuItem.new({ item: "Separator" }),
      await PredefinedMenuItem.new({ item: "Services" }),
      await PredefinedMenuItem.new({ item: "Separator" }),
      await PredefinedMenuItem.new({ item: "Hide", text: "Hide Agentflow" }),
      await PredefinedMenuItem.new({ item: "HideOthers" }),
      await PredefinedMenuItem.new({ item: "ShowAll" }),
      await PredefinedMenuItem.new({ item: "Separator" }),
      await PredefinedMenuItem.new({ item: "Quit", text: "Quit Agentflow" }),
    ],
  });

  const workflow = await Submenu.new({
    text: "Workflow",
    items: [
      await MenuItem.new({
        id: "workflow-new",
        text: "New Workflow",
        accelerator: "CmdOrCtrl+N",
        action: () => actions.onNewWorkflow(),
      }),
      await MenuItem.new({
        id: "workflow-save",
        text: "Save",
        accelerator: "CmdOrCtrl+S",
        action: () => actions.onSaveWorkflow(),
      }),
      await MenuItem.new({
        id: "workflow-rename",
        text: "Rename Workflow…",
        action: () => actions.onRenameWorkflow(),
      }),
      await MenuItem.new({
        id: "workflow-delete",
        text: "Delete Workflow",
        accelerator: "CmdOrCtrl+Backspace",
        action: () => actions.onDeleteWorkflow(),
      }),
      await PredefinedMenuItem.new({ item: "Separator" }),
      await MenuItem.new({
        id: "workflow-run",
        text: "Run",
        accelerator: "CmdOrCtrl+R",
        action: () => actions.onRunWorkflow(),
      }),
      await PredefinedMenuItem.new({ item: "Separator" }),
      await MenuItem.new({
        id: "workflow-schedule",
        text: "Schedule…",
        action: () => actions.onScheduleWorkflow(),
      }),
      await MenuItem.new({
        id: "workflow-schedules",
        text: "All Schedules…",
        action: () => actions.onOpenSchedules(),
      }),
    ],
  });

  const edit = await Submenu.new({
    text: "Edit",
    items: [
      await PredefinedMenuItem.new({ item: "Undo" }),
      await PredefinedMenuItem.new({ item: "Redo" }),
      await PredefinedMenuItem.new({ item: "Separator" }),
      await PredefinedMenuItem.new({ item: "Cut" }),
      await PredefinedMenuItem.new({ item: "Copy" }),
      await PredefinedMenuItem.new({ item: "Paste" }),
      await PredefinedMenuItem.new({ item: "SelectAll" }),
    ],
  });

  const view = await Submenu.new({
    text: "View",
    items: [
      await MenuItem.new({
        id: "view-toggle-sidebar",
        text: "Toggle Sidebar",
        action: () => actions.onToggleSidebar(),
      }),
      await MenuItem.new({
        id: "view-toggle-activity",
        text: "Toggle Activity Panel",
        action: () => actions.onToggleActivity(),
      }),
      await MenuItem.new({
        id: "view-fit-canvas",
        text: "Fit Workflow to Canvas",
        action: () => actions.onFitCanvas(),
      }),
      await PredefinedMenuItem.new({ item: "Separator" }),
      await PredefinedMenuItem.new({ item: "Fullscreen" }),
    ],
  });

  const windowMenu = await Submenu.new({
    text: "Window",
    items: [
      await PredefinedMenuItem.new({ item: "Minimize" }),
      await PredefinedMenuItem.new({ item: "Maximize" }),
      await PredefinedMenuItem.new({ item: "Separator" }),
      await PredefinedMenuItem.new({ item: "CloseWindow" }),
    ],
  });

  const help = await Submenu.new({
    text: "Help",
    items: [
      await MenuItem.new({
        id: "help-about-app",
        text: "What is Agentflow?",
        action: () => {
          window.alert(
            "Agentflow builds local automations across Claude Code, Cursor, Codex, and OpenCode — prompts, skills, models, and runnable workflows on your machine.",
          );
        },
      }),
      await MenuItem.new({
        id: "help-shortcuts",
        text: "Keyboard Shortcuts…",
        action: () => actions.onShowShortcuts(),
      }),
      await PredefinedMenuItem.new({ item: "Separator" }),
      await MenuItem.new({
        id: "help-check-updates",
        text: "Check for Updates…",
        action: () => actions.onCheckUpdates(),
      }),
    ],
  });

  const menu = await Menu.new({
    items: [about, workflow, edit, view, windowMenu, help],
  });

  await menu.setAsAppMenu();
}
