import { createRoot } from "react-dom/client";
import { ErrorBoundary } from "./ErrorBoundary";
import { WorkbenchShell } from "./WorkbenchShell";
import "dockview-react/dist/styles/dockview.css";
import "./styles.css";

createRoot(document.getElementById("root")!).render(
  <ErrorBoundary>
    <WorkbenchShell />
  </ErrorBoundary>
);
