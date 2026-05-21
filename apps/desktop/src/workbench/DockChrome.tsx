import type { IDockviewHeaderActionsProps, IDockviewPanelHeaderProps } from "dockview-react";
import { Maximize2, Minimize2, MoreHorizontal, X } from "lucide-react";
import { useEffect, useState } from "react";

export function DockTab({ api }: IDockviewPanelHeaderProps) {
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
