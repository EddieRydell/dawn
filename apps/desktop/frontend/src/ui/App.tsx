import { useCallback, useEffect, useRef, useState } from "react";
import { commands } from "../api";
import { installGlobalShortcuts } from "../commandRegistry";
import { runSnapshotCommand, subscribeToSnapshots, useAppStore } from "../store";
import type { AppSnapshot, WorkspaceLayoutState } from "../types";
import { EditorPane } from "./EditorPane";
import { NewProjectDialog } from "./NewProjectDialog";
import { NewSequenceDialog } from "./NewSequenceDialog";
import { OperatorRewriteDialog } from "./OperatorRewriteDialog";
import { SettingsDialog } from "./SettingsDialog";
import { StatusBar } from "./StatusBar";
import { TitleBar } from "./TitleBar";
import { WorkspaceResizeHandle } from "./WorkspaceResizeHandle";
import { THEME_METRICS } from "../theme";
import { CommandOverlays } from "../workspace/CommandOverlays";
import { WorkspaceSidebar } from "../workspace/WorkspaceSidebar";

const PROJECT_TREE_MIN_WIDTH_PX = THEME_METRICS.projectPanelMinWidth;
const PROJECT_TREE_MAX_WIDTH_PX = THEME_METRICS.projectPanelMaxWidth;
const WORKSPACE_LAYOUT_SAVE_DELAY_MS = THEME_METRICS.workspaceLayoutSaveDelay;

export function App() {
  const { snapshot, error, hydrate, compositionGraphEditing } = useAppStore();

  useEffect(() => {
    void hydrate();
    const disposeShortcuts = installGlobalShortcuts();
    let disposeEvents: (() => void) | undefined;
    void subscribeToSnapshots().then((dispose) => {
      disposeEvents = dispose;
    });
    return () => {
      disposeShortcuts();
      disposeEvents?.();
    };
  }, [hydrate]);

  if (!snapshot) {
    return <div className="app-loading">Dawn</div>;
  }

  return (
    <div className="app-shell">
      <TitleBar />
      <div className="alert-stack">
        {error !== null && error !== "" && <div className="error-strip">{error}</div>}
        {!compositionGraphEditing && snapshot.renderError !== null && snapshot.renderError !== "" && (
          <div className="error-strip">{snapshot.renderError}</div>
        )}
        {snapshot.previewError !== null && snapshot.previewError !== "" && (
          <div className="error-strip">{snapshot.previewError}</div>
        )}
      </div>
      <WorkspaceMain snapshot={snapshot} />
      <StatusBar snapshot={snapshot} />
      <NewProjectDialog />
      <NewSequenceDialog />
      <SettingsDialog />
      <OperatorRewriteDialog />
      <CommandOverlays />
      {snapshot.projectRoot === null && (
        <div className="empty-project">
          <button onClick={() => void runSnapshotCommand(commands.openProjectDialog)}>Open Project...</button>
        </div>
      )}
    </div>
  );
}

function WorkspaceMain({ snapshot }: { snapshot: AppSnapshot }) {
  const [workspaceLayout, setWorkspaceLayout] = useState<WorkspaceLayoutState>(() => snapshot.workspaceLayout);
  const [serverLayout, setServerLayout] = useState<WorkspaceLayoutState>(() => snapshot.workspaceLayout);
  const layoutSaveTimer = useRef<number | undefined>(undefined);

  if (!sameWorkspaceLayout(serverLayout, snapshot.workspaceLayout)) {
    setServerLayout(snapshot.workspaceLayout);
    setWorkspaceLayout(snapshot.workspaceLayout);
  }

  useEffect(() => {
    return () => {
      window.clearTimeout(layoutSaveTimer.current);
    };
  }, []);

  const updateWorkspaceLayout = useCallback((next: WorkspaceLayoutState, immediate = false) => {
    setWorkspaceLayout(next);
    window.clearTimeout(layoutSaveTimer.current);
    if (immediate) {
      void runSnapshotCommand(() => commands.saveWorkspaceLayoutState(next));
      return;
    }
    layoutSaveTimer.current = window.setTimeout(() => {
      void runSnapshotCommand(() => commands.saveWorkspaceLayoutState(next));
    }, WORKSPACE_LAYOUT_SAVE_DELAY_MS);
  }, []);

  const layout = workspaceLayout;

  return (
    <main className="workbench">
      <div
        className={`project-panel-shell ${layout.sidebarCollapsed ? "collapsed" : ""}`}
        style={{ width: layout.sidebarCollapsed ? undefined : layout.sidebarWidthPx }}
      >
        <WorkspaceSidebar snapshot={snapshot} layout={layout} onLayoutChange={updateWorkspaceLayout} />
        <WorkspaceResizeHandle
          ariaLabel="Resize side bar"
          collapsed={layout.sidebarCollapsed}
          direction="left"
          min={PROJECT_TREE_MIN_WIDTH_PX}
          max={PROJECT_TREE_MAX_WIDTH_PX}
          value={layout.sidebarWidthPx}
          onChange={(update) => {
            updateWorkspaceLayout({
              ...layout,
              sidebarCollapsed: update.collapsed,
              sidebarWidthPx: update.width
            });
          }}
        />
      </div>
      <EditorPane snapshot={snapshot} workspaceLayout={layout} onWorkspaceLayoutChange={updateWorkspaceLayout} />
    </main>
  );
}

function sameWorkspaceLayout(left: WorkspaceLayoutState, right: WorkspaceLayoutState) {
  return left.sidebarWidthPx === right.sidebarWidthPx
    && left.sidebarCollapsed === right.sidebarCollapsed
    && left.activeSidebarView === right.activeSidebarView
    && left.inspectorWidthPx === right.inspectorWidthPx
    && left.inspectorCollapsed === right.inspectorCollapsed;
}
