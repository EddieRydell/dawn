import * as Tooltip from "@radix-ui/react-tooltip";
import { AlertTriangle, Box, CheckCircle2, CircleX, FolderOpen } from "lucide-react";
import { FOCUS_SIDEBAR_EVENT } from "../commandRegistry";
import { THEME_METRICS } from "../theme";
import type { AppStaticSnapshot } from "../store";
import type { AppSnapshot, SidebarView } from "../types";

export function StatusBar({ snapshot }: { snapshot: AppStaticSnapshot }) {
  const errors = snapshot.diagnostics.filter((diagnostic) => diagnostic.severity === "error").length;
  const warnings = snapshot.diagnostics.filter((diagnostic) => diagnostic.severity === "warning").length;
  const projectParts = snapshot.projectRoot?.replace(/\\/g, "/").split("/") ?? [];
  const projectName = projectParts[projectParts.length - 1] ?? "No project";
  return (
    <Tooltip.Provider delayDuration={THEME_METRICS.tooltipDelayMs}>
      <footer className="status-bar">
        <StatusChip
          label={snapshot.projectHealth === "recovery" ? `${projectName} · Recovery` : projectName}
          tooltip={projectHealthTooltip(snapshot)}
          icon={<FolderOpen size={THEME_METRICS.iconSizeSmall} />}
          tone={`project-health-${snapshot.projectHealth}`}
          {...(snapshot.projectHealth === "recovery"
            ? { onClick: () => { focusSidebar("problems"); } }
            : {})}
        />
        <StatusChip
          label={packageLabel(snapshot.package.readiness)}
          tooltip={snapshot.package.message ?? packageTooltip(snapshot)}
          icon={<Box size={THEME_METRICS.iconSizeSmall} />}
          tone={`readiness-${snapshot.package.readiness}`}
          onClick={() => { focusSidebar("packages"); }}
        />
        <span className="status-spacer" title={snapshot.status}>{snapshot.status}</span>
        <StatusChip
          label={String(errors)}
          tooltip={`${errors} errors`}
          icon={errors > 0
            ? <CircleX size={THEME_METRICS.iconSizeSmall} />
            : <CheckCircle2 size={THEME_METRICS.iconSizeSmall} />}
          tone={errors > 0 ? "status-problem" : "status-ok"}
          onClick={() => { focusSidebar("problems"); }}
        />
        <StatusChip
          label={String(warnings)}
          tooltip={`${warnings} warnings`}
          icon={<AlertTriangle size={THEME_METRICS.iconSizeSmall} />}
          tone={warnings > 0 ? "status-warning" : ""}
          onClick={() => { focusSidebar("problems"); }}
        />
      </footer>
    </Tooltip.Provider>
  );
}

function projectHealthTooltip(snapshot: AppStaticSnapshot): string {
  if (snapshot.projectHealth === "recovery") {
    return "The project has model-blocking errors. Text, Search, Explorer, and Problems remain available.";
  }
  return snapshot.projectRoot ?? "No project is open";
}

function StatusChip({
  label,
  tooltip,
  icon,
  tone = "",
  onClick
}: {
  label: string;
  tooltip: string;
  icon: React.ReactNode;
  tone?: string;
  onClick?: () => void;
}) {
  const content = onClick === undefined
    ? <span className={`status-chip ${tone}`}>{icon}{label}</span>
    : <button type="button" className={`status-chip ${tone}`} onClick={onClick}>{icon}{label}</button>;
  return (
    <Tooltip.Root>
      <Tooltip.Trigger asChild>{content}</Tooltip.Trigger>
      <Tooltip.Portal><Tooltip.Content className="tooltip-content" side="top">{tooltip}</Tooltip.Content></Tooltip.Portal>
    </Tooltip.Root>
  );
}

function focusSidebar(view: SidebarView) {
  window.dispatchEvent(new CustomEvent<SidebarView>(FOCUS_SIDEBAR_EVENT, { detail: view }));
}

function packageLabel(readiness: AppSnapshot["package"]["readiness"]): string {
  switch (readiness) {
    case "noProject": return "Packages";
    case "invalid": return "Invalid package";
    case "needsSync": return "Packages need sync";
    case "warning": return "Package warnings";
    case "ready": return "Packages ready";
  }
}

function packageTooltip(snapshot: AppStaticSnapshot): string {
  const root = snapshot.package.root ?? "No package root";
  const registry = snapshot.package.registry ?? "No registry selected";
  return `${root} · ${registry}`;
}
