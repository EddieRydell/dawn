import type { KeyboardEvent } from "react";

import type { SequenceDocumentDto } from "../../../bindings";

import type { GuiFocus, LivePreview, SequenceSelection } from "../shared";

import { SequenceCanvas } from "./SequenceCanvas";
import { handleSequencePlaybackShortcut } from "./SequenceTransportControls";

export function SequenceEditor({
  document,
  preview,
  selected,
  setSelected,
  sequenceSelection,
  setSequenceSelection,
  activeMarkCollectionKey,
  setActiveMarkCollectionKey,
  visibleMarkCollectionKeys,
  setVisibleMarkCollectionKeys
}: {
  document: SequenceDocumentDto;
  preview: LivePreview;
  selected: GuiFocus;
  setSelected: (id: GuiFocus) => void;
  sequenceSelection: SequenceSelection;
  setSequenceSelection: (selection: SequenceSelection) => void;
  activeMarkCollectionKey: string | null;
  setActiveMarkCollectionKey: (key: string | null) => void;
  visibleMarkCollectionKeys: Set<string>;
  setVisibleMarkCollectionKeys: (keys: Set<string>) => void;
}) {
  const livePreview = preview;
  const unsupported = document.durationSeconds <= 0;
  const handleKeyDown = (event: KeyboardEvent<HTMLDivElement>) => {
    handleSequencePlaybackShortcut(event, document, livePreview, unsupported);
  };
  return (
    <div className="sequence-editor" tabIndex={-1} onKeyDown={handleKeyDown}>
      <SequenceCanvas
        document={document}
        previewPositionSeconds={livePreview.positionSeconds}
        previewHomeSeconds={livePreview.homeSeconds}
        selected={selected}
        setSelected={setSelected}
        sequenceSelection={sequenceSelection}
        setSequenceSelection={setSequenceSelection}
        activeMarkCollectionKey={activeMarkCollectionKey}
        setActiveMarkCollectionKey={setActiveMarkCollectionKey}
        visibleMarkCollectionKeys={visibleMarkCollectionKeys}
        setVisibleMarkCollectionKeys={setVisibleMarkCollectionKeys}
      />
    </div>
  );
}
