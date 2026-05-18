import ReactDOM from "react-dom/client";
import React from "react";

import "../../styles/base.css";
import { DemoWindow } from ".";

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
    <React.StrictMode>
        <DemoWindow />
    </React.StrictMode>
);