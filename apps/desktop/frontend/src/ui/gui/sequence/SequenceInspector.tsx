import type { SequenceEditorDocument, SequenceEffect, SequenceEffectScope, SequenceEffectScript } from "../../../types";
import { commands } from "../../../api";
import { runSnapshotCommand } from "../../../store";
import { InspectorScrollArea, Readout } from "../InspectorScrollArea";
import { formatSeconds, roundToNanosecond, type GuiFocus, type SequenceSelection } from "../shared";
import { ColorField, EffectParamInput } from "./params/EffectParamInput";
import { defaultMarkColor, nextCollectionKey } from "./marks";
import { selectedEffectId, selectionCompatibleWithFocusedItem, selectionCount } from "./sequenceSelection";

function selectedEffectScriptValue(effect: SequenceEffect, scripts: SequenceEffectScript[]) {
  const currentScript = effect.scriptSource;
  if (currentScript === null) return "";
  const index = scripts.findIndex((script) => scriptsEqual(script.script, currentScript));
  return index < 0 ? "" : String(index);
}

function scriptsEqual(left: SequenceEffectScript["script"], right: SequenceEffectScript["script"]) {
  return left.path === right.path && left.effectName === right.effectName;
}

export function SequenceInspector({
  document,
  selected,
  setSelected,
  sequenceSelection,
  activeMarkCollectionKey,
  setActiveMarkCollectionKey,
  visibleMarkCollectionKeys,
  setVisibleMarkCollectionKeys
}: {
  document: SequenceEditorDocument;
  selected: GuiFocus;
  setSelected: (id: GuiFocus) => void;
  sequenceSelection: SequenceSelection;
  activeMarkCollectionKey: string | null;
  setActiveMarkCollectionKey: (key: string | null) => void;
  visibleMarkCollectionKeys: Set<string>;
  setVisibleMarkCollectionKeys: (keys: Set<string>) => void;
}) {
const id = selectedEffectId(selected);
    const effect = document.effects.find((candidate) => candidate.id === id);
    const selectedMark = selected?.type === "mark" ? { collectionKey: selected.collectionKey, index: selected.index } : null;
    const selectedMarkCollection = selectedMark === null ? null : document.markCollections.find((collection) => collection.key === selectedMark.collectionKey) ?? null;
    const activeCollection = document.markCollections.find((collection) => collection.key === activeMarkCollectionKey) ?? document.markCollections[0] ?? null;
    const selectedMarkTime = selectedMarkCollection?.marksSeconds[selectedMark?.index ?? -1];
    const createCollection = () => {
      const name = "Marks";
      const key = nextCollectionKey(name, document.markCollections);
      void runSnapshotCommand(() =>
        commands.applySequenceGuiEdit({
          type: "createMarkCollection",
          key,
          name,
          color: defaultMarkColor(document.markCollections.length)
        })
      ).then(() => {
        setActiveMarkCollectionKey(key);
        setVisibleMarkCollectionKeys(new Set([...visibleMarkCollectionKeys, key]));
      });
    };
    if (sequenceSelection !== null && selectionCount(sequenceSelection) > 1 && selectionCompatibleWithFocusedItem(sequenceSelection, selected)) {
      return (
        <InspectorScrollArea>
          <h2>{sequenceSelection.type === "effects" ? "Effects" : "Marks"}</h2>
          <div className="inspector-readout-grid">
            <Readout label="Selected" value={String(selectionCount(sequenceSelection))} />
          </div>
          <button
            type="button"
            onClick={() =>
              void commands.applySequenceSelectionEdit({ type: "delete", selection: sequenceSelection }).then(() => {
                setSelected(null);
              })
            }
          >
            Delete
          </button>
        </InspectorScrollArea>
      );
    }
    const deleteActiveCollection = () => {
      if (activeCollection === null) return;
      if (activeCollection.marksSeconds.length > 0 && !window.confirm(`Delete ${activeCollection.name} and ${activeCollection.marksSeconds.length} marks?`)) return;
      void runSnapshotCommand(() =>
        commands.applySequenceGuiEdit({
          type: "deleteMarkCollection",
          key: activeCollection.key
        })
      ).then(() => {
        setSelected(null);
        setActiveMarkCollectionKey(null);
      });
    };
    if (selectedMark !== null && selectedMarkCollection !== null && selectedMarkTime !== undefined) {
      return (
        <InspectorScrollArea>
          <h2>Mark</h2>
          <div className="inspector-readout-grid">
            <Readout label="Collection" value={selectedMarkCollection.name} />
            <Readout label="Time" value={formatSeconds(selectedMarkTime)} />
            <Readout label="Color" value={selectedMarkCollection.color} swatch={selectedMarkCollection.color} />
          </div>
          <button
            type="button"
            onClick={() =>
              void runSnapshotCommand(() =>
                commands.applySequenceGuiEdit({
                  type: "deleteMark",
                  collectionKey: selectedMark.collectionKey,
                  index: selectedMark.index
                })
              ).then(() => {
                setSelected(null);
              })
            }
          >
            Delete mark
          </button>
        </InspectorScrollArea>
      );
    }
    if (effect !== undefined) {
      const currentScriptValue = selectedEffectScriptValue(effect, document.effectScripts);
      const resizeEffect = (startSeconds: number, durationSeconds: number) =>
        runSnapshotCommand(() =>
          commands.applySequenceGuiEdit({
            type: "resizeEffect",
            id: effect.id,
            startSeconds: Math.max(0, roundToNanosecond(startSeconds)),
            durationSeconds: Math.max(0.000000001, roundToNanosecond(durationSeconds))
          })
        );
      return (
        <InspectorScrollArea>
          <h2>Effect</h2>
          <div className="inspector-readout-grid">
            <div className="inspector-inline-row">
              <label>
                Start
                <input
                  key={`${effect.id}:start:${effect.startSeconds}`}
                  type="number"
                  min={0}
                  step="any"
                  defaultValue={effect.startSeconds}
                  onBlur={(event) => {
                    const nextStartSeconds = Number(event.currentTarget.value);
                    if (!Number.isFinite(nextStartSeconds) || roundToNanosecond(nextStartSeconds) === effect.startSeconds) return;
                    void resizeEffect(nextStartSeconds, effect.durationSeconds);
                  }}
                />
              </label>
              <label>
                Duration
                <input
                  key={`${effect.id}:duration:${effect.durationSeconds}`}
                  type="number"
                  min={0.000000001}
                  step="any"
                  defaultValue={effect.durationSeconds}
                  onBlur={(event) => {
                    const nextDurationSeconds = Number(event.currentTarget.value);
                    if (!Number.isFinite(nextDurationSeconds) || roundToNanosecond(nextDurationSeconds) === effect.durationSeconds) return;
                    void resizeEffect(effect.startSeconds, nextDurationSeconds);
                  }}
                />
              </label>
            </div>
          </div>
          <label>
            Effect type
            <select
              value={currentScriptValue}
              disabled={document.effectScripts.length === 0}
              onChange={(event) => {
                const script = document.effectScripts[Number(event.currentTarget.value)]?.script;
                if (script === undefined) return;
                void runSnapshotCommand(() =>
                  commands.applySequenceGuiEdit({
                    type: "changeEffectScript",
                    id: effect.id,
                    script
                  })
                );
              }}
            >
              {currentScriptValue === "" && <option value="">{effect.script}</option>}
              {document.effectScripts.map((script, index) => (
                <option key={`${script.script.path}:${script.script.effectName}`} value={String(index)}>
                  {script.name}
                </option>
              ))}
            </select>
          </label>
          <label>
            Scope
            <select
              value={effect.scope}
              onChange={(event) =>
                void runSnapshotCommand(() =>
                  commands.applySequenceGuiEdit({
                    type: "setEffectScope",
                    id: effect.id,
                    scope: event.currentTarget.value as SequenceEffectScope
                  })
                )
              }
            >
              <option value="perFixture">Per fixture</option>
              <option value="wholeTarget">Whole target</option>
            </select>
          </label>
          {effect.params.length > 0 && (
            <div className="effect-param-section">
              <h3>Parameters</h3>
              {effect.params.map((param) => (
                <EffectParamInput
                  key={`${effect.id}:${param.name}`}
                  effectId={effect.id}
                  param={param}
                  curveLibrary={document.curveLibrary}
                  markCollections={document.markCollections}
                />
              ))}
            </div>
          )}
          <button onClick={() => void runSnapshotCommand(() => commands.applySequenceGuiEdit({ type: "deleteEffect", id: effect.id }))}>Delete</button>
        </InspectorScrollArea>
      );
    }
    return (
      <InspectorScrollArea>
        <h2>Sequence</h2>
        <div className="mark-section">
          <h3>Marks</h3>
          <button type="button" className="neutral-button" onClick={createCollection}>Add collection</button>
          {document.markCollections.length > 0 && (
            <>
              <label>
                Active
                <select
                  value={activeCollection?.key ?? ""}
                  onChange={(event) => {
                    setActiveMarkCollectionKey(event.currentTarget.value || null);
                  }}
                >
                  {document.markCollections.map((collection) => (
                    <option key={collection.key} value={collection.key}>{collection.name}</option>
                  ))}
                </select>
              </label>
              {activeCollection !== null && (
                <>
                  <label>
                    Name
                    <input
                      key={`${activeCollection.key}:name`}
                      defaultValue={activeCollection.name}
                      onBlur={(event) => {
                        const name = event.currentTarget.value.trim() || activeCollection.name;
                        if (name === activeCollection.name) return;
                        void runSnapshotCommand(() =>
                          commands.applySequenceGuiEdit({ type: "renameMarkCollection", key: activeCollection.key, name })
                        );
                      }}
                    />
                  </label>
                  <ColorField
                    key={`${activeCollection.key}:color:${activeCollection.color.toLowerCase()}`}
                    label="Color"
                    value={activeCollection.color}
                    commit={(color) =>
                      runSnapshotCommand(() =>
                        commands.applySequenceGuiEdit({
                          type: "setMarkCollectionColor",
                          key: activeCollection.key,
                          color
                        })
                      ).then(() => undefined)
                    }
                  />
                </>
              )}
              <div className="mark-visibility-list">
                {document.markCollections.map((collection) => (
                  <label key={collection.key} className="mark-collection-row">
                    <span className="color-swatch" style={{ background: collection.color }} />
                    <span>{collection.name}</span>
                    <input
                      type="checkbox"
                      checked={visibleMarkCollectionKeys.has(collection.key)}
                      onChange={(event) => {
                        const next = new Set(visibleMarkCollectionKeys);
                        if (event.currentTarget.checked) {
                          next.add(collection.key);
                        } else {
                          next.delete(collection.key);
                        }
                        setVisibleMarkCollectionKeys(next);
                      }}
                    />
                  </label>
                ))}
              </div>
              {activeCollection !== null && <button type="button" onClick={deleteActiveCollection}>Delete collection</button>}
            </>
          )}
        </div>
        <p>Select a mark or effect.</p>
      </InspectorScrollArea>
    );
}
