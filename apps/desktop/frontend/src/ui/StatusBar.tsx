import { AlertTriangle, CheckCircle2 } from "lucide-react";
import type { ReactNode } from "react";
import type { AppSnapshot, ProjectDiagnostic } from "../types";

export function StatusBar({ snapshot }: { snapshot: AppSnapshot }) {
  const errorDiagnostics = snapshot.diagnostics.filter((diagnostic) => diagnostic.severity === "error");
  const warningDiagnostics = snapshot.diagnostics.filter((diagnostic) => diagnostic.severity === "warning");
  return (
    <footer className="status-bar">
      <span>{snapshot.status}</span>
      <span>{snapshot.projectRoot ?? "No project"}</span>
      <DiagnosticChip
        diagnostics={errorDiagnostics}
        icon={errorDiagnostics.length > 0 ? <AlertTriangle size={14} /> : <CheckCircle2 size={14} />}
        label={`${errorDiagnostics.length} errors`}
        tone={errorDiagnostics.length > 0 ? "status-problem" : "status-ok"}
      />
      <DiagnosticChip
        diagnostics={warningDiagnostics}
        icon={<AlertTriangle size={14} />}
        label={`${warningDiagnostics.length} warnings`}
        tone="status-warning"
      />
    </footer>
  );
}

function DiagnosticChip({
  diagnostics,
  icon,
  label,
  tone
}: {
  diagnostics: ProjectDiagnostic[];
  icon: ReactNode;
  label: string;
  tone: string;
}) {
  return (
    <button className={`status-diagnostics-button ${tone}`} type="button">
      {icon}
      {label}
      {diagnostics.length > 0 ? (
        <span className="status-diagnostics-tooltip" role="tooltip">
          {diagnostics.map((diagnostic, index) => (
            <span key={`${diagnostic.path}:${diagnostic.code}:${index}`}>{diagnosticLabel(diagnostic)}</span>
          ))}
        </span>
      ) : null}
    </button>
  );
}

function diagnosticLabel(diagnostic: ProjectDiagnostic): string {
  if (diagnostic.range === null) {
    return `${diagnostic.path} ${diagnostic.code} ${diagnostic.message}`;
  }
  return `${diagnostic.path}:${diagnostic.range.start.line + 1}:${diagnostic.range.start.character + 1} ${diagnostic.code} ${diagnostic.message}`;
}
