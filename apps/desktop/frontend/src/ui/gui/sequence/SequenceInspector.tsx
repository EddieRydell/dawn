import { useState } from "react";

import { Pencil, Plus, Trash2 } from "lucide-react";
import { THEME_COLORS, THEME_METRICS } from "../../../theme";

import type {
  SequenceEditorDocument,
  SequenceEffect,
  SequenceMarkCollection,
  SequenceMarkRef,
  SequenceEffectScope,
  SequenceEffectDefinition
} from "../../../types";
import { commands } from "../../../api";
import { runGuiEditCommand } from "../../../store";
import { ColorPicker } from "../../ColorPicker";
import { InspectorScrollArea, Readout } from "../InspectorScrollArea";
import { roundToNanosecond, type AutomationClipChooser, type GuiFocus, type SequenceSelection } from "../shared";
import { TypedParamInput } from "./params/TypedParamInput";
import { defaultMarkColor, nextCollectionKey } from "./marks";
import { selectedEffectId, selectionCompatibleWithFocusedItem, selectionCount } from "./sequenceSelection";
import { targetsEqual } from "./sequenceTargets";

type SequenceInspectorTab = "effect" | "controls" | "layers" | "marks";

type SelectedMarkEntry = {
  ref: SequenceMarkRef;
  collection: SequenceMarkCollection;
  timeSeconds: number;
};

const SEQUENCE_INSPECTOR_TABS: { id: SequenceInspectorTab; label: string }[] = [
  { id: "effect", label: "Effect" },
  { id: "controls", label: "Controls" },
  { id: "layers", label: "Layers" },
  { id: "marks", label: "Marks" }
];

function selectedEffectDefinitionValue(effect: SequenceEffect, definitions: SequenceEffectDefinition[]) {
  const index = definitions.findIndex((definition) => effectReferencesEqual(definition.effect, effect.effectReference));
  return index < 0 ? "" : String(index);
}

function effectReferencesEqual(left: SequenceEffectDefinition["effect"], right: SequenceEffectDefinition["effect"]) {
  if (left.type !== right.type) return false;
  return left.type === "builtin"
    ? left.effect === (right.type === "builtin" ? right.effect : undefined)
    : left.path === (right.type === "custom" ? right.path : undefined) && left.effectName === (right.type === "custom" ? right.effectName : undefined);
}

function effectReferenceKey(reference: SequenceEffectDefinition["effect"]) {
  return reference.type === "builtin" ? `builtin:${reference.effect}` : `${reference.path}:${reference.effectName}`;
}

function defaultLayerColor(index: number) {
  const colors = [THEME_COLORS.graphBlue, THEME_COLORS.graphRed, THEME_COLORS.graphGreen, THEME_COLORS.graphYellow, THEME_COLORS.graphPurple, THEME_COLORS.graphPink];
  return colors[index % colors.length] ?? THEME_COLORS.graphBlue;
}

export function SequenceInspector({
  document,
  selected,
  setSelected,
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
  selected: GuiFocus;
  setSelected: (id: GuiFocus) => void;
  sequenceSelection: SequenceSelection;
  setSequenceSelection: (selection: SequenceSelection) => void;
  automationClipChooser: AutomationClipChooser;
  setAutomationClipChooser: (chooser: AutomationClipChooser) => void;
  activeMarkCollectionKey: string | null;
  setActiveMarkCollectionKey: (key: string | null) => void;
  visibleMarkCollectionKeys: Set<string>;
  setVisibleMarkCollectionKeys: (keys: Set<string>) => void;
}) {
  const [activeTab, setActiveTab] = useState<SequenceInspectorTab>("effect");

  const footer = (
    <div className="sequence-inspector-tabs" role="tablist" aria-label="Sequence inspector sections">
      {SEQUENCE_INSPECTOR_TABS.map((tab) => (
        <button
          key={tab.id}
          type="button"
          role="tab"
          aria-selected={activeTab === tab.id}
          className={activeTab === tab.id ? "active" : ""}
          onClick={() => {
            setActiveTab(tab.id);
          }}
        >
          {tab.label}
        </button>
      ))}
    </div>
  );

  return (
    <InspectorScrollArea footer={footer}>
      {activeTab === "effect" && (
        <EffectInspectorPanel
          document={document}
          selected={selected}
          setSelected={setSelected}
          sequenceSelection={sequenceSelection}
          automationClipChooser={automationClipChooser}
          setAutomationClipChooser={setAutomationClipChooser}
        />
      )}
      {activeTab === "layers" && <LayerInspectorPanel document={document} />}
      {activeTab === "controls" && <ControlClipPanel document={document} />}
      {activeTab === "marks" && (
        <MarkInspectorPanel
          document={document}
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
    </InspectorScrollArea>
  );
}

function ControlClipPanel({ document }: { document: SequenceEditorDocument }) {
  return (
    <>
      <h2>Typed Controls</h2>
      {document.controlClips.length === 0 && <p>No scalar, indexed, or fixture-function controls.</p>}
      {document.controlClips.map((clip) => (
        <div className="control-clip-inspector" key={clip.id}>
          <strong>{clip.targetLabel}</strong>
          <span>{clip.controlType} · {clip.value}</span>
          <label>Start
            <input type="number" min={0} step={0.01} defaultValue={clip.startSeconds} onBlur={(event) => void runGuiEditCommand(() => commands.applySequenceGuiEdit({
              type: "moveControlClip",
              id: clip.id,
              startSeconds: Number(event.currentTarget.value),
              anchorLaneIndex: clip.anchorLaneIndex,
              laneIndex: clip.laneIndex
            }))} />
          </label>
          <label>Duration
            <input type="number" min={0.001} step={0.01} defaultValue={clip.durationSeconds} onBlur={(event) => void runGuiEditCommand(() => commands.applySequenceGuiEdit({
              type: "resizeControlClip",
              id: clip.id,
              startSeconds: clip.startSeconds,
              durationSeconds: Number(event.currentTarget.value)
            }))} />
          </label>
          <button type="button" onClick={() => void runGuiEditCommand(() => commands.applySequenceGuiEdit({ type: "deleteControlClip", id: clip.id }))}>
            <Trash2 size={THEME_METRICS.iconSizeExtraSmall} /> Delete
          </button>
        </div>
      ))}
    </>
  );
}

function EffectInspectorPanel({
  document,
  selected,
  setSelected,
  sequenceSelection,
  automationClipChooser,
  setAutomationClipChooser
}: {
  document: SequenceEditorDocument;
  selected: GuiFocus;
  setSelected: (id: GuiFocus) => void;
  sequenceSelection: SequenceSelection;
  automationClipChooser: AutomationClipChooser;
  setAutomationClipChooser: (chooser: AutomationClipChooser) => void;
}) {
  const id = selectedEffectId(selected);
  const effect = document.effects.find((candidate) => candidate.id === id);

  if (sequenceSelection !== null && selectionCount(sequenceSelection) > 1 && selectionCompatibleWithFocusedItem(sequenceSelection, selected)) {
    if (sequenceSelection.type !== "effects") {
      return (
        <>
          <h2>Effect Parameters</h2>
          <p>Select an effect on the timeline.</p>
        </>
      );
    }
    return (
      <>
        <h2>Effects</h2>
        <div className="inspector-readout-grid">
          <Readout label="Selected" value={String(selectionCount(sequenceSelection))} />
        </div>
        <button
          type="button"
          onClick={() =>
            void runGuiEditCommand(() => commands.applySequenceSelectionEdit({ type: "delete", selection: sequenceSelection })).then(() => {
              setSelected(null);
            })
          }
        >
          Delete
        </button>
      </>
    );
  }

  if (effect === undefined) {
    return (
      <>
        <h2>Effect Parameters</h2>
        <p>Select an effect on the timeline.</p>
      </>
    );
  }

  const currentScriptValue = selectedEffectDefinitionValue(effect, document.effectDefinitions);
  const resizeEffect = (startSeconds: number, durationSeconds: number) =>
    runGuiEditCommand(() =>
      commands.applySequenceGuiEdit({
        type: "resizeEffect",
        id: effect.id,
        startSeconds: Math.max(0, roundToNanosecond(startSeconds)),
        durationSeconds: Math.max(0.000000001, roundToNanosecond(durationSeconds))
      })
    );

  return (
    <>
      <h2>Effect Parameters</h2>
      <div className="effect-inspector-fields">
        <div className="inspector-readout-grid">
          <Readout label="ID" value={String(effect.id)} />
        </div>
        <label>
          Layer
          <select
            value={String(effect.layerId)}
            onChange={(event) =>
              void runGuiEditCommand(() =>
                commands.applySequenceGuiEdit({
                  type: "setEffectLayer",
                  id: effect.id,
                  layerId: Number(event.currentTarget.value)
                })
              )
            }
          >
            {document.layers.map((layer) => (
              <option key={layer.id} value={String(layer.id)}>
                {layer.name}
              </option>
            ))}
          </select>
        </label>
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
        <label>
          Effect type
          <select
            value={currentScriptValue}
            disabled={document.effectDefinitions.length === 0}
            onChange={(event) => {
              const definition = document.effectDefinitions[Number(event.currentTarget.value)]?.effect;
              if (definition === undefined) return;
              void runGuiEditCommand(() =>
                commands.applySequenceGuiEdit({
                  type: "changeEffectDefinition",
                  id: effect.id,
                  effect: definition
                })
              );
            }}
          >
            {currentScriptValue === "" && <option value="">{effect.effect}</option>}
            {document.effectDefinitions.map((definition, index) => (
              <option key={effectReferenceKey(definition.effect)} value={String(index)}>
                {definition.name}
              </option>
            ))}
          </select>
        </label>
      </div>
      <label>
        Scope
        <select
          value={effect.scope}
          onChange={(event) =>
            void runGuiEditCommand(() =>
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
        <>
          <div className="inspector-section-divider" />
          <div className="effect-param-section">
            <h3>Parameters</h3>
            {effect.params.map((param, index) => (
              <div
                key={`${effect.id}:${param.name}`}
                className={`effect-param-row ${index % 2 === 0 ? "effect-param-row-even" : "effect-param-row-odd"}`}
              >
                <TypedParamInput
                  param={param}
                  commitParam={(name, value) =>
                    runGuiEditCommand(() =>
                      commands.applySequenceGuiEdit({
                        type: "updateEffectParam",
                        id: effect.id,
                        name,
                        value
                      })
                    ).then(() => undefined)
                  }
                  curveLibrary={document.curveLibrary}
                  gradientLibrary={document.gradientLibrary}
                  markCollections={document.markCollections}
                  linkCurve={(name, curve) =>
                    runGuiEditCommand(() =>
                      commands.applySequenceGuiEdit({
                        type: "linkEffectCurve",
                        id: effect.id,
                        name,
                        sourcePath: curve.path,
                        objectKey: curve.objectKey
                      })
                    ).then(() => undefined)
                  }
                  unlinkCurve={(name) =>
                    runGuiEditCommand(() =>
                      commands.applySequenceGuiEdit({
                        type: "unlinkEffectCurve",
                        id: effect.id,
                        name
                      })
                    ).then(() => undefined)
                  }
                  linkGradient={(name, gradient) =>
                    runGuiEditCommand(() =>
                      commands.applySequenceGuiEdit({
                        type: "linkEffectGradient",
                        id: effect.id,
                        name,
                        sourcePath: gradient.path,
                        objectKey: gradient.objectKey
                      })
                    ).then(() => undefined)
                  }
                  unlinkGradient={(name) =>
                    runGuiEditCommand(() =>
                      commands.applySequenceGuiEdit({
                        type: "unlinkEffectGradient",
                        id: effect.id,
                        name
                      })
                    ).then(() => undefined)
                  }
                  automation={{
                    effectId: effect.id,
                    effectStartSeconds: effect.startSeconds,
                    effectDurationSeconds: effect.durationSeconds,
                    automationClips: document.automationClips,
                    canCreateAutomationClip: document.lanes.some((lane) => targetsEqual(lane.target, effect.target)),
                    automationClipChooser,
                    setAutomationClipChooser
                  }}
                />
              </div>
            ))}
          </div>
        </>
      )}
    </>
  );
}

function LayerInspectorPanel({ document }: { document: SequenceEditorDocument }) {
  return (
    <>
      <h2>Layers</h2>
      <button
        type="button"
        className="neutral-button"
        onClick={() =>
          void runGuiEditCommand(() =>
            commands.applySequenceGuiEdit({
              type: "createLayer",
              name: `Layer ${document.layers.length + 1}`,
              color: defaultLayerColor(document.layers.length)
            })
          )
        }
      >
        Add layer
      </button>
      <div className="sequence-layer-list">
        {document.layers.map((layer) => (
          <div key={layer.id} className="sequence-layer-row">
            <input
              type="checkbox"
              checked={layer.enabled}
              aria-label={`${layer.name} enabled`}
              onChange={(event) =>
                void runGuiEditCommand(() =>
                  commands.applySequenceGuiEdit({
                    type: "setLayerEnabled",
                    id: layer.id,
                    enabled: event.currentTarget.checked
                  })
                )
              }
            />
            <ColorPicker
              value={layer.color}
              label={`${layer.name} color`}
              commit={(color) =>
                runGuiEditCommand(() =>
                  commands.applySequenceGuiEdit({
                    type: "setLayerColor",
                    id: layer.id,
                    color
                  })
                ).then(() => undefined)
              }
            />
            <input
              key={`${layer.id}:name:${layer.name}`}
              defaultValue={layer.name}
              aria-label="Layer name"
              onBlur={(event) => {
                const name = event.currentTarget.value.trim() || layer.name;
                if (name === layer.name) return;
                void runGuiEditCommand(() =>
                  commands.applySequenceGuiEdit({
                    type: "renameLayer",
                    id: layer.id,
                    name
                  })
                );
              }}
            />
            {!layer.isDefault && (
              <button
                type="button"
                onClick={() => {
                  const effectCount = document.effects.filter((effect) => effect.layerId === layer.id).length;
                  if (effectCount > 0 && !window.confirm(`Delete ${layer.name} and move ${effectCount} effects to Default?`)) return;
                  void runGuiEditCommand(() =>
                    commands.applySequenceGuiEdit({
                      type: "deleteLayer",
                      id: layer.id,
                      migrateToLayerId: 0
                    })
                  );
                }}
              >
                Delete
              </button>
            )}
          </div>
        ))}
      </div>
    </>
  );
}

function MarkInspectorPanel({
  document,
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
  selected: GuiFocus;
  setSelected: (id: GuiFocus) => void;
  sequenceSelection: SequenceSelection;
  setSequenceSelection: (selection: SequenceSelection) => void;
  activeMarkCollectionKey: string | null;
  setActiveMarkCollectionKey: (key: string | null) => void;
  visibleMarkCollectionKeys: Set<string>;
  setVisibleMarkCollectionKeys: (keys: Set<string>) => void;
}) {
  const [editingCollectionKey, setEditingCollectionKey] = useState<string | null>(null);
  const selectedMark = selected?.type === "mark" ? { collectionKey: selected.collectionKey, index: selected.index } : null;
  const activeCollection = document.markCollections.find((collection) => collection.key === activeMarkCollectionKey) ?? document.markCollections[0] ?? null;
  const selectedMarks = selectedMarkEntries(document, selected, sequenceSelection);

  const createCollection = () => {
    const name = "Marks";
    const key = nextCollectionKey(name, document.markCollections);
    void runGuiEditCommand(() =>
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

  const deleteCollection = (collection: SequenceMarkCollection) => {
    if (collection.marksSeconds.length > 0 && !window.confirm(`Delete ${collection.name} and ${collection.marksSeconds.length} marks?`)) return;
    void runGuiEditCommand(() =>
      commands.applySequenceGuiEdit({
        type: "deleteMarkCollection",
        key: collection.key
      })
    ).then(() => {
      if (selectedMark?.collectionKey === collection.key) setSelected(null);
      if (activeCollection?.key === collection.key) {
        setActiveMarkCollectionKey(document.markCollections.find((candidate) => candidate.key !== collection.key)?.key ?? null);
      }
      setVisibleMarkCollectionKeys(new Set([...visibleMarkCollectionKeys].filter((key) => key !== collection.key)));
    });
  };

  const setSelectedMarkRefs = (refs: SequenceMarkRef[]) => {
    const validRefs = refs.filter((ref) => markEntry(document, ref) !== null);
    if (validRefs.length === 0) {
      setSelected(null);
      setSequenceSelection(null);
      return;
    }
    const firstRef = validRefs[0];
    if (firstRef === undefined) return;
    setSelected({ type: "mark", collectionKey: firstRef.collectionKey, index: firstRef.index });
    setSequenceSelection({ type: "marks", marks: validRefs });
  };

  const moveSelectedMark = (entry: SelectedMarkEntry, timeSeconds: number) => {
    const nextTimeSeconds = roundToNanosecond(Math.max(0, timeSeconds));
    if (!Number.isFinite(nextTimeSeconds) || nextTimeSeconds === entry.timeSeconds) return;
    const nextRefs = selectedRefsAfterMove(selectedMarks.map((mark) => mark.ref), entry.collection, entry.ref, nextTimeSeconds);
    void runGuiEditCommand(() =>
      commands.applySequenceGuiEdit({
        type: "moveMark",
        collectionKey: entry.ref.collectionKey,
        index: entry.ref.index,
        timeSeconds: nextTimeSeconds
      })
    ).then(() => {
      setSelectedMarkRefs(nextRefs);
    });
  };

  const reassignSelectedMark = (entry: SelectedMarkEntry, targetCollectionKey: string) => {
    if (targetCollectionKey === entry.ref.collectionKey) return;
    const targetCollection = document.markCollections.find((collection) => collection.key === targetCollectionKey);
    if (targetCollection === undefined) return;
    const nextRefs = selectedRefsAfterReassign(selectedMarks.map((mark) => mark.ref), entry.ref, targetCollection, entry.timeSeconds);
    void runGuiEditCommand(() =>
      commands.applySequenceGuiEdit({
        type: "reassignMarkCollection",
        collectionKey: entry.ref.collectionKey,
        index: entry.ref.index,
        targetCollectionKey
      })
    ).then(() => {
      setSelectedMarkRefs(nextRefs);
    });
  };

  const deleteSelectedMark = (entry: SelectedMarkEntry) => {
    const nextRefs = selectedRefsAfterDelete(selectedMarks.map((mark) => mark.ref), entry.ref);
    void runGuiEditCommand(() =>
      commands.applySequenceGuiEdit({
        type: "deleteMark",
        collectionKey: entry.ref.collectionKey,
        index: entry.ref.index
      })
    ).then(() => {
      setSelectedMarkRefs(nextRefs);
    });
  };

  return (
    <>
      <h2>Marks</h2>
      <label>
        Active collection
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
      <div className="mark-section">
        <h3>Collections</h3>
        <button type="button" className="neutral-button icon-text-button" onClick={createCollection}>
          <Plus size={THEME_METRICS.iconSizeSmall} />
          Add collection
        </button>
        {document.markCollections.length > 0 && (
          <div className="mark-collection-edit-list">
            {document.markCollections.map((collection) => (
              <div key={collection.key} className="mark-collection-edit-row">
                <input
                  type="radio"
                  name="active-mark-collection"
                  checked={activeCollection?.key === collection.key}
                  aria-label={`Use ${collection.name} for new marks`}
                  onChange={() => {
                    setActiveMarkCollectionKey(collection.key);
                  }}
                />
                <input
                  type="checkbox"
                  checked={visibleMarkCollectionKeys.has(collection.key)}
                  aria-label={`Show ${collection.name}`}
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
                <ColorPicker
                  value={collection.color}
                  label={`${collection.name} color`}
                  commit={(color) =>
                    runGuiEditCommand(() =>
                      commands.applySequenceGuiEdit({
                        type: "setMarkCollectionColor",
                        key: collection.key,
                        color
                      })
                    ).then(() => undefined)
                  }
                />
                {editingCollectionKey === collection.key ? (
                  <input
                    key={`${collection.key}:edit-name`}
                    autoFocus
                    defaultValue={collection.name}
                    aria-label="Collection name"
                    onBlur={(event) => {
                      const name = event.currentTarget.value.trim() || collection.name;
                      setEditingCollectionKey(null);
                      if (name === collection.name) return;
                      void runGuiEditCommand(() =>
                        commands.applySequenceGuiEdit({
                          type: "renameMarkCollection",
                          key: collection.key,
                          name
                        })
                      );
                    }}
                  />
                ) : (
                  <button
                    type="button"
                    className="mark-collection-name-button"
                    onClick={() => {
                      setActiveMarkCollectionKey(collection.key);
                    }}
                  >
                    {collection.name}
                  </button>
                )}
                <button
                  type="button"
                  className="icon-button neutral-icon-button"
                  title="Edit collection name"
                  aria-label={`Edit ${collection.name}`}
                  onClick={() => {
                    setEditingCollectionKey(collection.key);
                  }}
                >
                  <Pencil size={THEME_METRICS.iconSizeSmall} />
                </button>
                <button
                  type="button"
                  className="icon-button danger-icon-button"
                  title="Delete collection"
                  aria-label={`Delete ${collection.name}`}
                  onClick={() => {
                    deleteCollection(collection);
                  }}
                >
                  <Trash2 size={THEME_METRICS.iconSizeSmall} />
                </button>
              </div>
            ))}
          </div>
        )}
      </div>
      {selectedMarks.length > 0 ? (
        <div className="mark-section">
          <h3>{selectedMarks.length === 1 ? "Selected Mark" : "Selected Marks"}</h3>
          <div className="mark-selected-list">
            {selectedMarks.map((entry, index) => (
              <div key={`${entry.ref.collectionKey}:${entry.ref.index}:${index}`} className="mark-selected-row">
                <select
                  value={entry.ref.collectionKey}
                  aria-label="Selected mark collection"
                  onChange={(event) => {
                    reassignSelectedMark(entry, event.currentTarget.value);
                  }}
                >
                  {document.markCollections.map((collection) => (
                    <option key={collection.key} value={collection.key}>{collection.name}</option>
                  ))}
                </select>
                <input
                  key={`${entry.ref.collectionKey}:${entry.ref.index}:${entry.timeSeconds}`}
                  type="number"
                  min={0}
                  step="any"
                  defaultValue={formatMarkTimeInput(entry.timeSeconds)}
                  aria-label="Selected mark time"
                  onBlur={(event) => {
                    moveSelectedMark(entry, Number(event.currentTarget.value));
                  }}
                />
                <button
                  type="button"
                  className="icon-button danger-icon-button"
                  title="Delete mark"
                  aria-label="Delete mark"
                  onClick={() => {
                    deleteSelectedMark(entry);
                  }}
                >
                  <Trash2 size={THEME_METRICS.iconSizeSmall} />
                </button>
              </div>
            ))}
          </div>
        </div>
      ) : (
        <p>Select a mark on the timeline.</p>
      )}
    </>
  );
}

function markIndexAfterInsert(collection: SequenceMarkCollection, timeSeconds: number) {
  return collection.marksSeconds.filter((markTimeSeconds) => markTimeSeconds <= timeSeconds).length;
}

function selectedMarkEntries(
  document: SequenceEditorDocument,
  selected: GuiFocus,
  sequenceSelection: SequenceSelection
): SelectedMarkEntry[] {
  const refs =
    sequenceSelection?.type === "marks" && sequenceSelection.marks.length > 0
      ? sequenceSelection.marks
      : selected?.type === "mark"
        ? [{ collectionKey: selected.collectionKey, index: selected.index }]
        : [];
  const seen = new Set<string>();
  const entries: SelectedMarkEntry[] = [];
  for (const ref of refs) {
    const key = markRefKey(ref);
    if (seen.has(key)) continue;
    seen.add(key);
    const entry = markEntry(document, ref);
    if (entry !== null) entries.push(entry);
  }
  return entries;
}

function markEntry(document: SequenceEditorDocument, ref: SequenceMarkRef): SelectedMarkEntry | null {
  const collection = document.markCollections.find((candidate) => candidate.key === ref.collectionKey);
  const timeSeconds = collection?.marksSeconds[ref.index];
  if (collection === undefined || timeSeconds === undefined) return null;
  return { ref, collection, timeSeconds };
}

function selectedRefsAfterMove(
  refs: SequenceMarkRef[],
  collection: SequenceMarkCollection,
  movedRef: SequenceMarkRef,
  timeSeconds: number
) {
  const sorted = collection.marksSeconds
    .map((markTimeSeconds, markIndex) => ({
      markIndex,
      timeSeconds: markIndex === movedRef.index ? timeSeconds : markTimeSeconds
    }))
    .sort((left, right) => left.timeSeconds - right.timeSeconds || left.markIndex - right.markIndex);
  return refs.map((ref) => {
    if (ref.collectionKey !== movedRef.collectionKey) return ref;
    const nextIndex = sorted.findIndex((mark) => mark.markIndex === ref.index);
    return nextIndex < 0 ? ref : { collectionKey: ref.collectionKey, index: nextIndex };
  });
}

function selectedRefsAfterReassign(
  refs: SequenceMarkRef[],
  movedRef: SequenceMarkRef,
  targetCollection: SequenceMarkCollection,
  timeSeconds: number
) {
  const targetIndex = markIndexAfterInsert(targetCollection, timeSeconds);
  return refs.map((ref) => {
    if (sameMarkRef(ref, movedRef)) return { collectionKey: targetCollection.key, index: targetIndex };
    if (ref.collectionKey === movedRef.collectionKey && ref.index > movedRef.index) {
      return { collectionKey: ref.collectionKey, index: ref.index - 1 };
    }
    if (ref.collectionKey === targetCollection.key && ref.index >= targetIndex) {
      return { collectionKey: ref.collectionKey, index: ref.index + 1 };
    }
    return ref;
  });
}

function selectedRefsAfterDelete(refs: SequenceMarkRef[], deletedRef: SequenceMarkRef) {
  return refs
    .filter((ref) => !sameMarkRef(ref, deletedRef))
    .map((ref) => {
      if (ref.collectionKey === deletedRef.collectionKey && ref.index > deletedRef.index) {
        return { collectionKey: ref.collectionKey, index: ref.index - 1 };
      }
      return ref;
    });
}

function sameMarkRef(left: SequenceMarkRef, right: SequenceMarkRef) {
  return left.collectionKey === right.collectionKey && left.index === right.index;
}

function markRefKey(ref: SequenceMarkRef) {
  return `${ref.collectionKey}:${ref.index}`;
}

function formatMarkTimeInput(timeSeconds: number) {
  return timeSeconds.toFixed(9).replace(/\.?0+$/, "");
}
