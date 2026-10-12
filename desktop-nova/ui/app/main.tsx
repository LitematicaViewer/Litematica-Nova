import React from "react";
import ReactDOM from "react-dom/client";

import { App } from "../shell/AppShell";
import "../styles/base.css";
import { loadUserConfigMigratingLocalStorage } from "../../src/business/facade";
import { initializeWindowTheme } from "../shell/themeRuntime";

initializeWindowTheme(loadUserConfigMigratingLocalStorage, true).then(() => {
  ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
    <React.StrictMode>
      <App />
    </React.StrictMode>,
  );
});
