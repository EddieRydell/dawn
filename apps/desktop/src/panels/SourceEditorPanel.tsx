import Editor, { loader, type OnMount } from "@monaco-editor/react";
import * as monaco from "monaco-editor";
import { useCallback, useMemo } from "react";
import { useWorkbench } from "../store/workbenchStore";

loader.config({ monaco });

export function SourceEditorPanel() {
  const activeFile = useWorkbench((state) => state.activeFile);
  const activeEditor = useWorkbench((state) => state.openEditors.find((editor) => editor.path === state.activeFile) ?? null);
  const setFileContent = useWorkbench((state) => state.setFileContent);

  const language = useMemo(() => {
    if (activeFile?.endsWith(".vibe")) return "javascript";
    return "json";
  }, [activeFile]);

  const handleEditorMount: OnMount = useCallback((editor) => {
    editor.addCommand(monaco.KeyMod.CtrlCmd | monaco.KeyCode.KeyS, () => {
      void useWorkbench.getState().saveFile();
    });
    editor.addCommand(monaco.KeyMod.CtrlCmd | monaco.KeyCode.Tab, () => {
      useWorkbench.getState().activateNextEditor(1);
    });
    editor.addCommand(monaco.KeyMod.CtrlCmd | monaco.KeyMod.Shift | monaco.KeyCode.Tab, () => {
      useWorkbench.getState().activateNextEditor(-1);
    });
  }, []);

  return (
    <section className="editor-pane">
      <div className="workbench-content">
        <Editor
          path={activeEditor?.path ?? "donder://empty"}
          height="100%"
          language={language}
          theme="vs-dark"
          value={activeEditor?.content ?? ""}
          onChange={(value) => setFileContent(value ?? "")}
          onMount={handleEditorMount}
          saveViewState
          options={{
            minimap: { enabled: false },
            fontSize: 13,
            automaticLayout: true,
            scrollBeyondLastLine: false,
            tabSize: 2
          }}
        />
      </div>
    </section>
  );
}
