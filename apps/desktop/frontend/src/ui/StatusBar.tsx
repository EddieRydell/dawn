import { AlertTriangle, CheckCircle2 } from "lucide-react";
import type { AppSnapshotDto } from "../bindings";

export function StatusBar({ snapshot }: { snapshot: AppSnapshotDto }) {
  const errors = snapshot.diagnostics.filter((diagnostic) => diagnostic.severity === "error").length;
  const warnings = snapshot.diagnostics.filter((diagnostic) => diagnostic.severity === "warning").length;
  return (
    <footer className="status-bar">
      <span>{snapshot.status}</span>
      <span>{snapshot.projectRoot ?? "No project"}</span>
      <span className={`status-diagnostics-button ${errors > 0 ? "status-problem" : "status-ok"}`}>
        {errors > 0 ? <AlertTriangle size={14} /> : <CheckCircle2 size={14} />}
        {errors} errors
      </span>
      <span className="status-diagnostics-button status-warning">{warnings} warnings</span>
    </footer>
  );
}
