import { useEffect } from "react";
import { commands } from "../api";
import { installGlobalShortcuts } from "../commandRegistry";
import { runSnapshotCommand, subscribeToSnapshots, useAppStore } from "../store";
import { EditorPane } from "./EditorPane";
import { ProjectTree } from "./ProjectTree";
import { StatusBar } from "./StatusBar";
import { TitleBar } from "./TitleBar";

export function App() {
  const { snapshot, error, hydrate } = useAppStore();

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
      {error && <div className="error-strip">{error}</div>}
      <main className="workbench">
        {snapshot.projectTreeVisible && <ProjectTree snapshot={snapshot} />}
        <EditorPane snapshot={snapshot} />
      </main>
      <StatusBar snapshot={snapshot} />
      {!snapshot.projectRoot && (
        <div className="empty-project">
          <button onClick={() => void runSnapshotCommand(commands.openProjectDialog)}>Open Project</button>
        </div>
      )}
    </div>
  );
}
