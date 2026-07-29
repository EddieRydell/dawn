import { AlertTriangle, CircleX } from "lucide-react";
import { useMemo } from "react";
import { THEME_METRICS } from "../theme";
import type { AppSnapshot } from "../types";
import { sameWorkspacePath } from "./helpers";
import { navigateToText } from "./navigation";

export function ProblemsView({ snapshot }: { snapshot: AppSnapshot }) {
  const groups = useMemo(() => {
    const values = new Map<string, AppSnapshot["diagnostics"]>();
    for (const diagnostic of snapshot.diagnostics) {
      const entry = snapshot.projectEntries.find((candidate) =>
        sameWorkspacePath(diagnostic.path, candidate.path, snapshot.projectRoot)
      );
      const path = entry?.path ?? diagnostic.path;
      const group = values.get(path) ?? [];
      group.push(diagnostic);
      values.set(path, group);
    }
    return [...values.entries()];
  }, [snapshot.diagnostics, snapshot.projectEntries, snapshot.projectRoot]);
  const errors = snapshot.diagnostics.filter((diagnostic) => diagnostic.severity === "error").length;
  const warnings = snapshot.diagnostics.length - errors;
  return (
    <section className="sidebar-view problems-view" aria-label="Problems">
      <header className="sidebar-view-header">
        <h2>Problems</h2>
        <span>{errors} errors · {warnings} warnings</span>
      </header>
      <div className="sidebar-scroll">
        {groups.map(([path, diagnostics]) => (
          <section key={path} className="problem-file-group">
            <h3>{path}</h3>
            {diagnostics.map((diagnostic, index) => {
              const Icon = diagnostic.severity === "error" ? CircleX : AlertTriangle;
              return (
                <button
                  type="button"
                  key={`${diagnostic.code}:${index}`}
                  onClick={() => void navigateToText(path, diagnostic.range)}
                >
                  <Icon size={THEME_METRICS.iconSizeSmall} />
                  <span>
                    {diagnostic.message}
                    <small>
                      {diagnostic.code}
                      {diagnostic.range !== null
                        ? ` · ${diagnostic.range.start.line + 1}:${diagnostic.range.start.character + 1}`
                        : ""}
                    </small>
                  </span>
                </button>
              );
            })}
          </section>
        ))}
        {groups.length === 0 && <p className="empty-sidebar-state">No problems detected.</p>}
      </div>
    </section>
  );
}
