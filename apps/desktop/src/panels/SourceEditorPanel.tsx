import { useEffect, useRef } from "react";
import { DawnEditorRuntime } from "../language/DawnEditorRuntime";
import { useWorkbench } from "../store/workbenchStore";

export function SourceEditorPanel() {
  const containerRef = useRef<HTMLDivElement | null>(null);
  const runtimeRef = useRef<DawnEditorRuntime | null>(null);
  const projectState = useWorkbench((state) => state.projectState);
  const activeFile = useWorkbench((state) => state.activeFile);
  const pendingRevealProblem = useWorkbench((state) => state.pendingRevealProblem);
  const languageProblems = useWorkbench((state) => state.languageProblems);
  const openEditors = useWorkbench((state) => state.openEditors);
  const setEditorContent = useWorkbench((state) => state.setEditorContent);
  const setLanguageProblems = useWorkbench((state) => state.setLanguageProblems);
  const clearPendingRevealProblem = useWorkbench((state) => state.clearPendingRevealProblem);
  const saveFile = useWorkbench((state) => state.saveFile);
  const activateNextEditor = useWorkbench((state) => state.activateNextEditor);

  useEffect(() => {
    const container = containerRef.current;
    if (!container) return;

    const runtime = new DawnEditorRuntime({
      onContentChanged: setEditorContent,
      onProblemsChanged: setLanguageProblems,
      saveFile,
      activateNextEditor
    });
    runtimeRef.current = runtime;
    runtime.start(container);

    return () => {
      runtimeRef.current = null;
      void runtime.dispose();
    };
  }, [activateNextEditor, saveFile, setEditorContent, setLanguageProblems]);

  useEffect(() => {
    void runtimeRef.current?.setProject(projectState ? { root: projectState.root } : null);
  }, [projectState]);

  useEffect(() => {
    runtimeRef.current?.syncOpenFiles(openEditors);
  }, [openEditors]);

  useEffect(() => {
    runtimeRef.current?.setActiveFile(activeFile);
  }, [activeFile]);

  useEffect(() => {
    runtimeRef.current?.setProblems(languageProblems);
  }, [languageProblems]);

  useEffect(() => {
    if (!pendingRevealProblem || pendingRevealProblem.path !== activeFile) return;
    runtimeRef.current?.revealProblem(pendingRevealProblem);
    clearPendingRevealProblem();
  }, [activeFile, pendingRevealProblem, clearPendingRevealProblem]);

  return (
    <section className="editor-pane">
      <div ref={containerRef} className="workbench-content editor-host" />
    </section>
  );
}
