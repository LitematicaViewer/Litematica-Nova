import React from "react";
import ReactDOM from "react-dom/client";

import "../../styles/base.css";
import { LocalLibraryFoldersWindow } from ".";

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    <LocalLibraryFoldersWindow />
  </React.StrictMode>,
);