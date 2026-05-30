import { AlertTriangle, CheckCircle2 } from "lucide-react";
import type { AppSnapshotDto } from "../bindings";
import { openActiveEditorDiagnostics } from "../uiEvents";

export function StatusBar({ snapshot }: { snapshot: AppSnapshotDto }) {
  const errors = snapshot.diagnostics.filter((diagnostic) => diagnostic.severity === "error").length;
  const warnings = snapshot.diagnostics.filter((diagnostic) => diagnostic.severity === "warning").length;
  return (
    <footer className="status-bar">
      <span>{snapshot.status}</span>
      <span>{snapshot.projectRoot ?? "No project"}</span>
      <button
        type="button"
        className={`status-diagnostics-button ${errors > 0 ? "status-problem" : "status-ok"}`}
        onClick={openActiveEditorDiagnostics}
      >
        {errors > 0 ? <AlertTriangle size={14} /> : <CheckCircle2 size={14} />}
        {errors} errors
      </button>
      <button type="button" className="status-diagnostics-button status-warning" onClick={openActiveEditorDiagnostics}>
        {warnings} warnings
      </button>
    </footer>
  );
}
