import React from "react";
import { initializeUiLanguage, initI18n } from "../../src/services/i18n";
import ReactDOM from "react-dom/client";

import { App } from "../shell/AppShell";
import "../styles/base.css";
import { loadUserConfigMigratingLocalStorage } from "../../src/business/facade";
import { initializeWindowTheme } from "../shell/themeRuntime";

// Paint the cached/default shell immediately; disk config hydration is asynchronous.
async function bootstrap() {
await initializeUiLanguage(loadUserConfigMigratingLocalStorage);
await Promise.all([initializeWindowTheme(loadUserConfigMigratingLocalStorage, true), initI18n()]);
  ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
    <React.StrictMode>
      <App />
    </React.StrictMode>,
  );
}
void bootstrap().catch(console.error);
