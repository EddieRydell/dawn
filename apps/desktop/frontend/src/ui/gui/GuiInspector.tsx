import type { ReadyGuiDocumentDto, SequenceSelection } from "./shared";
import { FixtureInspector } from "./fixture/FixtureInspector";
import { LayoutInspector } from "./layout/LayoutInspector";
import { SequenceInspector } from "./sequence/SequenceInspector";

export function GuiInspector({
  gui,
  selected,
  setSelected,
  sequenceSelection,
  activeMarkCollectionKey,
  setActiveMarkCollectionKey,
  visibleMarkCollectionKeys,
  setVisibleMarkCollectionKeys
}: {
  gui: ReadyGuiDocumentDto;
  selected: string | null;
  setSelected: (id: string | null) => void;
  sequenceSelection: SequenceSelection;
  activeMarkCollectionKey: string | null;
  setActiveMarkCollectionKey: (key: string | null) => void;
  visibleMarkCollectionKeys: Set<string>;
  setVisibleMarkCollectionKeys: (keys: Set<string>) => void;
}) {
  if (gui.type === "sequence") {
    return (
      <SequenceInspector
        document={gui.document}
        selected={selected}
        setSelected={setSelected}
        sequenceSelection={sequenceSelection}
        activeMarkCollectionKey={activeMarkCollectionKey}
        setActiveMarkCollectionKey={setActiveMarkCollectionKey}
        visibleMarkCollectionKeys={visibleMarkCollectionKeys}
        setVisibleMarkCollectionKeys={setVisibleMarkCollectionKeys}
      />
    );
  }
  if (gui.type === "layout") return <LayoutInspector document={gui.document} selected={selected} />;
  return <FixtureInspector document={gui.document} selected={selected} />;
}
