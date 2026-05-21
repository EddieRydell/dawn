import Editor, { loader, type OnMount } from "@monaco-editor/react";
import * as monaco from "monaco-editor";
import { useCallback, useMemo } from "react";
import { useWorkbench } from "../store/workbenchStore";

loader.config({ monaco });

let dawnRegistered = false;

export function SourceEditorPanel() {
  const activeFile = useWorkbench((state) => state.activeFile);
  const activeEditor = useWorkbench((state) => state.openEditors.find((editor) => editor.path === state.activeFile) ?? null);
  const setFileContent = useWorkbench((state) => state.setFileContent);

  const language = useMemo(() => {
    if (activeFile?.endsWith(".dawn")) return "dawn";
    return "json";
  }, [activeFile]);

  const handleEditorMount: OnMount = useCallback((editor, monacoApi) => {
    if (!dawnRegistered) {
      monacoApi.languages.register({ id: "dawn", extensions: [".dawn", ".effect.dawn"] });
      dawnRegistered = true;
    }
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
          path={activeEditor?.path ?? "dawn://empty"}
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
