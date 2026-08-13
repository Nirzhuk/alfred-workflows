import { useState } from "react";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuTrigger,
  MenuItem,
  MenuLabel,
  MenuSeparator,
  useDropdownMenuClose,
} from "../../../../components/menu";

type Props = {
  activityOpen: boolean;
  settingsOpen: boolean;
  onToggleActivity: () => void;
  onOpenMemories?: () => void;
  onOpenSettings: () => void;
};

function MemoriesIcon() {
  return (
    <svg width="14" height="14" viewBox="0 0 16 16" fill="none" aria-hidden>
      <path
        d="M3.5 3.5h9v10.5l-2.25-1.5L8 14l-2.25-1.5L3.5 14V3.5Z"
        stroke="currentColor"
        strokeWidth="1.35"
        strokeLinejoin="round"
      />
    </svg>
  );
}

function ActivityIcon() {
  return (
    <svg width="14" height="14" viewBox="0 0 16 16" fill="none" aria-hidden>
      <path
        d="M2.5 12.5V9.5M6.5 12.5V5.5M10.5 12.5V7.5M13.5 12.5V3.5"
        stroke="currentColor"
        strokeWidth="1.5"
        strokeLinecap="round"
      />
    </svg>
  );
}

function HelpIcon() {
  return (
    <svg width="14" height="14" viewBox="0 0 16 16" fill="none" aria-hidden>
      <circle cx="8" cy="8" r="5.75" stroke="currentColor" strokeWidth="1.35" />
      <path
        d="M6.4 6.2a1.7 1.7 0 0 1 3.25.7c0 1.15-1.65 1.45-1.65 2.4"
        stroke="currentColor"
        strokeWidth="1.35"
        strokeLinecap="round"
      />
      <circle cx="8" cy="11.35" r="0.65" fill="currentColor" />
    </svg>
  );
}

function SettingsIcon() {
  return (
    <svg width="14" height="14" viewBox="0 0 16 16" fill="none" aria-hidden>
      <path
        d="M6.48 1.75h3.04l.32 1.72c.44.15.84.38 1.2.66l1.66-.6 1.52 2.64-1.34 1.12c.07.38.11.78.11 1.19 0 .41-.04.81-.11 1.19l1.34 1.12-1.52 2.64-1.66-.6a4.8 4.8 0 0 1-1.2.66l-.32 1.72H6.48l-.32-1.72a4.8 4.8 0 0 1-1.2-.66l-1.66.6-1.52-2.64 1.34-1.12A5 5 0 0 1 2.9 8.48c0-.41.04-.81.11-1.19L1.67 6.17l1.52-2.64 1.66.6c.36-.28.76-.51 1.2-.66l.32-1.72Z"
        stroke="currentColor"
        strokeWidth="1.3"
        strokeLinejoin="round"
      />
      <circle cx="8" cy="8" r="2" stroke="currentColor" strokeWidth="1.3" />
    </svg>
  );
}

function HelpMenuItems() {
  const close = useDropdownMenuClose();

  return (
    <>
      <MenuLabel>Help</MenuLabel>
      <MenuItem
        onSelect={() => {
          close();
          window.alert(
            [
              "Keyboard shortcuts",
              "",
              "⌘N — New workflow",
              "⌘S — Save workflow",
              "⌘R — Run workflow",
              "Right-click workflow — Actions",
              "Add step (canvas) — New node menu",
              "Right-click canvas — Add step",
            ].join("\n"),
          );
        }}
      >
        Keyboard shortcuts
      </MenuItem>
      <MenuItem
        onSelect={() => {
          close();
          window.alert(
            "Agentflow runs local coding agents as visual workflow automations. Workflows, memories, and runs stay on this machine.",
          );
        }}
      >
        About Agentflow
      </MenuItem>
      <MenuSeparator />
      <MenuItem
        onSelect={() => {
          close();
          window.dispatchEvent(new Event("agentflow:open-schedule"));
        }}
      >
        Schedule current workflow…
      </MenuItem>
    </>
  );
}

export function SidebarBottomBar({
  activityOpen,
  settingsOpen,
  onToggleActivity,
  onOpenMemories,
  onOpenSettings,
}: Props) {
  const [helpOpen, setHelpOpen] = useState(false);

  return (
    <div className="sidebar-bottom-bar">
      <div className="sidebar-bottom-actions sidebar-bottom-actions-left">
        <button
          type="button"
          className={[
            "sidebar-bottom-icon",
            settingsOpen ? "is-active" : "",
          ]
            .filter(Boolean)
            .join(" ")}
          title="Settings"
          aria-label="Settings"
          aria-pressed={settingsOpen}
          onClick={onOpenSettings}
        >
          <SettingsIcon />
        </button>

        <DropdownMenu
          className="sidebar-help"
          open={helpOpen}
          onOpenChange={setHelpOpen}
        >
          <DropdownMenuTrigger
            className={[
              "sidebar-bottom-icon",
              helpOpen ? "is-active" : "",
            ]
              .filter(Boolean)
              .join(" ")}
            title="Help"
            aria-label="Help"
          >
            <HelpIcon />
          </DropdownMenuTrigger>
          <DropdownMenuContent align="start" side="top" aria-label="Help">
            <HelpMenuItems />
          </DropdownMenuContent>
        </DropdownMenu>
      </div>

      <div className="sidebar-bottom-actions">
        {onOpenMemories ? (
          <button
            type="button"
            className="sidebar-bottom-icon"
            title="Memories"
            aria-label="Memories"
            onClick={onOpenMemories}
          >
            <MemoriesIcon />
          </button>
        ) : null}
        <button
          type="button"
          className={[
            "sidebar-bottom-icon",
            activityOpen ? "is-active" : "",
          ]
            .filter(Boolean)
            .join(" ")}
          title={activityOpen ? "Hide activity" : "Show activity"}
          aria-label={activityOpen ? "Hide activity" : "Show activity"}
          aria-pressed={activityOpen}
          onClick={onToggleActivity}
        >
          <ActivityIcon />
        </button>
      </div>
    </div>
  );
}
