import React from "react";
import ReactDOM from "react-dom/client";

import { App } from "../shell/AppShell";
import "../styles/base.css";
import { loadUserConfigMigratingLocalStorage } from "../../src/business/facade";
import { initializeWindowTheme } from "../shell/themeRuntime";

// Paint the cached/default shell immediately; disk config hydration is asynchronous.
void initializeWindowTheme(loadUserConfigMigratingLocalStorage, true);
  ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
    <React.StrictMode>
      <App />
    </React.StrictMode>,
  );
