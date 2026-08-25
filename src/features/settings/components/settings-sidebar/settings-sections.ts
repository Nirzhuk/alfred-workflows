export type SettingsSectionId =
  | "general"
  | "quick-access"
  | "shortcuts"
  | "notifications"
  | "license-billing"
  | "native-agents"
  | "connected-apps"
  | "data";

export const SETTINGS_SECTION_LABELS: Record<SettingsSectionId, string> = {
  general: "General",
  "quick-access": "Quick Access",
  shortcuts: "Keyboard shortcuts",
  notifications: "Notifications",
  "license-billing": "License & Billing",
  "native-agents": "Native Agents",
  "connected-apps": "Connected apps",
  data: "Data & storage",
};
