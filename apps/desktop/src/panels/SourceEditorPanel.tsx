import { useEffect, useMemo, useRef } from "react";
import { FileText, PanelsTopLeft } from "lucide-react";
import { DawnEditorRuntime } from "../language/DawnEditorRuntime";
import { useWorkbench } from "../store/workbenchStore";
import { LayoutViewer } from "./LayoutViewer";
import { FixtureViewer } from "./FixtureViewer";

export function SourceEditorPanel() {
  const containerRef = useRef<HTMLDivElement | null>(null);
  const runtimeRef = useRef<DawnEditorRuntime | null>(null);
  const projectState = useWorkbench((state) => state.projectState);
  const activeFile = useWorkbench((state) => state.activeFile);
  const pendingRevealProblem = useWorkbench((state) => state.pendingRevealProblem);
  const languageProblems = useWorkbench((state) => state.languageProblems);
  const openEditors = useWorkbench((state) => state.openEditors);
  const editorViewsByPath = useWorkbench((state) => state.editorViewsByPath);
  const layoutView = useWorkbench((state) => state.layoutView);
  const fixtureView = useWorkbench((state) => state.fixtureView);
  const setEditorContent = useWorkbench((state) => state.setEditorContent);
  const setLanguageProblems = useWorkbench((state) => state.setLanguageProblems);
  const clearPendingRevealProblem = useWorkbench((state) => state.clearPendingRevealProblem);
  const saveFile = useWorkbench((state) => state.saveFile);
  const activateNextEditor = useWorkbench((state) => state.activateNextEditor);
  const inspectActiveDocument = useWorkbench((state) => state.inspectActiveDocument);
  const setEditorView = useWorkbench((state) => state.setEditorView);
  const selectLayoutObject = useWorkbench((state) => state.selectLayoutObject);
  const selectLayoutFixture = useWorkbench((state) => state.selectLayoutFixture);
  const selectLayoutGroup = useWorkbench((state) => state.selectLayoutGroup);
  const selectFixtureObject = useWorkbench((state) => state.selectFixtureObject);
  const applyLayoutDocumentEdit = useWorkbench((state) => state.applyLayoutDocumentEdit);
  const applyFixtureDocumentEdit = useWorkbench((state) => state.applyFixtureDocumentEdit);

  const activeView = activeFile ? editorViewsByPath[activeFile] ?? "text" : "text";
  const descriptor = layoutView.path === activeFile ? layoutView.descriptor : null;
  const layoutEnabled = Boolean(descriptor?.availableViews.includes("layout"));
  const fixtureEnabled = Boolean(descriptor?.availableViews.includes("fixture"));
  const guiView = activeView !== "text" && ((activeView === "layout" && layoutEnabled) || (activeView === "fixture" && fixtureEnabled))
    ? activeView
    : layoutEnabled
      ? "layout"
      : fixtureEnabled
        ? "fixture"
        : null;
  const guiActive = activeView !== "text";
  const layoutObjectKeys = useMemo(
    () => descriptor?.objects.filter((object) => object.kind === "layout").map((object) => object.key) ?? [],
    [descriptor]
  );
  const fixtureObjectKeys = useMemo(
    () => descriptor?.objects.filter((object) => object.kind === "fixture").map((object) => object.key) ?? [],
    [descriptor]
  );

  const commitLayoutDocument = async (document: Parameters<typeof applyLayoutDocumentEdit>[0]) => {
    const result = await applyLayoutDocumentEdit(document, false);
    if (result?.type === "blocked" && window.confirm(`${result.message}\n\n${formatDiagnostics(result.diagnostics)}\n\nApply anyway?`)) {
      await applyLayoutDocumentEdit(document, true);
    }
  };

  const commitFixtureDocument = async (document: Parameters<typeof applyFixtureDocumentEdit>[0]) => {
    const result = await applyFixtureDocumentEdit(document, false);
    if (result?.type === "blocked" && window.confirm(`${result.message}\n\n${formatDiagnostics(result.diagnostics)}\n\nApply anyway?`)) {
      await applyFixtureDocumentEdit(document, true);
    }
  };

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
    void inspectActiveDocument();
  }, [activeFile]);

  useEffect(() => {
    if (activeView === "text") {
      window.requestAnimationFrame(() => runtimeRef.current?.layout());
    }
  }, [activeView]);

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
      <div className="editor-view-toolbar">
        <div className="segmented-control" role="tablist" aria-label="Editor view">
          <button
            className={activeView === "text" ? "active" : ""}
            disabled={!activeFile}
            onClick={() => activeFile && setEditorView(activeFile, "text")}
            title="Text"
          >
            <FileText size={14} />
            <span>Text</span>
          </button>
          <button
            className={guiActive ? "active" : ""}
            disabled={!activeFile || !guiView}
            onClick={() => activeFile && guiView && setEditorView(activeFile, guiView)}
            title="GUI"
          >
            <PanelsTopLeft size={14} />
            <span>GUI</span>
          </button>
        </div>
        {activeView === "layout" && layoutObjectKeys.length > 1 && (
          <select
            className="layout-object-select"
            value={layoutView.objectKey ?? layoutObjectKeys[0]}
            onChange={(event) => selectLayoutObject(event.target.value)}
            aria-label="Layout object"
          >
            {layoutObjectKeys.map((key) => (
              <option key={key} value={key}>{key}</option>
            ))}
          </select>
        )}
        {activeView === "fixture" && fixtureObjectKeys.length > 1 && (
          <select
            className="layout-object-select"
            value={fixtureView.selectedObjectKey ?? fixtureObjectKeys[0]}
            onChange={(event) => selectFixtureObject(event.target.value)}
            aria-label="Fixture object"
          >
            {fixtureObjectKeys.map((key) => (
              <option key={key} value={key}>{key}</option>
            ))}
          </select>
        )}
      </div>
      <div className="editor-view-stack">
        <div
          ref={containerRef}
          className={activeView === "text" ? "workbench-content editor-host" : "workbench-content editor-host hidden"}
        />
        {activeView === "layout" && (
          <div className="layout-view-host">
            {layoutView.status === "loading" && <div className="layout-loading">Loading layout...</div>}
            {layoutView.status === "error" && <div className="layout-error">{layoutView.error}</div>}
            {layoutView.status === "idle" && <div className="layout-error">Layout view is not available for this file.</div>}
            {layoutView.status === "ready" && layoutView.document && (
              <LayoutViewer
                document={layoutView.document}
                selectedFixtureId={layoutView.selectedFixtureId}
                highlightedGroup={layoutView.highlightedGroup}
                onSelectFixture={selectLayoutFixture}
                onHighlightGroup={selectLayoutGroup}
                onDocumentChange={commitLayoutDocument}
              />
            )}
          </div>
        )}
        {activeView === "fixture" && (
          <div className="layout-view-host">
            {fixtureView.status === "loading" && <div className="layout-loading">Loading fixtures...</div>}
            {fixtureView.status === "error" && <div className="layout-error">{fixtureView.error}</div>}
            {fixtureView.status === "idle" && <div className="layout-error">Fixture view is not available for this file.</div>}
            {fixtureView.status === "ready" && fixtureView.document && (
              <FixtureViewer
                document={fixtureView.document}
                selectedObjectKey={fixtureView.selectedObjectKey}
                onSelectObject={selectFixtureObject}
                onDocumentChange={commitFixtureDocument}
              />
            )}
          </div>
        )}
      </div>
    </section>
  );
}

function formatDiagnostics(diagnostics: Array<{ path: string; line: number; column: number; message: string }>) {
  return diagnostics.slice(0, 5).map((diagnostic) =>
    `${diagnostic.path}:${diagnostic.line}:${diagnostic.column} ${diagnostic.message}`
  ).join("\n");
}
