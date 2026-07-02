import { useState } from "react";

import type { AppSnapshot, GuiDocument } from "../../types";

import type { GuiFocus, ReadyGuiDocument, SequenceSelection } from "./shared";

import { LayoutCanvas } from "./layout/LayoutCanvas";

import { FixtureCanvas } from "./fixture/FixtureCanvas";

import { GuiInspector } from "./GuiInspector";

import { BlockedGui } from "./BlockedGui";

import { SequenceEditor } from "./sequence/SequenceEditor";
import { useAppStore } from "../../store";

import { handleSequencePlaybackShortcut, isSequenceTransportUnsupported } from "./sequence/SequenceTransportControls";

import { markSelectionConsumesKey } from "./sequence/sequenceSelection";

export function GuiEditor({
  guiDocument,
  snapshot,
  audioTransport,
  sequenceSelection,
  setSequenceSelection,
  resetRevision
}: {
  guiDocument: GuiDocument | null;
  snapshot: AppSnapshot;
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
      audioTransport={audioTransport}
      sequenceSelection={sequenceSelection}
      setSequenceSelection={setSequenceSelection}
    />
  );
}

function GuiEditorInner({
  gui,
  audioTransport,
  sequenceSelection,
  setSequenceSelection
}: {
  gui: ReadyGuiDocument;
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

  return (
    <div
      className="gui-editor-shell"
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
          sequenceSelection={sequenceSelection}
          setSequenceSelection={setSequenceSelection}
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
      <GuiInspector
        gui={gui}
        selected={selected}
        setSelected={setSelected}
        sequenceSelection={sequenceSelection}
        activeMarkCollectionKey={activeMarkCollectionKey}
        setActiveMarkCollectionKey={setActiveMarkCollectionKey}
        visibleMarkCollectionKeys={visibleMarkCollectionKeys}
        setVisibleMarkCollectionKeys={setVisibleMarkCollectionKeys}
      />
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
