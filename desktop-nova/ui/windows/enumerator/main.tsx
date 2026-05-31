import React from "react";
import ReactDOM from "react-dom/client";

import { initI18n, loadDatabases } from "../../../src/business/facade";
import "../../styles/base.css";
import { EnumeratorWindow } from ".";

async function bootstrap() {
  loadDatabases();
  await initI18n();
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