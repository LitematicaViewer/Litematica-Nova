import React from "react";
import ReactDOM from "react-dom/client";

import { initI18n, loadDatabases, getUserConfig } from "../../../src/business/facade";
import "../../styles/base.css";
import "../../styles/enumerator.css";
import { initializeWindowTheme } from "../../shell/themeRuntime";
import { EnumeratorWindow } from ".";

async function bootstrap() {
  loadDatabases();
  await Promise.all([initI18n(), initializeWindowTheme(getUserConfig)]);
  ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
    <React.StrictMode>
      <EnumeratorWindow />
    </React.StrictMode>,
  );
}

bootstrap().catch((error) => {
  console.error("Failed to bootstrap enumerator window", error);
  ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
    <React.StrictMode>
      <main className="enumerator-window">
        <pre className="material-list-window-state material-list-window-error">{String(error)}</pre>
      </main>
    </React.StrictMode>,
  );
});