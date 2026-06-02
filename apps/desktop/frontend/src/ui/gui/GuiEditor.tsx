import { useState } from "react";

import type { AppSnapshotDto } from "../../bindings";

import type { ReadyGuiDocumentDto, SequenceSelection } from "./shared";

import { LayoutCanvas } from "./layout/LayoutCanvas";

import { FixtureCanvas } from "./fixture/FixtureCanvas";

import { GuiInspector } from "./GuiInspector";

import { BlockedGui } from "./BlockedGui";

import { SequenceEditor } from "./sequence/SequenceEditor";

import { useSequencePreview, handleSequencePlaybackShortcut } from "./sequence/SequenceTransportControls";

import { markSelectionConsumesKey } from "./sequence/sequenceSelection";

export function GuiEditor({
  snapshot,
  sequenceSelection,
  setSequenceSelection
}: {
  snapshot: AppSnapshotDto;
  sequenceSelection: SequenceSelection;
  setSequenceSelection: (selection: SequenceSelection) => void;
}) {
  const gui = snapshot.activeGuiDocument;

  if (!gui) {
    return <BlockedGui reason="GUI data is not available for this document." diagnostics={[]} />;
  }
  if (gui.type === "blocked") {
    return <BlockedGui reason={gui.reason} diagnostics={gui.diagnostics} />;
  }

  const editorKey = guiEditorKey(snapshot.activeFile, gui);
  return (
    <GuiEditorInner
      key={editorKey}
      gui={gui}
      snapshot={snapshot}
      sequenceSelection={sequenceSelection}
      setSequenceSelection={setSequenceSelection}
    />
  );
}

function GuiEditorInner({
  gui,
  snapshot,
  sequenceSelection,
  setSequenceSelection
}: {
  gui: ReadyGuiDocumentDto;
  snapshot: AppSnapshotDto;
  sequenceSelection: SequenceSelection;
  setSequenceSelection: (selection: SequenceSelection) => void;
}) {
  const [selected, setSelected] = useState<string | null>(null);
  const [activeMarkCollectionKey, setActiveMarkCollectionKey] = useState<string | null>(() =>
    gui.type === "sequence" ? gui.document.markCollections[0]?.key ?? null : null
  );
  const [visibleMarkCollectionKeys, setVisibleMarkCollectionKeys] = useState<Set<string>>(() =>
    new Set(gui.type === "sequence" ? gui.document.markCollections.map((collection) => collection.key) : [])
  );
  const livePreview = useSequencePreview(snapshot.preview);

  return (
    <div
      className="gui-editor-shell"
      onKeyDownCapture={(event) => {
        if (gui.type === "sequence" && !markSelectionConsumesKey(selected, event.key)) {
          handleSequencePlaybackShortcut(event, gui.document, livePreview, gui.document.durationSeconds <= 0);
        }
      }}
    >
      {gui.type === "sequence" && (
        <SequenceEditor
          key={`${gui.document.path}:${gui.document.objectKey}`}
          document={gui.document}
          preview={livePreview}
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

function guiEditorKey(activeFile: string | null, gui: ReadyGuiDocumentDto) {
  switch (gui.type) {
    case "sequence":
    case "layout":
      return `${activeFile ?? ""}:${gui.type}:${gui.document.path}:${gui.document.objectKey}`;
    case "fixture":
      return `${activeFile ?? ""}:${gui.type}:${gui.document.path}:${gui.document.selectedObjectKey ?? ""}`;
  }
}
