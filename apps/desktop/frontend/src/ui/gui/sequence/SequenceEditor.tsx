import type { KeyboardEvent } from "react";

import type { SequenceEditorDocument } from "../../../types";

import type { AudioTransportViewSnapshot, AutomationClipChooser, GuiFocus, SequenceSelection } from "../shared";

import { SequenceCanvas } from "./SequenceCanvas";
import { GraphEditorModal } from "./GraphEditorModal";
import { handleSequencePlaybackShortcut, isSequenceTransportUnsupported } from "./SequenceTransportControls";

export function SequenceEditor({
  document,
  transport,
  selected,
  setSelected,
  openGraphClipId,
  setOpenGraphClipId,
  sequenceSelection,
  setSequenceSelection,
  automationClipChooser,
  setAutomationClipChooser,
  activeMarkCollectionKey,
  setActiveMarkCollectionKey,
  visibleMarkCollectionKeys,
  setVisibleMarkCollectionKeys
}: {
  document: SequenceEditorDocument;
  transport: AudioTransportViewSnapshot;
  selected: GuiFocus;
  setSelected: (id: GuiFocus) => void;
  openGraphClipId: number | null;
  setOpenGraphClipId: (id: number | null) => void;
  sequenceSelection: SequenceSelection;
  setSequenceSelection: (selection: SequenceSelection) => void;
  automationClipChooser: AutomationClipChooser;
  setAutomationClipChooser: (chooser: AutomationClipChooser) => void;
  activeMarkCollectionKey: string | null;
  setActiveMarkCollectionKey: (key: string | null) => void;
  visibleMarkCollectionKeys: Set<string>;
  setVisibleMarkCollectionKeys: (keys: Set<string>) => void;
}) {
  const liveTransport = transport;
  const unsupported = isSequenceTransportUnsupported(document, liveTransport);
  const graphClip = openGraphClipId === null ? null : document.graphClips.find((clip) => clip.id === openGraphClipId) ?? null;
  const selectedGraphNodeId = selected?.type === "graphNode" && selected.clipId === graphClip?.id ? selected.nodeId : null;
  const handleKeyDown = (event: KeyboardEvent<HTMLDivElement>) => {
    if (event.key === "Escape" && graphClip !== null) {
      event.preventDefault();
      setOpenGraphClipId(null);
      if (selected?.type === "graphNode" && selected.clipId === graphClip.id) {
        setSelected({ type: "effect", id: graphClip.id });
      }
      return;
    }
    if (event.key === "Escape" && automationClipChooser !== null) {
      event.preventDefault();
      setAutomationClipChooser(null);
      return;
    }
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
        automationClipChooser={automationClipChooser}
        setAutomationClipChooser={setAutomationClipChooser}
        activeMarkCollectionKey={activeMarkCollectionKey}
        setActiveMarkCollectionKey={setActiveMarkCollectionKey}
        visibleMarkCollectionKeys={visibleMarkCollectionKeys}
        setVisibleMarkCollectionKeys={setVisibleMarkCollectionKeys}
      />
      {graphClip !== null && (
        <GraphEditorModal
          document={document}
          clip={graphClip}
          selectedNodeId={selectedGraphNodeId}
          setSelectedNodeId={(nodeId) => {
            setSelected(nodeId === null ? { type: "effect", id: graphClip.id } : { type: "graphNode", clipId: graphClip.id, nodeId });
          }}
          onClose={() => {
            setOpenGraphClipId(null);
            if (selected?.type === "graphNode" && selected.clipId === graphClip.id) {
              setSelected({ type: "effect", id: graphClip.id });
            }
          }}
        />
      )}
    </div>
  );
}
