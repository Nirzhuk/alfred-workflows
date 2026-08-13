import React from "react";
import ReactDOM from "react-dom/client";
import App from "./App";
import {
  bootstrapTheme,
  enableThemeTransitions,
} from "./features/settings/theme";

bootstrapTheme();

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    <App />
  </React.StrictMode>,
);

enableThemeTransitions();
