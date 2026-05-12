import React from "react";
import ReactDOM from "react-dom/client";

import { loadDatabases, initI18n } from "../../../src/business/facade";
import "../../styles/base.css";
import { MaterialListWindow } from ".";

async function bootstrap() {
    loadDatabases();
    await initI18n();
    ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
        <React.StrictMode>
            <MaterialListWindow />
        </React.StrictMode>
    );
}

bootstrap().catch((error) => {
    console.error("Failed to bootstrap material list window", error);
    ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
        <React.StrictMode>
            <main className="material-list-window">
                <pre className="material-list-window-state material-list-window-error">{String(error)}</pre>
            </main>
        </React.StrictMode>
    );
});
