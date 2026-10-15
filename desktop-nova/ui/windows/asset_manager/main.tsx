import React from "react";
import ReactDOM from "react-dom/client";

import { loadDatabases, initI18n, getUserConfig } from "../../../src/business/facade";
import "../../styles/base.css";
import "../../styles/asset-manager.css";
import { initializeWindowTheme } from "../../shell/themeRuntime";
import { AssetManagerWindow } from ".";

async function bootstrap() {
  void loadDatabases().catch((error) => console.warn("Game data:", error));
  await Promise.all([initI18n(), initializeWindowTheme(getUserConfig)]);
  ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
    <React.StrictMode>
      <AssetManagerWindow />
    </React.StrictMode>,
  );
}

bootstrap().catch((error) => {
  console.error("Failed to bootstrap asset manager window", error);
  ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
    <React.StrictMode>
      <main className="asset-manager-window">
        <pre className="material-list-window-state material-list-window-error">{String(error)}</pre>
      </main>
    </React.StrictMode>,
  );
});
