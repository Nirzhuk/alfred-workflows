export type SettingsSectionId =
  | "general"
  | "quick-access"
  | "shortcuts"
  | "appearance"
  | "notifications"
  | "connected-apps"
  | "data";

export const SETTINGS_SECTION_LABELS: Record<SettingsSectionId, string> = {
  general: "General",
  "quick-access": "Quick Access",
  shortcuts: "Keyboard shortcuts",
  appearance: "Appearance",
  notifications: "Notifications",
  "connected-apps": "Connected apps",
  data: "Data & storage",
};
