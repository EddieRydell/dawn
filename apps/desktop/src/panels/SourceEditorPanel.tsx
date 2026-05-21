import Editor, { loader } from "@monaco-editor/react";
import { Save, SearchCheck } from "lucide-react";
import * as monaco from "monaco-editor";
import { useMemo } from "react";
import { useWorkbench } from "../store/workbenchStore";

loader.config({ monaco });

export function SourceEditorPanel() {
  const activeFile = useWorkbench((state) => state.activeFile);
  const fileContent = useWorkbench((state) => state.fileContent);
  const dirty = useWorkbench((state) => state.dirty);
  const setFileContent = useWorkbench((state) => state.setFileContent);
  const saveFile = useWorkbench((state) => state.saveFile);
  const runCheck = useWorkbench((state) => state.runCheck);

  const language = useMemo(() => {
    if (activeFile?.endsWith(".vibe")) return "javascript";
    return "json";
  }, [activeFile]);

  return (
    <section className="editor-pane">
      <div className="toolbar">
        <span>{activeFile ? `${dirty ? "* " : ""}${activeFile}` : "No file open"}</span>
        <button title="Save" onClick={() => void saveFile()} disabled={!activeFile}>
          <Save size={17} />
        </button>
        <button title="Check project" onClick={() => void runCheck()}>
          <SearchCheck size={17} />
        </button>
      </div>
      <div className="workbench-content">
        <Editor
          height="100%"
          language={language}
          theme="vs-dark"
          value={fileContent}
          onChange={(value) => setFileContent(value ?? "")}
          options={{ minimap: { enabled: false }, fontSize: 13 }}
        />
      </div>
    </section>
  );
}
