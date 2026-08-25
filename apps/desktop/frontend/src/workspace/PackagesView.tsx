import * as AlertDialog from "@radix-ui/react-alert-dialog";
import {
  ExternalLink,
  GitFork,
  PackageCheck,
  RefreshCw,
  Trash2
} from "lucide-react";
import { useState } from "react";
import { commands } from "../api";
import { runSnapshotCommand } from "../store";
import { THEME_METRICS } from "../theme";
import type { AppSnapshot, PackageDependencyStatus } from "../types";
import type { AppStaticSnapshot } from "../store";

export function PackagesView({ snapshot }: { snapshot: AppStaticSnapshot }) {
  const status = snapshot.package;
  const [pendingRemoval, setPendingRemoval] = useState<PackageDependencyStatus | null>(null);
  const hasRegistry = status.dependencies.some((dependency) => dependency.source === "registry");
  return (
    <section className="sidebar-view packages-view" aria-label="Packages">
      <header className="sidebar-view-header">
        <div>
          <h2>Packages</h2>
          <span className={`package-readiness readiness-${status.readiness}`}>
            {readinessLabel(status.readiness)}
          </span>
        </div>
        <div className="sidebar-toolbar">
          <button
            type="button"
            aria-label="Synchronize packages"
            title="Synchronize lockfile and cache"
            onClick={() => void runSnapshotCommand(commands.syncPackages)}
          >
            <PackageCheck size={THEME_METRICS.iconSizeCompact} />
          </button>
          <button
            type="button"
            aria-label="Check package updates"
            title="Check package updates"
            disabled={!hasRegistry}
            onClick={() => void runSnapshotCommand(commands.checkPackageUpdates)}
          >
            <RefreshCw size={THEME_METRICS.iconSizeCompact} />
          </button>
          <button
            type="button"
            disabled={!hasRegistry}
            onClick={() => void runSnapshotCommand(() => commands.updatePackages(null))}
          >
            Update all
          </button>
        </div>
      </header>
      <div className="sidebar-scroll">
        {status.message !== null && <p className="sidebar-message">{status.message}</p>}
        {status.registry !== null && (
          <dl className="package-metadata">
            <div><dt>Registry</dt><dd title={status.registry}>{status.registry}</dd></div>
            <div><dt>Lockfile</dt><dd>{status.lockCurrent ? "Current" : "Needs sync"}</dd></div>
          </dl>
        )}
        <h3>Dependencies</h3>
        {status.dependencies.length === 0 ? (
          <p className="empty-sidebar-state">No dependencies declared.</p>
        ) : (
          <ul className="package-list">
            {status.dependencies.map((dependency) => (
              <li key={dependency.alias}>
                <div className="package-dependency-heading">
                  <div>
                    <strong>{dependency.alias}</strong>
                    <span>{dependency.package ?? dependency.requirement}</span>
                  </div>
                  <span className={`package-cache cache-${dependency.cache}`}>
                    {dependency.cache}
                  </span>
                </div>
                <div className="package-dependency-details">
                  <span>{dependency.source === "path" ? "Local path" : dependency.lockedVersion ?? "Not locked"}</span>
                  {dependency.moduleId !== null && <span title={dependency.moduleId}>Module {dependency.moduleId}</span>}
                </div>
                {dependency.warnings.map((warning) => <p key={warning} className="package-warning">{warning}</p>)}
                <div className="package-actions">
                  {dependency.source === "registry" && (
                    <>
                      <button
                        type="button"
                        disabled={dependency.updateAvailable !== true}
                        onClick={() => void runSnapshotCommand(() => commands.updatePackages(dependency.alias))}
                      >
                        Update
                      </button>
                      <button
                        type="button"
                        onClick={() => void runSnapshotCommand(() => commands.forkPackageDependency(dependency.alias))}
                      >
                        <GitFork size={THEME_METRICS.iconSizeSmall} /> Fork
                      </button>
                    </>
                  )}
                  <button
                    type="button"
                    disabled={dependency.websiteUrl === null}
                    title={dependency.websiteUrl ?? "No registry page for local dependencies"}
                    onClick={() => void runSnapshotCommand(() => commands.openPackagePage(dependency.alias))}
                  >
                    <ExternalLink size={THEME_METRICS.iconSizeSmall} /> Website
                  </button>
                  <button type="button" className="danger" onClick={() => { setPendingRemoval(dependency); }}>
                    <Trash2 size={THEME_METRICS.iconSizeSmall} /> Remove
                  </button>
                </div>
              </li>
            ))}
          </ul>
        )}
        {status.warnings.length > 0 && (
          <>
            <h3>Warnings</h3>
            <ul className="problem-list">
              {status.warnings.map((warning, index) => (
                <li key={`${warning.package}:${index}`}>
                  <strong>{warning.package}</strong>
                  <span>{warning.message}</span>
                </li>
              ))}
            </ul>
          </>
        )}
        {status.modules.length > 0 && (
          <>
            <h3>Modules</h3>
            <ul className="package-module-list">
              {status.modules.map((module) => (
                <li key={module.moduleId}>
                  <strong>{module.identity}</strong>
                  <span title={module.moduleId}>{module.version ?? "local"}</span>
                  <ul>
                    {module.documents.map((document) => <li key={document}>{document}</li>)}
                  </ul>
                </li>
              ))}
            </ul>
          </>
        )}
      </div>
      <AlertDialog.Root open={pendingRemoval !== null} onOpenChange={(open) => { if (!open) setPendingRemoval(null); }}>
        <AlertDialog.Portal>
          <AlertDialog.Overlay className="dialog-overlay" />
          <AlertDialog.Content className="dialog-content">
            <AlertDialog.Title>Remove {pendingRemoval?.alias}</AlertDialog.Title>
            <AlertDialog.Description>
              This removes the dependency from the package manifest and synchronizes the lockfile.
            </AlertDialog.Description>
            <div className="dialog-actions">
              <AlertDialog.Cancel>Cancel</AlertDialog.Cancel>
              <AlertDialog.Action
                onClick={() => {
                  if (pendingRemoval !== null) {
                    void runSnapshotCommand(() => commands.removePackageDependency(pendingRemoval.alias));
                  }
                }}
              >
                Remove
              </AlertDialog.Action>
            </div>
          </AlertDialog.Content>
        </AlertDialog.Portal>
      </AlertDialog.Root>
    </section>
  );
}

function readinessLabel(readiness: AppSnapshot["package"]["readiness"]): string {
  switch (readiness) {
    case "noProject": return "No project";
    case "invalid": return "Invalid";
    case "needsSync": return "Needs sync";
    case "warning": return "Warnings";
    case "ready": return "Ready";
  }
}
