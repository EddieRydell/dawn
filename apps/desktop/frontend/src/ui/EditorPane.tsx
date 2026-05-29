import { keymap } from "@codemirror/view";
import { defaultKeymap } from "@codemirror/commands";
import { yaml } from "@codemirror/lang-yaml";
import { EditorState } from "@codemirror/state";
import { EditorView, ViewUpdate } from "@codemirror/view";
import { X } from "lucide-react";
import { useEffect, useRef } from "react";
import { commands } from "../api";
import type { AppSnapshotDto } from "../bindings";
import { commandRegistry } from "../commandRegistry";
import { runSnapshotCommand, useAppStore } from "../store";
import { GuiEditor, SequenceTransportControls } from "./GuiEditor";

export function EditorPane({ snapshot }: { snapshot: AppSnapshotDto }) {
  const { localText, setLocalText } = useAppStore();
  const editorHost = useRef<HTMLDivElement | null>(null);
  const view = useRef<EditorView | null>(null);
  const applyingExternalText = useRef(false);
  const activePath = snapshot.activeBuffer?.path ?? null;
  const viewMode = snapshot.activeBuffer?.viewMode ?? "text";
  const activeSequenceDocument =
    viewMode === "gui" && snapshot.activeGuiDocument?.type === "sequence" ? snapshot.activeGuiDocument.document : null;

  useEffect(() => {
    if (viewMode !== "text") {
      view.current?.destroy();
      view.current = null;
      return;
    }
    if (!editorHost.current || view.current) return;
    view.current = new EditorView({
      parent: editorHost.current,
      state: createState(localText, (update) => {
        if (!update.docChanged) return;
        if (applyingExternalText.current) return;
        const text = update.state.doc.toString();
        setLocalText(text);
        scheduleAutosave(text);
      }, async (text, redo) => {
        window.clearTimeout(autosaveTimer);
        if (!redo) {
          await runSnapshotCommand(() => commands.updateActiveText(text));
          await runSnapshotCommand(commands.undoActiveEdit);
        } else {
          await runSnapshotCommand(commands.redoActiveEdit);
        }
      })
    });
    return () => {
      view.current?.destroy();
      view.current = null;
    };
  }, [localText, setLocalText, viewMode]);

  useEffect(() => {
    if (!view.current) return;
    if (viewMode !== "text") return;
    const current = view.current.state.doc.toString();
    if (current !== localText) {
      applyingExternalText.current = true;
      view.current.dispatch({
        changes: { from: 0, to: current.length, insert: localText }
      });
      applyingExternalText.current = false;
    }
  }, [activePath, localText, viewMode]);

  if (snapshot.tabs.length === 0) {
    return (
      <section className="editor-shell empty-editor">
        <span>{snapshot.projectRoot !== null ? "Open a Dawn file from the project tree." : "Open a project to start."}</span>
      </section>
    );
  }

  return (
    <section className="editor-shell">
      <div className="tab-strip">
        {snapshot.tabs.map((tab) => (
          <button
            key={tab.path}
            className={`tab ${tab.path === snapshot.activeFile ? "active" : ""}`}
            onClick={() => void runSnapshotCommand(() => commands.setActiveFile(tab.path))}
          >
            <span>{tab.name}</span>
            {tab.dirty && <span className="dirty-dot" />}
            <X
              size={14}
              onClick={(event) => {
                event.stopPropagation();
                void runSnapshotCommand(() => commands.closeFile(tab.path));
              }}
            />
          </button>
        ))}
      </div>
      <div className="editor-toolbar">
        {activeSequenceDocument !== null && (
          <SequenceTransportControls document={activeSequenceDocument} preview={snapshot.preview} />
        )}
        <div className="segmented-control">
          <button
            className={viewMode === "text" ? "active" : ""}
            onClick={() => void runSnapshotCommand(() => commands.setActiveViewMode("text"))}
          >
            Text
          </button>
          <button
            className={viewMode === "gui" ? "active" : ""}
            onClick={() => void runSnapshotCommand(() => commands.setActiveViewMode("gui"))}
          >
            GUI
          </button>
        </div>
      </div>
      {viewMode === "gui" ? <GuiEditor snapshot={snapshot} /> : <div ref={editorHost} className="editor-host" />}
    </section>
  );
}

let autosaveTimer: number | undefined;

function scheduleAutosave(text: string) {
  window.clearTimeout(autosaveTimer);
  autosaveTimer = window.setTimeout(() => {
    void runSnapshotCommand(() => commands.updateActiveText(text));
  }, 450);
}

function createState(
  text: string,
  onUpdate: (update: ViewUpdate) => void,
  onHistoryCommand: (text: string, redo: boolean) => Promise<void>
) {
  return EditorState.create({
    doc: text,
    extensions: [
      yaml(),
      keymap.of([
        {
          key: "Mod-s",
          run: () => {
            void commandRegistry["file.save"].run();
            return true;
          }
        },
        {
          key: "Mod-z",
          run: (view) => {
            void onHistoryCommand(view.state.doc.toString(), false);
            return true;
          }
        },
        {
          key: "Mod-Shift-z",
          run: () => {
            void onHistoryCommand("", true);
            return true;
          }
        },
        ...defaultKeymap
      ]),
      EditorView.updateListener.of(onUpdate),
      EditorView.theme({
        "&": { height: "100%", backgroundColor: "#17181b", color: "#ebe7df" },
        ".cm-scroller": { fontFamily: "Consolas, 'SFMono-Regular', monospace", fontSize: "13px" },
        ".cm-content": { caretColor: "#6abf8a" },
        ".cm-cursor": { borderLeftColor: "#6abf8a" },
        ".cm-selectionBackground": { backgroundColor: "#31543f !important" },
        ".cm-gutters": { backgroundColor: "#1d1f23", color: "#77736d", borderRight: "1px solid #373b42" }
      })
    ]
  });
}
