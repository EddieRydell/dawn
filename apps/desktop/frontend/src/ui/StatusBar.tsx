import { AlertTriangle, CheckCircle2 } from "lucide-react";
import { AppSnapshotDto } from "../bindings";

export function StatusBar({ snapshot }: { snapshot: AppSnapshotDto }) {
  const errors = snapshot.diagnostics.filter((diagnostic) => diagnostic.severity === "error").length;
  const warnings = snapshot.diagnostics.filter((diagnostic) => diagnostic.severity === "warning").length;
  return (
    <footer className="status-bar">
      <span>{snapshot.status}</span>
      <span>{snapshot.projectRoot ?? "No project"}</span>
      <span className={errors ? "status-problem" : "status-ok"}>
        {errors ? <AlertTriangle size={14} /> : <CheckCircle2 size={14} />}
        {errors} errors
      </span>
      <span>{warnings} warnings</span>
    </footer>
  );
}
