import { useEffect } from "react";
import { commands } from "../api";
import { installGlobalShortcuts } from "../commandRegistry";
import { runRuntimeCommand, subscribeToRuntimeState, useAppStore } from "../store";
import { EditorPane } from "./EditorPane";
import { ExportFseqDialog } from "./ExportFseqDialog";
import { NewProjectDialog } from "./NewProjectDialog";
import { ProjectTree } from "./ProjectTree";
import { StatusBar } from "./StatusBar";
import { TitleBar } from "./TitleBar";

export function App() {
  const { runtimeState, error, hydrate } = useAppStore();

  useEffect(() => {
    void hydrate();
    const disposeShortcuts = installGlobalShortcuts();
    let disposeEvents: (() => void) | undefined;
    void subscribeToRuntimeState().then((dispose) => {
      disposeEvents = dispose;
    });
    return () => {
      disposeShortcuts();
      disposeEvents?.();
    };
  }, [hydrate]);

  if (!runtimeState) {
    return <div className="app-loading">Dawn</div>;
  }

  return (
    <div className="app-shell">
      <TitleBar />
      {error !== null && error !== "" && <div className="error-strip">{error}</div>}
      <main className="workbench">
        {runtimeState.projectTreeVisible ? <ProjectTree snapshot={runtimeState} /> : null}
        <EditorPane snapshot={runtimeState} />
      </main>
      <StatusBar snapshot={runtimeState} />
      <NewProjectDialog />
      <ExportFseqDialog />
      {runtimeState.projectRoot === null && (
        <div className="empty-project">
          <button onClick={() => void runRuntimeCommand(commands.openProjectDialog)}>Open Project</button>
        </div>
      )}
    </div>
  );
}
