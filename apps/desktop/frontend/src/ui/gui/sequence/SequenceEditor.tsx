import type { KeyboardEvent } from "react";

import type { SequenceEditorDocument } from "../../../types";

import type { AudioTransportViewSnapshot, GuiFocus, SequenceSelection } from "../shared";

import { SequenceCanvas } from "./SequenceCanvas";
import { handleSequencePlaybackShortcut } from "./SequenceTransportControls";

export function SequenceEditor({
  document,
  transport,
  selected,
  setSelected,
  sequenceSelection,
  setSequenceSelection,
  activeMarkCollectionKey,
  setActiveMarkCollectionKey,
  visibleMarkCollectionKeys,
  setVisibleMarkCollectionKeys
}: {
  document: SequenceEditorDocument;
  transport: AudioTransportViewSnapshot;
  selected: GuiFocus;
  setSelected: (id: GuiFocus) => void;
  sequenceSelection: SequenceSelection;
  setSequenceSelection: (selection: SequenceSelection) => void;
  activeMarkCollectionKey: string | null;
  setActiveMarkCollectionKey: (key: string | null) => void;
  visibleMarkCollectionKeys: Set<string>;
  setVisibleMarkCollectionKeys: (keys: Set<string>) => void;
}) {
  const liveTransport = transport;
  const unsupported = document.durationSeconds <= 0;
  const handleKeyDown = (event: KeyboardEvent<HTMLDivElement>) => {
    handleSequencePlaybackShortcut(event, document, liveTransport, unsupported);
  };
  return (
    <div className="sequence-editor" tabIndex={-1} onKeyDown={handleKeyDown}>
      <SequenceCanvas
        document={document}
        playheadSeconds={liveTransport.positionSeconds}
        homeSeconds={liveTransport.homeSeconds}
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
