import React from "react";
import ReactDOM from "react-dom/client";

import "../../styles/base.css";
import { getUserConfig } from "../../../src/business/facade";
import { initializeWindowTheme } from "../../shell/themeRuntime";
import { RedenLibraryWindow } from ".";

initializeWindowTheme(getUserConfig).then(() => {
  ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
    <React.StrictMode>
      <RedenLibraryWindow />
    </React.StrictMode>,
  );
});
