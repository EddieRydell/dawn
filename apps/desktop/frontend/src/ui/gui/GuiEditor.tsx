import { useEffect, useState } from "react";

import type { AppSnapshot, GuiDocument, WorkspaceLayoutState } from "../../types";

import type { AutomationClipChooser, GuiFocus, ReadyGuiDocument, SequenceSelection } from "./shared";

import { LayoutCanvas } from "./layout/LayoutCanvas";

import { FixtureCanvas } from "./fixture/FixtureCanvas";

import { GuiInspector } from "./GuiInspector";

import { BlockedGui } from "./BlockedGui";

import { SequenceEditor } from "./sequence/SequenceEditor";
import { useAppStore } from "../../store";

import { handleSequencePlaybackShortcut, isSequenceTransportUnsupported } from "./sequence/SequenceTransportControls";

import { markSelectionConsumesKey } from "./sequence/sequenceSelection";
import { WorkspaceResizeHandle } from "../WorkspaceResizeHandle";
import { OPEN_LAYER_GRAPH_EVENT } from "../uiEvents";

const INSPECTOR_MIN_WIDTH_PX = 240;
const INSPECTOR_MAX_WIDTH_PX = 560;

export function GuiEditor({
  guiDocument,
  snapshot,
  workspaceLayout,
  onWorkspaceLayoutChange,
  audioTransport,
  sequenceSelection,
  setSequenceSelection,
  resetRevision
}: {
  guiDocument: GuiDocument | null;
  snapshot: AppSnapshot;
  workspaceLayout: WorkspaceLayoutState;
  onWorkspaceLayoutChange: (layout: WorkspaceLayoutState) => void;
  audioTransport: AppSnapshot["audioTransport"];
  sequenceSelection: SequenceSelection;
  setSequenceSelection: (selection: SequenceSelection) => void;
  resetRevision: number;
}) {
  const gui = guiDocument;

  if (!gui) {
    return <BlockedGui reason="GUI data is not available for this document." diagnostics={[]} />;
  }
  if (gui.type === "blocked") {
    return <BlockedGui reason={gui.reason} diagnostics={gui.diagnostics} />;
  }

  const editorKey = `${guiEditorKey(snapshot.activeFile, gui)}:${resetRevision}`;
  return (
    <GuiEditorInner
      key={editorKey}
      gui={gui}
      workspaceLayout={workspaceLayout}
      onWorkspaceLayoutChange={onWorkspaceLayoutChange}
      audioTransport={audioTransport}
      sequenceSelection={sequenceSelection}
      setSequenceSelection={setSequenceSelection}
    />
  );
}

function GuiEditorInner({
  gui,
  workspaceLayout,
  onWorkspaceLayoutChange,
  audioTransport,
  sequenceSelection,
  setSequenceSelection
}: {
  gui: ReadyGuiDocument;
  workspaceLayout: WorkspaceLayoutState;
  onWorkspaceLayoutChange: (layout: WorkspaceLayoutState) => void;
  audioTransport: AppSnapshot["audioTransport"];
  sequenceSelection: SequenceSelection;
  setSequenceSelection: (selection: SequenceSelection) => void;
}) {
  const restoreState = useAppStore((store) => store.restoreState);
  const sequenceRestore =
    gui.type === "sequence"
      ? restoreState?.sequenceViewports[`${gui.document.path}::${gui.document.objectKey}`]
      : undefined;
  const [selected, setSelected] = useState<GuiFocus>(null);
  const [compositionGraphOpen, setCompositionGraphOpen] = useState(false);
  const [automationClipChooser, setAutomationClipChooser] = useState<AutomationClipChooser>(null);
  const [activeMarkCollectionKey, setActiveMarkCollectionKey] = useState<string | null>(() =>
    gui.type === "sequence"
      ? sequenceRestore?.activeMarkCollectionKey ?? gui.document.markCollections[0]?.key ?? null
      : null
  );
  const [visibleMarkCollectionKeys, setVisibleMarkCollectionKeys] = useState<Set<string>>(() =>
    new Set(
      gui.type === "sequence" && sequenceRestore !== undefined
        ? sequenceRestore.visibleMarkCollectionKeys
        : gui.type === "sequence"
          ? gui.document.markCollections.map((collection) => collection.key)
          : []
    )
  );

  useEffect(() => {
    if (gui.type !== "sequence") return;
    const openLayerGraph = () => {
      setCompositionGraphOpen(true);
    };
    window.addEventListener(OPEN_LAYER_GRAPH_EVENT, openLayerGraph);
    return () => {
      window.removeEventListener(OPEN_LAYER_GRAPH_EVENT, openLayerGraph);
    };
  }, [gui.type]);

  return (
    <div
      className="gui-editor-shell"
      style={{
        gridTemplateColumns: workspaceLayout.inspectorCollapsed
          ? "minmax(0, 1fr) 8px"
          : `minmax(0, 1fr) ${workspaceLayout.inspectorWidthPx}px`
      }}
      onKeyDownCapture={(event) => {
        if (gui.type === "sequence" && !markSelectionConsumesKey(selected, event.key)) {
          handleSequencePlaybackShortcut(
            event,
            gui.document,
            audioTransport,
            isSequenceTransportUnsupported(gui.document, audioTransport)
          );
        }
      }}
    >
      {gui.type === "sequence" && (
        <SequenceEditor
          key={`${gui.document.path}:${gui.document.objectKey}`}
          document={gui.document}
          transport={audioTransport}
          selected={selected}
          setSelected={setSelected}
          compositionGraphOpen={compositionGraphOpen}
          setCompositionGraphOpen={setCompositionGraphOpen}
          sequenceSelection={sequenceSelection}
          setSequenceSelection={setSequenceSelection}
          automationClipChooser={automationClipChooser}
          setAutomationClipChooser={setAutomationClipChooser}
          activeMarkCollectionKey={activeMarkCollectionKey}
          setActiveMarkCollectionKey={setActiveMarkCollectionKey}
          visibleMarkCollectionKeys={visibleMarkCollectionKeys}
          setVisibleMarkCollectionKeys={setVisibleMarkCollectionKeys}
        />
      )}
      {gui.type === "layout" && <LayoutCanvas document={gui.document} selected={selected} setSelected={setSelected} />}
      {gui.type === "fixture" && (
        <FixtureCanvas document={gui.document} selected={selected} setSelected={setSelected} />
      )}
      <div className={`gui-inspector-resize-shell ${workspaceLayout.inspectorCollapsed ? "collapsed" : ""}`}>
        <WorkspaceResizeHandle
          ariaLabel="Resize inspector"
          collapsed={workspaceLayout.inspectorCollapsed}
          direction="right"
          min={INSPECTOR_MIN_WIDTH_PX}
          max={INSPECTOR_MAX_WIDTH_PX}
          value={workspaceLayout.inspectorWidthPx}
          onChange={(update) => {
            onWorkspaceLayoutChange({
              ...workspaceLayout,
              inspectorCollapsed: update.collapsed,
              inspectorWidthPx: update.width
            });
          }}
        />
        {!workspaceLayout.inspectorCollapsed && (
          <GuiInspector
            gui={gui}
            selected={selected}
            setSelected={setSelected}
            sequenceSelection={sequenceSelection}
            automationClipChooser={automationClipChooser}
            setAutomationClipChooser={setAutomationClipChooser}
            activeMarkCollectionKey={activeMarkCollectionKey}
            setActiveMarkCollectionKey={setActiveMarkCollectionKey}
            visibleMarkCollectionKeys={visibleMarkCollectionKeys}
            setVisibleMarkCollectionKeys={setVisibleMarkCollectionKeys}
          />
        )}
      </div>
    </div>
  );
}

function guiEditorKey(activeFile: string | null, gui: ReadyGuiDocument) {
  switch (gui.type) {
    case "sequence":
    case "layout":
      return `${activeFile ?? ""}:${gui.type}:${gui.document.path}:${gui.document.objectKey}`;
    case "fixture":
      return `${activeFile ?? ""}:${gui.type}:${gui.document.path}:${gui.document.selectedObjectKey ?? ""}`;
  }
}
