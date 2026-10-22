import { initializeUiLanguage } from "../../../src/services/i18n";
import React from "react";
import ReactDOM from "react-dom/client";

import "../../styles/base.css";
import { getUserConfig } from "../../../src/business/facade";
import { initializeWindowTheme } from "../../shell/themeRuntime";
import { LocalLibraryFoldersWindow } from ".";

initializeUiLanguage(getUserConfig).then(() => initializeWindowTheme(getUserConfig)).then(() => {
  ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
    <React.StrictMode>
      <LocalLibraryFoldersWindow />
    </React.StrictMode>,
  );
});
