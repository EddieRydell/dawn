import { useCallback, useEffect, useRef, useState } from "react";
import { commands } from "../api";
import { installGlobalShortcuts } from "../commandRegistry";
import { runSnapshotCommand, subscribeToSnapshots, useAppStore } from "../store";
import type { AppSnapshot, WorkspaceLayoutState } from "../types";
import { EditorPane } from "./EditorPane";
import { NewProjectDialog } from "./NewProjectDialog";
import { NewSequenceDialog } from "./NewSequenceDialog";
import { ProjectTree } from "./ProjectTree";
import { SettingsDialog } from "./SettingsDialog";
import { StatusBar } from "./StatusBar";
import { TitleBar } from "./TitleBar";
import { WorkspaceResizeHandle } from "./WorkspaceResizeHandle";

const PROJECT_TREE_MIN_WIDTH_PX = 220;
const PROJECT_TREE_MAX_WIDTH_PX = 520;
const WORKSPACE_LAYOUT_SAVE_DELAY_MS = 250;

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
  const layoutSaveTimer = useRef<number | undefined>(undefined);

  useEffect(() => {
    return () => {
      window.clearTimeout(layoutSaveTimer.current);
    };
  }, []);

  const updateWorkspaceLayout = useCallback((next: WorkspaceLayoutState) => {
    setWorkspaceLayout(next);
    window.clearTimeout(layoutSaveTimer.current);
    layoutSaveTimer.current = window.setTimeout(() => {
      void runSnapshotCommand(() => commands.saveWorkspaceLayoutState(next));
    }, WORKSPACE_LAYOUT_SAVE_DELAY_MS);
  }, []);

  const layout = workspaceLayout;

  return (
    <main className="workbench">
      {snapshot.projectTreeVisible ? (
        <div
          className={`project-panel-shell ${layout.projectTreeCollapsed ? "collapsed" : ""}`}
          style={{ width: layout.projectTreeCollapsed ? 8 : layout.projectTreeWidthPx }}
        >
          {!layout.projectTreeCollapsed && <ProjectTree snapshot={snapshot} />}
          <WorkspaceResizeHandle
            ariaLabel="Resize project tree"
            collapsed={layout.projectTreeCollapsed}
            direction="left"
            min={PROJECT_TREE_MIN_WIDTH_PX}
            max={PROJECT_TREE_MAX_WIDTH_PX}
            value={layout.projectTreeWidthPx}
            onChange={(update) => {
              updateWorkspaceLayout({
                ...layout,
                projectTreeCollapsed: update.collapsed,
                projectTreeWidthPx: update.width
              });
            }}
          />
        </div>
      ) : null}
      <EditorPane snapshot={snapshot} workspaceLayout={layout} onWorkspaceLayoutChange={updateWorkspaceLayout} />
    </main>
  );
}
