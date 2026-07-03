import type {
  SequenceEditorDocument,
  SequenceEffect,
  SequenceEffectParam,
  SequenceEffectParamValue,
  SequenceEffectScope,
  SequenceEffectScript,
  SequenceGraphOperatorParam
} from "../../../types";
import { commands } from "../../../api";
import { runGuiEditCommand } from "../../../store";
import { InspectorScrollArea, Readout } from "../InspectorScrollArea";
import { formatSeconds, roundToNanosecond, type AutomationClipChooser, type GuiFocus, type SequenceSelection } from "../shared";
import { ColorField, EffectParamInput } from "./params/EffectParamInput";
import { defaultMarkColor, nextCollectionKey } from "./marks";
import { selectedEffectId, selectionCompatibleWithFocusedItem, selectionCount } from "./sequenceSelection";
import { targetsEqual } from "./sequenceTargets";

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
  openGraphClipId,
  setOpenGraphClipId,
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
  openGraphClipId: number | null;
  setOpenGraphClipId: (id: number | null) => void;
  sequenceSelection: SequenceSelection;
  automationClipChooser: AutomationClipChooser;
  setAutomationClipChooser: (chooser: AutomationClipChooser) => void;
  activeMarkCollectionKey: string | null;
  setActiveMarkCollectionKey: (key: string | null) => void;
  visibleMarkCollectionKeys: Set<string>;
  setVisibleMarkCollectionKeys: (keys: Set<string>) => void;
}) {
const id = selectedEffectId(selected);
    const effect = document.effects.find((candidate) => candidate.id === id);
    const selectedGraphClip = selected?.type === "graphNode" ? document.graphClips.find((clip) => clip.id === selected.clipId) ?? null : null;
    const selectedGraphNode =
      selected?.type === "graphNode" ? selectedGraphClip?.nodes.find((node) => node.id === selected.nodeId) ?? null : null;
    const selectedAutomationClip = selected?.type === "automationClip" ? document.automationClips.find((clip) => clip.id === selected.id) : undefined;
    const selectedMark = selected?.type === "mark" ? { collectionKey: selected.collectionKey, index: selected.index } : null;
    const selectedMarkCollection = selectedMark === null ? null : document.markCollections.find((collection) => collection.key === selectedMark.collectionKey) ?? null;
    const activeCollection = document.markCollections.find((collection) => collection.key === activeMarkCollectionKey) ?? document.markCollections[0] ?? null;
    const selectedMarkTime = selectedMarkCollection?.marksSeconds[selectedMark?.index ?? -1];
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
              void runGuiEditCommand(() => commands.applySequenceSelectionEdit({ type: "delete", selection: sequenceSelection })).then(() => {
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
        </InspectorScrollArea>
      );
    }
    if (selected?.type === "graphNode" && selectedGraphClip !== null && selectedGraphNode !== null) {
      const incoming = selectedGraphClip.edges.filter((edge) => edge.toNode === selectedGraphNode.id).length;
      const outgoing = selectedGraphClip.edges.filter((edge) => edge.fromNode === selectedGraphNode.id).length;
      const deleteNode = () =>
        runGuiEditCommand(() =>
          commands.applySequenceGuiEdit({
            type: "deleteGraphNode",
            clipId: selectedGraphClip.id,
            nodeId: selectedGraphNode.id
          })
        ).then(() => {
          setSelected({ type: "effect", id: selectedGraphClip.id });
        });
      if (selectedGraphNode.kind.type === "source") {
        const source = selectedGraphNode.kind;
        const currentScriptValue = graphSourceScriptValue(source.scriptSource, document.effectScripts);
        return (
          <InspectorScrollArea>
            <h2>Graph Source</h2>
            <div className="inspector-readout-grid">
              <Readout label="Clip" value={String(selectedGraphClip.id)} />
              <Readout label="Node" value={String(selectedGraphNode.id)} />
              <Readout label="Target" value={source.targetLabel} />
              <Readout label="Scope" value={scopeLabel(source.scope)} />
              <Readout label="Start" value={formatSeconds(source.startSeconds)} />
              <Readout label="Duration" value={formatSeconds(source.durationSeconds)} />
              <Readout label="Outputs" value={String(outgoing)} />
            </div>
            <label>
              Source
              <select
                value={currentScriptValue}
                disabled={document.effectScripts.filter((script) => script.kind === "sample").length === 0}
                onChange={(event) => {
                  const script = document.effectScripts.filter((candidate) => candidate.kind === "sample")[Number(event.currentTarget.value)]?.script;
                  if (script === undefined) return;
                  void runGuiEditCommand(() =>
                    commands.applySequenceGuiEdit({
                      type: "changeGraphSourceScript",
                      clipId: selectedGraphClip.id,
                      nodeId: selectedGraphNode.id,
                      script
                    })
                  );
                }}
              >
                {currentScriptValue === "" && <option value="">{source.script}</option>}
                {document.effectScripts.filter((script) => script.kind === "sample").map((script, index) => (
                  <option key={`${script.script.path}:${script.script.effectName}`} value={String(index)}>
                    {script.name}
                  </option>
                ))}
              </select>
            </label>
            {source.params.length > 0 && (
              <GraphParamSection
                params={source.params}
                document={document}
                commitParam={(name, value) =>
                  runGuiEditCommand(() =>
                    commands.applySequenceGuiEdit({
                      type: "updateGraphSourceParam",
                      clipId: selectedGraphClip.id,
                      nodeId: selectedGraphNode.id,
                      name,
                      value
                    })
                  ).then(() => undefined)
                }
              />
            )}
            <button type="button" onClick={() => void deleteNode()}>
              Delete node
            </button>
          </InspectorScrollArea>
        );
      }
      if (selectedGraphNode.kind.type === "operator") {
        const operator = selectedGraphNode.kind;
        return (
          <InspectorScrollArea>
            <h2>Graph Operator</h2>
            <div className="inspector-readout-grid">
              <Readout label="Clip" value={String(selectedGraphClip.id)} />
              <Readout label="Node" value={String(selectedGraphNode.id)} />
              <Readout label="Operator" value={operatorLabel(operator.operator)} />
              <Readout label="Inputs" value={String(incoming)} />
              <Readout label="Outputs" value={String(outgoing)} />
            </div>
            {operator.params.length > 0 && (
              <GraphParamSection
                params={operator.params.map(graphOperatorParamToEffectParam)}
                document={document}
                commitParam={(name, value) =>
                  runGuiEditCommand(() =>
                    commands.applySequenceGuiEdit({
                      type: "updateGraphOperatorParam",
                      clipId: selectedGraphClip.id,
                      nodeId: selectedGraphNode.id,
                      name,
                      value
                    })
                  ).then(() => undefined)
                }
              />
            )}
            <button type="button" onClick={() => void deleteNode()}>
              Delete node
            </button>
          </InspectorScrollArea>
        );
      }
      return (
        <InspectorScrollArea>
          <h2>Graph Output</h2>
          <div className="inspector-readout-grid">
            <Readout label="Clip" value={String(selectedGraphClip.id)} />
            <Readout label="Node" value={String(selectedGraphNode.id)} />
            <Readout label="Inputs" value={String(incoming)} />
            <Readout label="Outputs" value={String(outgoing)} />
          </div>
          <p>This node returns the graph clip output.</p>
        </InspectorScrollArea>
      );
    }
    if (effect !== undefined) {
      const graphClip = effect.kind === "graph" ? document.graphClips.find((clip) => clip.id === effect.id) ?? null : null;
      if (graphClip !== null) {
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
            <InspectorScrollArea>
              <h2>Graph</h2>
              <div className="effect-inspector-fields">
                <div className="inspector-readout-grid">
                  <Readout label="ID" value={String(effect.id)} />
                  <Readout label="Nodes" value={String(graphClip.nodes.length)} />
                  <Readout label="Edges" value={String(graphClip.edges.length)} />
                </div>
                <div className="inspector-inline-row">
                  <label>
                    Start
                    <input
                      key={`${effect.id}:graph-start:${effect.startSeconds}`}
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
                      key={`${effect.id}:graph-duration:${effect.durationSeconds}`}
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
                <button
                  type="button"
                  className="neutral-button"
                  onClick={() => {
                    setOpenGraphClipId(openGraphClipId === graphClip.id ? null : graphClip.id);
                  }}
                >
                  {openGraphClipId === graphClip.id ? "Close graph" : "Open graph"}
                </button>
                <button
                  type="button"
                  className="neutral-button"
                  onClick={() => {
                    setOpenGraphClipId(null);
                    setSelected(null);
                  }}
                >
                  Show timeline
                </button>
              </div>
            </InspectorScrollArea>
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
        <InspectorScrollArea>
          <h2>Effect</h2>
          <div className="effect-inspector-fields">
            <div className="inspector-readout-grid">
              <Readout label="ID" value={String(effect.id)} />
            </div>
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
        </InspectorScrollArea>
      );
    }
    if (selectedAutomationClip !== undefined) {
      return (
        <InspectorScrollArea>
          <h2>Automation</h2>
          <div className="inspector-readout-grid">
            <Readout label="ID" value={String(selectedAutomationClip.id)} />
            <Readout label="Start" value={formatSeconds(selectedAutomationClip.startSeconds)} />
            <Readout label="Duration" value={formatSeconds(selectedAutomationClip.durationSeconds)} />
            <Readout label="Bindings" value={String(selectedAutomationClip.bindings.length)} />
          </div>
          <button
            type="button"
            onClick={() =>
              void runGuiEditCommand(() =>
                commands.applySequenceGuiEdit({ type: "deleteAutomationClip", id: selectedAutomationClip.id })
              ).then(() => {
                setSelected(null);
              })
            }
          >
            Delete
          </button>
        </InspectorScrollArea>
      );
    }
    return (
      <InspectorScrollArea>
        <h2>Sequence</h2>
        {document.lanes[0] !== undefined && (
          <div className="mark-section">
            <h3>Graphs</h3>
            <button
              type="button"
              className="neutral-button"
              onClick={() => {
                const firstLane = document.lanes[0];
                if (firstLane === undefined) return;
                void runGuiEditCommand(() =>
                  commands.applySequenceGuiEdit({
                    type: "addGraphClip",
                    target: firstLane.target,
                    scope: "wholeTarget",
                    startSeconds: 0,
                    durationSeconds: Math.min(5, Math.max(0.000000001, document.durationSeconds))
                  })
                );
              }}
            >
              Add graph
            </button>
          </div>
        )}
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
        <p>Select a mark or effect.</p>
      </InspectorScrollArea>
    );
}

function GraphParamSection({
  params,
  document,
  commitParam
}: {
  params: SequenceEffectParam[];
  document: SequenceEditorDocument;
  commitParam: (name: string, value: SequenceEffectParamValue) => Promise<void>;
}) {
  return (
    <>
      <div className="inspector-section-divider" />
      <div className="effect-param-section">
        <h3>Parameters</h3>
        {params.map((param, index) => (
          <div
            key={param.name}
            className={`effect-param-row ${index % 2 === 0 ? "effect-param-row-even" : "effect-param-row-odd"}`}
          >
            <EffectParamInput
              param={param}
              commitParam={commitParam}
              curveLibrary={document.curveLibrary}
              markCollections={document.markCollections}
            />
          </div>
        ))}
      </div>
    </>
  );
}

function graphSourceScriptValue(
  currentScript: SequenceEffectScript["script"] | null,
  scripts: SequenceEffectScript[]
) {
  if (currentScript === null) return "";
  const sampleScripts = scripts.filter((script) => script.kind === "sample");
  const index = sampleScripts.findIndex((script) => scriptsEqual(script.script, currentScript));
  return index < 0 ? "" : String(index);
}

function graphOperatorParamToEffectParam(param: SequenceGraphOperatorParam): SequenceEffectParam {
  return {
    name: param.name,
    kind: param.kind,
    options: [],
    editable: true,
    value: param.value,
    curveSource: null,
    automation: null
  };
}

function scopeLabel(scope: SequenceEffectScope) {
  return scope === "perFixture" ? "Per fixture" : "Whole target";
}

function operatorLabel(operator: string) {
  return operator.replace(/[A-Z]/g, (match) => ` ${match}`).replace(/^./, (match) => match.toUpperCase());
}
