import { initializeUiLanguage } from "../../../src/services/i18n";
import React from "react";
import ReactDOM from "react-dom/client";

import { loadDatabases, initI18n, getUserConfig, displayPathText } from "../../../src/business/facade";
import "../../styles/base.css";
import { initializeWindowTheme } from "../../shell/themeRuntime";
import { MaterialListWindow } from ".";

async function bootstrap() {
  await initializeUiLanguage(getUserConfig);
    loadDatabases();
    await Promise.all([initI18n(), initializeWindowTheme(getUserConfig)]);
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
                <pre className="material-list-window-state material-list-window-error">{displayPathText(String(error))}</pre>
            </main>
        </React.StrictMode>
    );
});
