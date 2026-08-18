export type SettingsSectionId =
  | "general"
  | "quick-access"
  | "shortcuts"
  | "notifications"
  | "connected-apps"
  | "data";

export const SETTINGS_SECTION_LABELS: Record<SettingsSectionId, string> = {
  general: "General",
  "quick-access": "Quick Access",
  shortcuts: "Keyboard shortcuts",
  notifications: "Notifications",
  "connected-apps": "Connected apps",
  data: "Data & storage",
};
