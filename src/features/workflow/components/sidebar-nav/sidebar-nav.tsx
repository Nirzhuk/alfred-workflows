import type { ReactNode } from "react";

export type SidebarView = "canvas" | "schedules" | "settings";

type Props = {
  view: SidebarView;
  onChange: (view: SidebarView) => void;
};

function ClockIcon() {
  return (
    <svg width="14" height="14" viewBox="0 0 16 16" fill="none" aria-hidden>
      <circle cx="8" cy="8" r="5.25" stroke="currentColor" strokeWidth="1.35" />
      <path
        d="M8 5.2V8l1.8 1.2"
        stroke="currentColor"
        strokeWidth="1.35"
        strokeLinecap="round"
      />
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

const ITEMS: Array<{
  id: SidebarView;
  label: string;
  icon: ReactNode;
}> = [
  { id: "schedules", label: "Schedules", icon: <ClockIcon /> },
  { id: "settings", label: "Settings", icon: <SettingsIcon /> },
];

export function SidebarNav({ view, onChange }: Props) {
  return (
    <nav className="sidebar-nav" aria-label="App sections">
      {ITEMS.map((item) => {
        const active = view === item.id;
        return (
          <button
            key={item.id}
            type="button"
            className={["sidebar-nav-item", active ? "is-active" : ""]
              .filter(Boolean)
              .join(" ")}
            aria-current={active ? "page" : undefined}
            onClick={() => onChange(active ? "canvas" : item.id)}
          >
            <span className="sidebar-nav-icon">{item.icon}</span>
            {item.label}
          </button>
        );
      })}
    </nav>
  );
}
