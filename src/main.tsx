import React from "react";
import ReactDOM from "react-dom/client";
import { getCurrentWindow } from "@tauri-apps/api/window";
import {
  bootstrapTheme,
  enableThemeTransitions,
} from "./features/settings/theme";
import { applyDesktopPlatform } from "./platform";

applyDesktopPlatform(document.documentElement);
bootstrapTheme();

const isQuickAccessWindow = getCurrentWindow().label === "quick-access";
document.documentElement.dataset.window = isQuickAccessWindow
  ? "quick-access"
  : "main";

const WindowRoot = isQuickAccessWindow
  ? React.lazy(async () => ({
      default: (await import("./features/quick-access/quick-access-popover"))
        .QuickAccessPopover,
    }))
  : React.lazy(() => import("./App"));

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    <React.Suspense fallback={null}>
      <WindowRoot />
    </React.Suspense>
  </React.StrictMode>,
);

enableThemeTransitions();
