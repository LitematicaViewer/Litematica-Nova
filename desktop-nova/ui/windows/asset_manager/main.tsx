import React from "react";
import ReactDOM from "react-dom/client";

import { AssetManagerWindow } from ".";
import "../../styles/base.css";

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    <AssetManagerWindow />
  </React.StrictMode>,
);
