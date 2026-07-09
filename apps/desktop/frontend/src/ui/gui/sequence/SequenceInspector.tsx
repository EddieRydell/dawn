import { useState } from "react";

import type {
  SequenceEditorDocument,
  SequenceEffect,
  SequenceEffectScope,
  SequenceEffectScript
} from "../../../types";
import { commands } from "../../../api";
import { runGuiEditCommand } from "../../../store";
import { InspectorScrollArea, Readout } from "../InspectorScrollArea";
import { formatSeconds, roundToNanosecond, type AutomationClipChooser, type GuiFocus, type SequenceSelection } from "../shared";
import { ColorField, EffectParamInput } from "./params/EffectParamInput";
import { defaultMarkColor, nextCollectionKey } from "./marks";
import { selectedEffectId, selectionCompatibleWithFocusedItem, selectionCount } from "./sequenceSelection";
import { targetsEqual } from "./sequenceTargets";

type SequenceInspectorTab = "effect" | "layers" | "marks";

const SEQUENCE_INSPECTOR_TABS: { id: SequenceInspectorTab; label: string }[] = [
  { id: "effect", label: "Effect" },
  { id: "layers", label: "Layers" },
  { id: "marks", label: "Marks" }
];

function selectedEffectScriptValue(effect: SequenceEffect, scripts: SequenceEffectScript[]) {
  const currentScript = effect.scriptSource;
  if (currentScript === null) return "";
  const index = scripts.findIndex((script) => scriptsEqual(script.script, currentScript));
  return index < 0 ? "" : String(index);
}

function scriptsEqual(left: SequenceEffectScript["script"], right: SequenceEffectScript["script"]) {
  return left.path === right.path && left.effectName === right.effectName;
}

function defaultLayerColor(index: number) {
  const colors = ["#50a0ff", "#f45b69", "#37a987", "#f6b84b", "#9b6dff", "#e86fb0"];
  return colors[index % colors.length] ?? "#50a0ff";
}

export function SequenceInspector({
  document,
  selected,
  setSelected,
  sequenceSelection,
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
      {activeTab === "marks" && (
        <MarkInspectorPanel
          document={document}
          selected={selected}
          setSelected={setSelected}
          sequenceSelection={sequenceSelection}
          activeMarkCollectionKey={activeMarkCollectionKey}
          setActiveMarkCollectionKey={setActiveMarkCollectionKey}
          visibleMarkCollectionKeys={visibleMarkCollectionKeys}
          setVisibleMarkCollectionKeys={setVisibleMarkCollectionKeys}
        />
      )}
    </InspectorScrollArea>
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

  const currentScriptValue = selectedEffectScriptValue(effect, document.effectScripts);
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
            disabled={document.effectScripts.length === 0}
            onChange={(event) => {
              const script = document.effectScripts[Number(event.currentTarget.value)]?.script;
              if (script === undefined) return;
              void runGuiEditCommand(() =>
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
                <EffectParamInput
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
                  markCollections={document.markCollections}
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
            <input
              type="color"
              value={layer.color}
              aria-label={`${layer.name} color`}
              onChange={(event) =>
                void runGuiEditCommand(() =>
                  commands.applySequenceGuiEdit({
                    type: "setLayerColor",
                    id: layer.id,
                    color: event.currentTarget.value
                  })
                )
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
  const selectedMark = selected?.type === "mark" ? { collectionKey: selected.collectionKey, index: selected.index } : null;
  const selectedMarkCollection = selectedMark === null ? null : document.markCollections.find((collection) => collection.key === selectedMark.collectionKey) ?? null;
  const selectedMarkTime = selectedMarkCollection?.marksSeconds[selectedMark?.index ?? -1];
  const activeCollection = document.markCollections.find((collection) => collection.key === activeMarkCollectionKey) ?? document.markCollections[0] ?? null;

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

  const deleteActiveCollection = () => {
    if (activeCollection === null) return;
    if (activeCollection.marksSeconds.length > 0 && !window.confirm(`Delete ${activeCollection.name} and ${activeCollection.marksSeconds.length} marks?`)) return;
    void runGuiEditCommand(() =>
      commands.applySequenceGuiEdit({
        type: "deleteMarkCollection",
        key: activeCollection.key
      })
    ).then(() => {
      setSelected(null);
      setActiveMarkCollectionKey(null);
    });
  };

  return (
    <>
      <h2>Marks</h2>
      {sequenceSelection !== null && sequenceSelection.type === "marks" && selectionCount(sequenceSelection) > 1 && selectionCompatibleWithFocusedItem(sequenceSelection, selected) && (
        <div className="mark-section">
          <h3>Selection</h3>
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
        </div>
      )}
      {selectedMark !== null && selectedMarkCollection !== null && selectedMarkTime !== undefined && (
        <div className="mark-section">
          <h3>Selected Mark</h3>
          <div className="inspector-readout-grid">
            <Readout label="Collection" value={selectedMarkCollection.name} />
            <Readout label="Time" value={formatSeconds(selectedMarkTime)} />
            <Readout label="Color" value={selectedMarkCollection.color} swatch={selectedMarkCollection.color} />
          </div>
          <button
            type="button"
            onClick={() =>
              void runGuiEditCommand(() =>
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
        </div>
      )}
      <div className="mark-section">
        <h3>Collections</h3>
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
                      void runGuiEditCommand(() =>
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
                    runGuiEditCommand(() =>
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
      {selectedMark === null && <p>Select a mark on the timeline.</p>}
    </>
  );
}
