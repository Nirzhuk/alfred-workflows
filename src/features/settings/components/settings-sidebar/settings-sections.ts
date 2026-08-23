export type SettingsSectionId =
  | "general"
  | "quick-access"
  | "shortcuts"
  | "notifications"
  | "license-billing"
  | "connected-apps"
  | "data";

export const SETTINGS_SECTION_LABELS: Record<SettingsSectionId, string> = {
  general: "General",
  "quick-access": "Quick Access",
  shortcuts: "Keyboard shortcuts",
  notifications: "Notifications",
  "license-billing": "License & Billing",
  "connected-apps": "Connected apps",
  data: "Data & storage",
};
