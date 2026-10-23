import React from "react";
import ReactDOM from "react-dom/client";
import { initializeUiLanguage } from "../../../src/services/i18n";
import { getUserConfig } from "../../../src/business/facade";
import { initializeWindowTheme } from "../../shell/themeRuntime";
import { IrisOptionsWindow } from "./IrisOptionsWindow";
import "../../styles/base.css";
import "./iris-options.css";
async function bootstrap() {
  await Promise.all([initializeUiLanguage(getUserConfig), initializeWindowTheme(getUserConfig)]);
  ReactDOM.createRoot(document.getElementById("root")!).render(<React.StrictMode><IrisOptionsWindow /></React.StrictMode>);
}
void bootstrap().catch(error => { ReactDOM.createRoot(document.getElementById("root")!).render(<pre role="alert">{String(error)}</pre>); });
