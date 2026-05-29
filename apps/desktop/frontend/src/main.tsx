import React from "react";
import ReactDOM from "react-dom/client";
import "./styles.css";
import { App } from "./ui/App";
import { PreviewWindow } from "./ui/PreviewWindow";

const root = document.getElementById("root");
if (root === null) {
  throw new Error("root element is missing");
}

ReactDOM.createRoot(root).render(
  <React.StrictMode>
    {new URLSearchParams(window.location.search).get("view") === "preview" ? <PreviewWindow /> : <App />}
  </React.StrictMode>
);
