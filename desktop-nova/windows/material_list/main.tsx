import React from "react";
import ReactDOM from "react-dom/client";

import "../../src/styles/base.css";
import "../../shell/theme.css";
import { MaterialListWindow } from ".";

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
    <React.StrictMode>
        <MaterialListWindow />
    </React.StrictMode>
);
