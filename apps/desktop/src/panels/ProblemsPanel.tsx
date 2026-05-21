import { useWorkbench } from "../store/workbenchStore";
import type { Diagnostic } from "../types";

const emptyDiagnostics: Diagnostic[] = [];

export function ProblemsPanel() {
  const diagnostics = useWorkbench((state) => state.projectState?.diagnostics ?? emptyDiagnostics);
  const openFile = useWorkbench((state) => state.openFile);

  return (
    <section className="panel problems">
      {diagnostics.length === 0 ? (
        <div className="empty-problems">No problems reported.</div>
      ) : (
        diagnostics.map((diagnostic, index) => (
          <button key={index} className={`problem ${diagnostic.severity.toLowerCase()}`} onClick={() => void openFile(diagnostic.path)}>
            <strong>{diagnostic.severity}</strong>
            <span>{diagnostic.message}</span>
            <small>{diagnostic.path}</small>
          </button>
        ))
      )}
    </section>
  );
}
