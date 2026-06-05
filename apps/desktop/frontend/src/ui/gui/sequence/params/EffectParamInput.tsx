import { useEffect, useRef, useState, type PointerEvent, type ReactNode } from "react";
import { ChevronRight, FlipHorizontal2, FlipVertical2, Link2Off, Minus } from "lucide-react";

import { commands } from "../../../../api";

import type { ColorCurvePointDto, FloatCurvePointDto, SequenceCurveLibraryItemDto, SequenceEffectParamDto, SequenceEffectParamValueDto, SequenceMarkCollectionDto } from "../../../../bindings";

import { runRuntimeCommand } from "../../../../store";

import { Readout } from "../../InspectorScrollArea";
import { clamp } from "../../shared";

const CURVE_EDITOR = {
  width: 240,
  height: 120,
  roundScale: 1000,
  flatRangeEpsilon: 0.0001,
  colorMismatchDistance: 0.001,
  emptyGradient: "#17181b",
  defaultColor: "#ffffff"
} as const;

export type EditedFloatCurvePoint = { time: number; value: number };

export type EditedColorCurvePoint = { time: number; value: string };

export function EffectParamInput({
  effectId,
  param,
  curveLibrary,
  markCollections
}: {
  effectId: number;
  param: SequenceEffectParamDto;
  curveLibrary: SequenceCurveLibraryItemDto[];
  markCollections: SequenceMarkCollectionDto[];
}) {
  const commit = (value: SequenceEffectParamValueDto) => {
    return runRuntimeCommand(() =>
      commands.applySequenceDocumentEdit({
        type: "updateEffectParam",
        id: effectId,
        name: param.name,
        value
      })
    ).then(() => undefined);
  };

  if (!param.editable) {
    return <Readout label={param.name} value="Unavailable" />;
  }

  switch (param.value.type) {
    case "int":
      return <NumberParam key={`${param.name}:${param.value.value}`} param={param} value={param.value.value} step={1} commit={(value) => commit({ type: "int", value: Math.max(0, Math.round(value)) })} />;
    case "float":
      return <NumberParam key={`${param.name}:${param.value.value}`} param={param} value={param.value.value} step={0.05} commit={(value) => commit({ type: "float", value })} />;
    case "bool":
      return (
        <label className="effect-param-check">
          <input
            type="checkbox"
            checked={param.value.value}
            onChange={(event) => void commit({ type: "bool", value: event.currentTarget.checked })}
          />
          <span>{param.name}</span>
        </label>
      );
    case "color":
      return <ColorField key={`${param.name}:${param.value.value.toLowerCase()}`} label={param.name} value={param.value.value} commit={(value) => commit({ type: "color", value })} />;
    case "enum":
      return (
        <label>
          {param.name}
          <select value={param.value.value} onChange={(event) => void commit({ type: "enum", value: event.currentTarget.value })}>
            {param.options.map((option) => <option key={option} value={option}>{option}</option>)}
          </select>
        </label>
      );
    case "flags": {
      const selectedFlags = param.value.value;
      return (
        <div className="effect-param-group">
          <div className="effect-param-name">{param.name}</div>
          {param.options.map((option) => {
            const checked = selectedFlags.includes(option);
            const nextValue = checked
              ? selectedFlags.filter((value: string) => value !== option)
              : [...selectedFlags, option];
            return (
              <label key={option} className="effect-param-check">
                <input
                  type="checkbox"
                  checked={checked}
                  onChange={() => void commit({ type: "flags", value: nextValue })}
                />
                <span>{option}</span>
              </label>
            );
          })}
        </div>
      );
    }
    case "floatCurve":
      return (
        <CurveParamSourceShell
          effectId={effectId}
          param={param}
          valueType="float"
          curveLibrary={curveLibrary}
          points={normalizeFloatCurvePoints(param.value.points)}
          commit={(points) => commit({ type: "floatCurve", points })}
          render={(props) => <FloatCurveParamShell name={param.name} {...props} />}
        />
      );
    case "colorCurve":
      return (
        <CurveParamSourceShell
          effectId={effectId}
          param={param}
          valueType="color"
          curveLibrary={curveLibrary}
          points={normalizeColorCurvePoints(param.value.points)}
          commit={(points) => commit({ type: "colorCurve", points })}
          render={(props) => <ColorCurveParamShell name={param.name} {...props} />}
        />
      );
    case "marks":
      return (
        <label>
          {param.name}
          <select value={param.value.key} onChange={(event) => void commit({ type: "marks", key: event.currentTarget.value })}>
            {markCollections.map((collection) => (
              <option key={collection.key} value={collection.key}>{collection.name}</option>
            ))}
          </select>
        </label>
      );
  }
}

function NumberParam({
  param,
  value,
  step,
  commit
}: {
  param: SequenceEffectParamDto;
  value: number;
  step: number;
  commit: (value: number) => Promise<void>;
}) {
  const [text, setText] = useState(String(value));
  const lastCommitted = useRef(value);
  const commitText = () => {
    const next = Number(text);
    if (!Number.isFinite(next)) {
      setText(String(value));
      return;
    }
    if (next !== lastCommitted.current) {
      lastCommitted.current = next;
      void commit(next);
    }
  };
  return (
    <label>
      {param.name}
      <input
        type="number"
        step={step}
        value={text}
        onChange={(event) => { setText(event.currentTarget.value); }}
        onBlur={commitText}
        onKeyDown={(event) => {
          if (event.key === "Enter") {
            commitText();
            event.currentTarget.blur();
          }
        }}
      />
    </label>
  );
}

export function ColorField({ label, value, commit }: { label: string; value: string; commit: (value: string) => Promise<void> }) {
  const committedValue = value.toLowerCase();
  const [draft, setDraft] = useState(committedValue);
  const lastCommitted = useRef(committedValue);
  const commitDraft = (candidate = draft) => {
    if (!isHexColor(candidate)) {
      setDraft(committedValue);
      return;
    }
    const next = candidate.toLowerCase();
    setDraft(next);
    if (next !== lastCommitted.current) {
      lastCommitted.current = next;
      void commit(next);
    }
  };
  const displayedColor = isHexColor(draft) ? draft : committedValue;
  return (
    <label>
      {label}
      <div className="effect-param-color">
        <span className="color-swatch" style={{ background: displayedColor }} />
        <input
          type="color"
          value={displayedColor}
          onChange={(event) => {
            setDraft(event.currentTarget.value);
          }}
          onBlur={() => { commitDraft(); }}
        />
        <input
          value={draft}
          onChange={(event) => { setDraft(event.currentTarget.value); }}
          onBlur={() => { commitDraft(); }}
          onKeyDown={(event) => {
            if (event.key === "Enter") {
              commitDraft();
              event.currentTarget.blur();
            }
          }}
        />
      </div>
    </label>
  );
}

function openColorPicker(input: HTMLInputElement | null | undefined) {
  if (input === null || input === undefined) return;
  input.showPicker();
}

type EditedCurvePoint = EditedFloatCurvePoint | EditedColorCurvePoint;

type CurveEditorProps<T extends EditedCurvePoint> = {
  points: T[];
  commit: (points: T[]) => Promise<void>;
  readOnly?: boolean;
  requestInlineEdit?: () => void;
};

function CurveParamSourceShell<T extends EditedCurvePoint>({
  effectId,
  param,
  valueType,
  curveLibrary,
  points,
  commit,
  render
}: {
  effectId: number;
  param: SequenceEffectParamDto;
  valueType: "float" | "color";
  curveLibrary: SequenceCurveLibraryItemDto[];
  points: T[];
  commit: (points: T[]) => Promise<void>;
  render: (props: CurveEditorProps<T>) => ReactNode;
}) {
  const source = param.curveSource?.type === "library" ? param.curveSource : null;
  const linked = source !== null;
  const linkedLabel = source?.displayName ?? source?.reference ?? "";
  const matchingCurves = curveLibrary.filter((item) => item.valueType === valueType);
  const selectedCurveIndex = linked && source.path !== null && source.objectKey !== null
    ? matchingCurves.findIndex((item) => item.path === source.path && item.objectKey === source.objectKey)
    : -1;
  const unlinkCopy = () =>
    runRuntimeCommand(() =>
      commands.applySequenceDocumentEdit({
        type: "unlinkEffectCurveParam",
        id: effectId,
        name: param.name
      })
    ).then(() => undefined);
  const confirmUnlinkCopy = () => {
    if (!linked) return true;
    if (!window.confirm(`Unlink ${param.name} and edit a local copy?`)) return false;
    void unlinkCopy();
    return true;
  };
  const flipHorizontal = () => {
    const next = sortCurvePoints(points.map((point) => ({ ...point, time: roundCurveValue(1 - point.time) }))) as T[];
    if (!linked || window.confirm(`Unlink ${param.name} and flip a local copy?`)) {
      void commit(next);
    }
  };
  const flipVertical = () => {
    if (valueType !== "float") return;
    const next = points.map((point) => ({ ...point, value: roundCurveValue(1 - (point.value as number)) })) as T[];
    if (!linked || window.confirm(`Unlink ${param.name} and flip a local copy?`)) {
      void commit(next);
    }
  };
  return (
    <div className={`curve-source-shell ${linked ? "linked" : ""}`}>
      <div className="curve-source-row">
        <select
          title={`${param.name} source`}
          value={linked ? "library" : "inline"}
          onChange={(event) => {
            if (event.currentTarget.value === "inline") {
              if (linked) void unlinkCopy();
              return;
            }
            const first = matchingCurves[0];
            if (first === undefined) return;
            void runRuntimeCommand(() =>
              commands.applySequenceDocumentEdit({
                type: "linkEffectCurveParam",
                id: effectId,
                name: param.name,
                curvePath: first.path,
                objectKey: first.objectKey
              })
            );
          }}
        >
          <option value="inline">Inline</option>
          <option value="library" disabled={matchingCurves.length === 0}>Library</option>
        </select>
        <select
          title={`${param.name} library curve`}
          disabled={matchingCurves.length === 0}
          value={String(selectedCurveIndex)}
          onChange={(event) => {
            const curve = matchingCurves[Number(event.currentTarget.value)];
            if (curve === undefined) return;
            void runRuntimeCommand(() =>
              commands.applySequenceDocumentEdit({
                type: "linkEffectCurveParam",
                id: effectId,
                name: param.name,
                curvePath: curve.path,
                objectKey: curve.objectKey
              })
            );
          }}
        >
          {!linked && <option value="-1">Choose curve</option>}
          {linked && selectedCurveIndex === -1 && (
            <option value="-1">{linkedLabel}</option>
          )}
          {matchingCurves.map((item, index) => (
            <option key={index} value={String(index)}>
              {item.displayName}
            </option>
          ))}
        </select>
      </div>
      {linked && <div className="curve-linked-label">{linkedLabel}</div>}
      <div className="curve-action-row">
        {linked && (
          <button type="button" className="neutral-button" title="Unlink copy" onClick={() => void unlinkCopy()}>
            <Link2Off size={14} />
          </button>
        )}
        <button type="button" className="neutral-button" title="Flip horizontal" onClick={flipHorizontal}>
          <FlipHorizontal2 size={14} />
        </button>
        {valueType === "float" && (
          <button type="button" className="neutral-button" title="Flip vertical" onClick={flipVertical}>
            <FlipVertical2 size={14} />
          </button>
        )}
      </div>
      {render({ points, commit, readOnly: linked, requestInlineEdit: confirmUnlinkCopy })}
    </div>
  );
}

function FloatCurveParamShell(props: {
  name: string;
  points: EditedFloatCurvePoint[];
  commit: (points: EditedFloatCurvePoint[]) => Promise<void>;
  readOnly?: boolean;
  requestInlineEdit?: () => void;
}) {
  return <FloatCurveParam {...props} />;
}

function ColorCurveParamShell(props: {
  name: string;
  points: EditedColorCurvePoint[];
  commit: (points: EditedColorCurvePoint[]) => Promise<void>;
  readOnly?: boolean;
  requestInlineEdit?: () => void;
}) {
  return <ColorCurveParam {...props} />;
}

function FloatCurveParam({
  name,
  points,
  commit,
  readOnly = false,
  requestInlineEdit
}: {
  name: string;
  points: EditedFloatCurvePoint[];
  commit: (points: EditedFloatCurvePoint[]) => Promise<void>;
  readOnly?: boolean;
  requestInlineEdit?: () => void;
}) {
  const [drafts, setDrafts] = useState(points);
  const draftsRef = useRef(points);
  const [selectedIndex, setSelectedIndex] = useState(0);
  const [pointsCollapsed, setPointsCollapsed] = useState(false);
  const svgRef = useRef<SVGSVGElement | null>(null);
  const draggingPoint = useRef<number | null>(null);
  const pointsSignature = curvePointsSignature(points);
  const lastCommittedSignature = useRef(pointsSignature);
  const pendingSignature = useRef<string | null>(null);
  useEffect(() => {
    draftsRef.current = drafts;
  }, [drafts]);
  useEffect(() => {
    const draftSignature = curvePointsSignature(draftsRef.current);
    if (pointsSignature === draftSignature) {
      pendingSignature.current = null;
      return;
    }
    if (pendingSignature.current === draftSignature) return;
    setDrafts(points);
    draftsRef.current = points;
    lastCommittedSignature.current = pointsSignature;
    setSelectedIndex((index) => Math.min(index, points.length - 1));
  }, [points, pointsSignature]);
  const update = (next: EditedFloatCurvePoint[]) => {
    if (readOnly) {
      requestInlineEdit?.();
      return;
    }
    if (next.length > 0 && next.every((point) => Number.isFinite(point.time) && Number.isFinite(point.value))) {
      const sorted = sortCurvePoints(next);
      const signature = curvePointsSignature(sorted);
      setDrafts(sorted);
      draftsRef.current = sorted;
      setSelectedIndex((index) => Math.min(index, sorted.length - 1));
      if (signature !== lastCommittedSignature.current) {
        lastCommittedSignature.current = signature;
        pendingSignature.current = signature;
        void commit(sorted).catch(() => {
          pendingSignature.current = null;
        });
      }
    }
  };
  const setPoint = (index: number, point: EditedFloatCurvePoint, commitChange: boolean) => {
    const next = sortCurvePoints(replaceAt(draftsRef.current, index, point));
    const nextIndex = nearestFloatPointIndex(next, point);
    setDrafts(next);
    draftsRef.current = next;
    setSelectedIndex(nextIndex);
    if (commitChange) {
      update(next);
    }
    return nextIndex;
  };
  const deletePoint = (index: number) => {
    if (draftsRef.current.length <= 1) return;
    const next = draftsRef.current.filter((_, pointIndex) => pointIndex !== index);
    update(next);
    setSelectedIndex(Math.min(index, next.length - 1));
  };
  const commitDraftPoint = (index: number) => {
    const point = draftsRef.current[index];
    if (!point) return;
    update(replaceAt(draftsRef.current, index, { time: clamp(point.time, 0, 1), value: point.value }));
  };
  const valueRange = floatCurveValueRange(drafts);
  const path = floatCurveSvgPath(drafts, valueRange);
  const pointFromPointer = (event: PointerEvent<SVGSVGElement>): EditedFloatCurvePoint => {
    const rect = svgRef.current?.getBoundingClientRect();
    if (rect === undefined) return { time: 0, value: 0 };
    const x = clamp((event.clientX - rect.left) / Math.max(1, rect.width), 0, 1);
    const y = clamp((event.clientY - rect.top) / Math.max(1, rect.height), 0, 1);
    return {
      time: roundCurveValue(x),
      value: roundCurveValue(valueRange.max - y * (valueRange.max - valueRange.min))
    };
  };
  return (
    <div className="effect-param-group float-curve-editor">
      <div className="effect-param-name">{name}</div>
      <svg
        ref={svgRef}
        className="float-curve-graph"
        viewBox={`0 0 ${CURVE_EDITOR.width} ${CURVE_EDITOR.height}`}
        role="img"
        aria-label={`${name} curve`}
        onPointerDown={(event) => {
          if (readOnly) {
            requestInlineEdit?.();
            return;
          }
          if (event.target instanceof SVGCircleElement) return;
          const point = pointFromPointer(event);
          update([...draftsRef.current, point]);
          setSelectedIndex(nearestFloatPointIndex(draftsRef.current, point));
        }}
        onPointerMove={(event) => {
          const index = draggingPoint.current;
          if (index === null) return;
          draggingPoint.current = setPoint(index, pointFromPointer(event), false);
        }}
        onPointerUp={(event) => {
          if (draggingPoint.current === null) return;
          event.currentTarget.releasePointerCapture(event.pointerId);
          draggingPoint.current = null;
          update(draftsRef.current);
        }}
        onPointerCancel={(event) => {
          if (draggingPoint.current === null) return;
          event.currentTarget.releasePointerCapture(event.pointerId);
          draggingPoint.current = null;
          update(draftsRef.current);
        }}
      >
        <rect className="float-curve-graph-bg" x="0" y="0" width={CURVE_EDITOR.width} height={CURVE_EDITOR.height} />
        <path className="float-curve-grid-line" d={`M0 ${CURVE_EDITOR.height / 2}H${CURVE_EDITOR.width}`} />
        <path className="float-curve-grid-line" d={`M${CURVE_EDITOR.width / 2} 0V${CURVE_EDITOR.height}`} />
        <path className="float-curve-line" d={path} />
        {drafts.map((point, index) => {
          const x = point.time * CURVE_EDITOR.width;
          const y = CURVE_EDITOR.height - ((point.value - valueRange.min) / (valueRange.max - valueRange.min)) * CURVE_EDITOR.height;
          return (
            <circle
              key={index}
              className={`float-curve-point ${index === selectedIndex ? "selected" : ""}`}
              cx={x}
              cy={y}
              r={index === selectedIndex ? 5 : 4}
              tabIndex={0}
              onPointerDown={(event) => {
                event.stopPropagation();
                if (readOnly) {
                  requestInlineEdit?.();
                  return;
                }
                event.currentTarget.ownerSVGElement?.setPointerCapture(event.pointerId);
                draggingPoint.current = index;
                setSelectedIndex(index);
              }}
              onContextMenu={(event) => {
                event.preventDefault();
                event.stopPropagation();
                if (readOnly) {
                  requestInlineEdit?.();
                  return;
                }
                deletePoint(index);
              }}
              onFocus={() => { setSelectedIndex(index); }}
            />
          );
        })}
      </svg>
      <div className="float-curve-points-panel">
        <button
          type="button"
          className="float-curve-points-toggle"
          onClick={() => { setPointsCollapsed((collapsed) => !collapsed); }}
        >
          {pointsCollapsed ? <ChevronRight size={13} /> : <ChevronRight className="expanded" size={13} />}
          <span>Points</span>
          <strong>{drafts.length}</strong>
        </button>
        {!pointsCollapsed && (
          <div className="float-curve-point-list">
            {drafts.map((point, index) => (
              <div
                key={`${index}:${point.time}:${point.value}`}
                className={`float-curve-point-row ${index === selectedIndex ? "selected" : ""}`}
                onPointerDown={() => { setSelectedIndex(index); }}
              >
                <label>
                  <span>t</span>
                  <input
                    type="number"
                    min={0}
                    max={1}
                    step={0.01}
                    value={point.time}
                    readOnly={readOnly}
                    onFocus={() => { setSelectedIndex(index); }}
                    onChange={(event) => {
                      if (readOnly) return;
                      setPoint(index, { ...point, time: Number(event.currentTarget.value) }, false);
                    }}
                    onBlur={() => { commitDraftPoint(index); }}
                    onKeyDown={(event) => {
                      if (event.key === "Enter") {
                        commitDraftPoint(index);
                        event.currentTarget.blur();
                      }
                    }}
                  />
                </label>
                <label>
                  <span>v</span>
                  <input
                    type="number"
                    step={0.05}
                    value={point.value}
                    readOnly={readOnly}
                    onFocus={() => { setSelectedIndex(index); }}
                    onChange={(event) => {
                      if (readOnly) return;
                      setPoint(index, { ...point, value: Number(event.currentTarget.value) }, false);
                    }}
                    onBlur={() => { commitDraftPoint(index); }}
                    onKeyDown={(event) => {
                      if (event.key === "Enter") {
                        commitDraftPoint(index);
                        event.currentTarget.blur();
                      }
                    }}
                  />
                </label>
                <button
                  type="button"
                  className="float-curve-point-delete"
                  title="Delete point"
                  disabled={readOnly || drafts.length <= 1}
                  onClick={() => { deletePoint(index); }}
                >
                  <Minus size={14} />
                </button>
              </div>
            ))}
          </div>
        )}
      </div>
      <button type="button" disabled={readOnly} onClick={() => {
        const nextPoint = { time: 1, value: drafts[drafts.length - 1]?.value ?? 0 };
        update([...drafts, nextPoint]);
        setSelectedIndex(drafts.length);
      }}>Add point</button>
    </div>
  );
}

function ColorCurveParam({
  name,
  points,
  commit,
  readOnly = false,
  requestInlineEdit
}: {
  name: string;
  points: EditedColorCurvePoint[];
  commit: (points: EditedColorCurvePoint[]) => Promise<void>;
  readOnly?: boolean;
  requestInlineEdit?: () => void;
}) {
  const [drafts, setDrafts] = useState(points);
  const draftsRef = useRef(points);
  const [selectedIndex, setSelectedIndex] = useState(0);
  const [pointsCollapsed, setPointsCollapsed] = useState(false);
  const gradientRef = useRef<HTMLDivElement | null>(null);
  const colorInputRefs = useRef<Array<HTMLInputElement | null>>([]);
  const draggingPoint = useRef<{ index: number; moved: boolean } | null>(null);
  const lastCommittedValues = useRef(points.map((point) => point.value.toLowerCase()));
  const pointsSignature = curvePointsSignature(points);
  const pendingSignature = useRef<string | null>(null);
  useEffect(() => {
    draftsRef.current = drafts;
  }, [drafts]);
  useEffect(() => {
    const draftSignature = curvePointsSignature(draftsRef.current);
    if (pointsSignature === draftSignature) {
      pendingSignature.current = null;
      return;
    }
    if (pendingSignature.current === draftSignature) return;
    setDrafts(points);
    draftsRef.current = points;
    lastCommittedValues.current = points.map((point) => point.value.toLowerCase());
    setSelectedIndex((index) => Math.min(index, points.length - 1));
  }, [points, pointsSignature]);
  const update = (next: EditedColorCurvePoint[]) => {
    if (readOnly) {
      requestInlineEdit?.();
      return;
    }
    if (next.length > 0 && next.every((point) => Number.isFinite(point.time) && isHexColor(point.value))) {
      const sorted = sortCurvePoints(next).map((point) => ({ ...point, value: point.value.toLowerCase() }));
      const signature = curvePointsSignature(sorted);
      setDrafts(sorted);
      draftsRef.current = sorted;
      lastCommittedValues.current = sorted.map((point) => point.value);
      pendingSignature.current = signature;
      void commit(sorted).catch(() => {
        pendingSignature.current = null;
      });
    }
  };
  const setPoint = (index: number, point: EditedColorCurvePoint, commitChange: boolean) => {
    const next = sortCurvePoints(replaceAt(draftsRef.current, index, point));
    const nextIndex = nearestColorPointIndex(next, point);
    setDrafts(next);
    draftsRef.current = next;
    setSelectedIndex(nextIndex);
    if (commitChange) {
      update(next);
    }
    return nextIndex;
  };
  const commitDraftValue = (index: number, candidate = drafts[index]?.value) => {
    const draft = candidate ?? points[index]?.value;
    if (draft === undefined || draft === "") return;
    if (!isHexColor(draft)) {
      const fallback = points[index];
      if (fallback !== undefined) {
        setDrafts((current) => replaceAt(current, index, fallback));
      }
      return;
    }
    const next = draft.toLowerCase();
    const currentPoint = drafts[index] ?? points[index];
    if (currentPoint === undefined) return;
    setDrafts((current) => replaceAt(current, index, { ...(current[index] ?? currentPoint), value: next }));
    if (next !== lastCommittedValues.current[index]) {
      lastCommittedValues.current = replaceAt(lastCommittedValues.current, index, next);
      update(replaceAt(drafts, index, { ...currentPoint, value: next }));
    }
  };
  const commitDraftPoint = (index: number) => {
    const point = draftsRef.current[index];
    if (!point) return;
    if (!isHexColor(point.value)) {
      commitDraftValue(index);
      return;
    }
    update(replaceAt(draftsRef.current, index, { time: clamp(point.time, 0, 1), value: point.value.toLowerCase() }));
  };
  const deletePoint = (index: number) => {
    if (draftsRef.current.length <= 1) return;
    const next = draftsRef.current.filter((_, pointIndex) => pointIndex !== index);
    update(next);
    setSelectedIndex(Math.min(index, next.length - 1));
  };
  const pointFromPointer = (event: PointerEvent<HTMLElement>, color: string): EditedColorCurvePoint => {
    const rect = gradientRef.current?.getBoundingClientRect();
    if (rect === undefined) return { time: 0, value: color };
    return {
      time: roundCurveValue(clamp((event.clientX - rect.left) / Math.max(1, rect.width), 0, 1)),
      value: color
    };
  };
  const gradient = colorCurveGradient(drafts);
  return (
    <div className="effect-param-group color-curve-editor">
      <div className="effect-param-name">{name}</div>
      <div
        ref={gradientRef}
        className="color-curve-gradient"
        style={{ background: gradient }}
        onPointerDown={(event) => {
          if (readOnly) {
            requestInlineEdit?.();
            return;
          }
          if (event.target !== event.currentTarget) return;
          const previous = draftsRef.current[draftsRef.current.length - 1]?.value ?? CURVE_EDITOR.defaultColor;
          const point = pointFromPointer(event, previous);
          update([...draftsRef.current, point]);
          setSelectedIndex(nearestColorPointIndex(draftsRef.current, point));
        }}
        onPointerMove={(event) => {
          const drag = draggingPoint.current;
          if (drag === null) return;
          const point = draftsRef.current[drag.index];
          if (point === undefined) return;
          const nextIndex = setPoint(drag.index, pointFromPointer(event, point.value), false);
          draggingPoint.current = { index: nextIndex, moved: true };
        }}
        onPointerUp={(event) => {
          if (draggingPoint.current === null) return;
          event.currentTarget.releasePointerCapture(event.pointerId);
          const moved = draggingPoint.current.moved;
          draggingPoint.current = null;
          if (moved) {
            update(draftsRef.current);
          }
        }}
        onPointerCancel={(event) => {
          if (draggingPoint.current === null) return;
          event.currentTarget.releasePointerCapture(event.pointerId);
          const moved = draggingPoint.current.moved;
          draggingPoint.current = null;
          if (moved) {
            update(draftsRef.current);
          }
        }}
      >
        {drafts.map((point, index) => {
          const displayedColor = isHexColor(point.value) ? point.value : (points[index]?.value ?? CURVE_EDITOR.defaultColor);
          return (
            <span
              key={index}
              className={`color-curve-stop ${index === selectedIndex ? "selected" : ""}`}
              style={{ left: `${point.time * 100}%` }}
              onPointerDown={(event) => {
                event.stopPropagation();
                if (readOnly) {
                  requestInlineEdit?.();
                  return;
                }
                event.currentTarget.parentElement?.setPointerCapture(event.pointerId);
                draggingPoint.current = { index, moved: false };
                setSelectedIndex(index);
              }}
              onDoubleClick={(event) => {
                event.preventDefault();
                event.stopPropagation();
                if (readOnly) {
                  requestInlineEdit?.();
                  return;
                }
                openColorPicker(colorInputRefs.current[index]);
              }}
              onContextMenu={(event) => {
                event.preventDefault();
                event.stopPropagation();
                if (readOnly) {
                  requestInlineEdit?.();
                  return;
                }
                deletePoint(index);
              }}
              onFocus={() => { setSelectedIndex(index); }}
            >
              <span className="color-curve-stop-line" />
              <label className="color-curve-stop-picker" title={`Gradient stop ${index + 1}`}>
                <input
                  ref={(input) => {
                    colorInputRefs.current[index] = input;
                  }}
                  type="color"
                  value={displayedColor}
                  disabled={readOnly}
                  onChange={(event) => {
                    if (readOnly) return;
                    setPoint(index, { ...point, value: event.currentTarget.value }, false);
                  }}
                  onBlur={() => { commitDraftValue(index); }}
                />
              </label>
            </span>
          );
        })}
      </div>
      <div className="float-curve-points-panel">
        <button
          type="button"
          className="float-curve-points-toggle"
          onClick={() => { setPointsCollapsed((collapsed) => !collapsed); }}
        >
          {pointsCollapsed ? <ChevronRight size={13} /> : <ChevronRight className="expanded" size={13} />}
          <span>Stops</span>
          <strong>{drafts.length}</strong>
        </button>
        {!pointsCollapsed && (
          <div className="float-curve-point-list">
            {drafts.map((point, index) => {
              const displayedColor = isHexColor(point.value) ? point.value : (points[index]?.value ?? CURVE_EDITOR.defaultColor);
              return (
                <div
                  key={index}
                  className={`color-curve-point-row-compact ${index === selectedIndex ? "selected" : ""}`}
                  onPointerDown={() => { setSelectedIndex(index); }}
                >
                  <label>
                    <span>t</span>
                    <input
                      type="number"
                      min={0}
                      max={1}
                      step={0.01}
                      value={point.time}
                      readOnly={readOnly}
                      onFocus={() => { setSelectedIndex(index); }}
                      onChange={(event) => {
                        setPoint(index, { ...point, time: Number(event.currentTarget.value) }, false);
                      }}
                      onBlur={() => { commitDraftPoint(index); }}
                      onKeyDown={(event) => {
                        if (event.key === "Enter") {
                          commitDraftPoint(index);
                          event.currentTarget.blur();
                        }
                      }}
                    />
                  </label>
                  <label className="color-swatch-picker">
                    <span className="color-swatch" style={{ background: displayedColor }} />
                    <input
                      type="color"
                      value={displayedColor}
                      disabled={readOnly}
                      onFocus={() => { setSelectedIndex(index); }}
                      onChange={(event) => {
                        if (readOnly) return;
                        setPoint(index, { ...point, value: event.currentTarget.value }, false);
                      }}
                      onBlur={() => { commitDraftValue(index); }}
                    />
                  </label>
                  <input
                    value={point.value}
                    readOnly={readOnly}
                    onFocus={() => { setSelectedIndex(index); }}
                    onChange={(event) => {
                      if (readOnly) return;
                      setPoint(index, { ...point, value: event.currentTarget.value }, false);
                    }}
                    onBlur={() => { commitDraftValue(index); }}
                    onKeyDown={(event) => {
                      if (event.key === "Enter") {
                        commitDraftValue(index);
                        event.currentTarget.blur();
                      }
                    }}
                  />
                  <button type="button" className="float-curve-point-delete" disabled={readOnly || drafts.length <= 1} onClick={() => { deletePoint(index); }}>
                    <Minus size={14} />
                  </button>
                </div>
              );
            })}
          </div>
        )}
      </div>
      <button type="button" disabled={readOnly} onClick={() => {
        const nextPoint = { time: 1, value: drafts[drafts.length - 1]?.value ?? CURVE_EDITOR.defaultColor };
        update([...drafts, nextPoint]);
        setSelectedIndex(drafts.length);
      }}>Add stop</button>
    </div>
  );
}

function replaceAt<T>(items: T[], index: number, value: T) {
  return items.map((item, itemIndex) => (itemIndex === index ? value : item));
}

function sortCurvePoints<T extends { time: number }>(points: T[]) {
  return [...points].sort((left, right) => left.time - right.time);
}

function floatCurveValueRange(points: EditedFloatCurvePoint[]) {
  const values = points.map((point) => point.value).filter(Number.isFinite);
  const min = Math.min(0, ...values);
  const max = Math.max(1, ...values);
  if (Math.abs(max - min) < CURVE_EDITOR.flatRangeEpsilon) return { min: min - 0.5, max: max + 0.5 };
  return { min, max };
}

function floatCurveSvgPath(points: EditedFloatCurvePoint[], range: { min: number; max: number }) {
  const sorted = sortCurvePoints(points);
  if (sorted.length === 0) return "";
  return sorted
    .map((point, index) => {
      const x = point.time * CURVE_EDITOR.width;
      const y = CURVE_EDITOR.height - ((point.value - range.min) / (range.max - range.min)) * CURVE_EDITOR.height;
      return `${index === 0 ? "M" : "L"}${x.toFixed(2)} ${y.toFixed(2)}`;
    })
    .join(" ");
}

function nearestFloatPointIndex(points: EditedFloatCurvePoint[], point: EditedFloatCurvePoint) {
  let bestIndex = 0;
  let bestDistance = Infinity;
  points.forEach((candidate, index) => {
    const distance = Math.abs(candidate.time - point.time) + Math.abs(candidate.value - point.value);
    if (distance < bestDistance) {
      bestDistance = distance;
      bestIndex = index;
    }
  });
  return bestIndex;
}

function nearestColorPointIndex(points: EditedColorCurvePoint[], point: EditedColorCurvePoint) {
  let bestIndex = 0;
  let bestDistance = Infinity;
  points.forEach((candidate, index) => {
    const distance = Math.abs(candidate.time - point.time) + (candidate.value === point.value ? 0 : CURVE_EDITOR.colorMismatchDistance);
    if (distance < bestDistance) {
      bestDistance = distance;
      bestIndex = index;
    }
  });
  return bestIndex;
}

function colorCurveGradient(points: EditedColorCurvePoint[]) {
  const stops = sortCurvePoints(points)
    .filter((point) => isHexColor(point.value))
    .map((point) => `${point.value} ${clamp(point.time, 0, 1) * 100}%`);
  if (stops.length === 0) return CURVE_EDITOR.emptyGradient;
  if (stops.length === 1) return stops[0]?.split(" ")[0] ?? CURVE_EDITOR.emptyGradient;
  return `linear-gradient(90deg, ${stops.join(", ")})`;
}

function roundCurveValue(value: number) {
  return Math.round(value * CURVE_EDITOR.roundScale) / CURVE_EDITOR.roundScale;
}

function curvePointsSignature(points: Array<{ time: number; value: number | string }>) {
  return JSON.stringify(points);
}

function normalizeFloatCurvePoints(points: FloatCurvePointDto[]): EditedFloatCurvePoint[] {
  const normalized = points
    .filter((point) => Number.isFinite(point.time) && Number.isFinite(point.value))
    .map((point) => ({ time: clamp(point.time, 0, 1), value: point.value }));
  return normalized.length > 0 ? normalized : [{ time: 0, value: 0 }];
}

function normalizeColorCurvePoints(points: ColorCurvePointDto[]): EditedColorCurvePoint[] {
  const normalized = points
    .filter((point) => isHexColor(point.value))
    .filter((point) => Number.isFinite(point.time))
    .map((point) => ({ time: clamp(point.time, 0, 1), value: point.value.toLowerCase() }));
  return normalized.length > 0 ? normalized : [{ time: 0, value: CURVE_EDITOR.defaultColor }];
}

function isHexColor(value: string): boolean {
  return /^#[0-9a-fA-F]{6}$/.test(value);
}
