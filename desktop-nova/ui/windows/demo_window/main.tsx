import ReactDOM from "react-dom/client";
import React from "react";

import "../../styles/base.css";
import { getUserConfig } from "../../../src/business/facade";
import { initializeWindowTheme } from "../../shell/themeRuntime";
import { DemoWindow } from ".";

initializeWindowTheme(getUserConfig).then(() => {
  ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
      <React.StrictMode>
          <DemoWindow />
      </React.StrictMode>
  );
});
