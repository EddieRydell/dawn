import type { IDockviewHeaderActionsProps, IDockviewPanelHeaderProps } from "dockview-react";
import { Maximize2, Minimize2, MoreHorizontal, X } from "lucide-react";
import { useEffect, useState } from "react";
import { useWorkbench } from "../store/workbenchStore";

export function DockTab({ api }: IDockviewPanelHeaderProps) {
  if (api.id === "editor") {
    return <EditorDockTabs />;
  }

  return <StandardDockTab api={api} />;
}

function StandardDockTab({ api }: { api: IDockviewPanelHeaderProps["api"] }) {
  const [title, setTitle] = useState(api.title ?? "");

  useEffect(() => {
    const disposable = api.onDidTitleChange((event) => setTitle(event.title));
    return () => disposable.dispose();
  }, [api]);

  return (
    <div className="dock-tab">
      <span className="dock-tab-title">{title}</span>
    </div>
  );
}

function EditorDockTabs() {
  const activeFile = useWorkbench((state) => state.activeFile);
  const openEditors = useWorkbench((state) => state.openEditors);
  const activateFile = useWorkbench((state) => state.activateFile);
  const closeFile = useWorkbench((state) => state.closeFile);

  if (openEditors.length === 0) {
    return (
      <div className="dock-tab editor-dock-tabs empty">
        <span className="dock-tab-title">No file open</span>
      </div>
    );
  }

  return (
    <div className="editor-dock-tabs" role="tablist" aria-label="Open editors">
      {openEditors.map((editor) => (
        <button
          key={editor.path}
          className={editor.path === activeFile ? "editor-dock-file-tab active" : "editor-dock-file-tab"}
          type="button"
          role="tab"
          aria-selected={editor.path === activeFile}
          title={editor.path}
          onClick={(event) => {
            event.stopPropagation();
            activateFile(editor.path);
          }}
          onAuxClick={(event) => {
            if (event.button === 1) {
              event.preventDefault();
              event.stopPropagation();
              void closeFile(editor.path);
            }
          }}
        >
          <span className="editor-dock-file-tab-label">{editor.dirty ? "* " : ""}{leafName(editor.path)}</span>
          <span
            className="editor-dock-file-tab-close"
            role="button"
            tabIndex={-1}
            title="Close editor"
            onClick={(event) => {
              event.stopPropagation();
              void closeFile(editor.path);
            }}
          >
            <X size={13} />
          </span>
        </button>
      ))}
    </div>
  );
}

function leafName(path: string) {
  const parts = path.split(/[\\/]/).filter(Boolean);
  return parts[parts.length - 1] ?? path;
}

export function DockHeaderActions({ api, activePanel }: IDockviewHeaderActionsProps) {
  const [maximized, setMaximized] = useState(api.isMaximized());

  function toggleMaximized() {
    if (api.isMaximized()) {
      api.exitMaximized();
      setMaximized(false);
    } else {
      api.maximize();
      setMaximized(true);
    }
  }

  return (
    <div className="dock-header-actions">
      <button title="Panel options" onClick={(event) => event.stopPropagation()}>
        <MoreHorizontal size={13} />
      </button>
      <button title={maximized ? "Restore panel group" : "Maximize panel group"} onClick={toggleMaximized}>
        {maximized ? <Minimize2 size={13} /> : <Maximize2 size={13} />}
      </button>
      <button title="Close active panel" disabled={!activePanel} onClick={() => activePanel?.api.close()}>
        <X size={13} />
      </button>
    </div>
  );
}
