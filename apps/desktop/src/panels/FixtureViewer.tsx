import { useEffect, useMemo, useRef, useState } from "react";
import { Copy, LocateFixed, Plus, Trash2, ZoomIn, ZoomOut } from "lucide-react";
import type { ColorModel, FixtureDefinitionDocument, FixtureDocument, Geometry, Point3 } from "../generated/bindings";
import {
  fitViewBox,
  renderGeometryPlan,
  renderPointsAsGeometry,
  ScaleBar,
  svgPixelHeight,
  svgPixelWidth,
  type ViewBox,
  zoomViewBox
} from "./geometryRender";

type FixtureViewerProps = {
  document: FixtureDocument;
  selectedObjectKey: string | null;
  onSelectObject: (objectKey: string | null) => void;
  onDocumentChange: (document: FixtureDocument) => Promise<void>;
};

const colorModels: ColorModel[] = ["rgb", "rgba", "rgbw", "rgbaw", "white"];
const defaultBulbSize = 1;
const viewBoxOptions = { minSize: 0.25, paddingScale: 0.35, paddingBase: 0.25 };

export function FixtureViewer({ document, selectedObjectKey, onSelectObject, onDocumentChange }: FixtureViewerProps) {
  const svgRef = useRef<SVGSVGElement | null>(null);
  const panRef = useRef<{ x: number; y: number; viewBox: ViewBox } | null>(null);
  const selected = document.fixtures.find((fixture) => fixture.objectKey === selectedObjectKey) ?? document.fixtures[0] ?? null;
  const bounds = useMemo(() => selected?.renderPlan.bounds ?? defaultPreviewBounds(), [selected]);
  const [viewportAspect, setViewportAspect] = useState(16 / 9);
  const [viewBox, setViewBox] = useState<ViewBox>(() => fitViewBox(bounds, 16 / 9, viewBoxOptions));

  const commitFixture = (objectKey: string, update: (fixture: FixtureDefinitionDocument) => FixtureDefinitionDocument) =>
    onDocumentChange({
      ...document,
      selectedObjectKey: objectKey,
      fixtures: document.fixtures.map((fixture) => fixture.objectKey === objectKey ? update(fixture) : fixture)
    });

  useEffect(() => {
    const svg = svgRef.current;
    if (!svg) return;

    const updateAspect = () => {
      const width = Math.max(svg.clientWidth, 1);
      const height = Math.max(svg.clientHeight, 1);
      setViewportAspect(width / height);
    };
    updateAspect();

    const observer = new ResizeObserver(updateAspect);
    observer.observe(svg);
    return () => observer.disconnect();
  }, []);

  useEffect(() => {
    setViewBox(fitViewBox(bounds, viewportAspect, viewBoxOptions));
  }, [bounds, selected?.objectKey, viewportAspect]);

  return (
    <div className="fixture-viewer">
      <aside className="fixture-sidebar">
        <div className="layout-details-heading">
          <h3>Fixtures</h3>
          <button title="Create fixture" onClick={() => {
            const objectKey = uniqueId("fixture", document.fixtures.map((fixture) => fixture.objectKey));
            onSelectObject(objectKey);
            void onDocumentChange({
              ...document,
              selectedObjectKey: objectKey,
              fixtures: [...document.fixtures, {
                objectKey,
                name: "Fixture",
                colorModel: "rgb",
                bulbSize: defaultBulbSize,
                geometry: { type: "points", points: [{ x: 0, y: 0, z: 0 }] },
                geometrySummary: "",
                renderPlan: {
                  emitters: [{ x: 0, y: 0, z: 0 }],
                  guides: [],
                  bounds: defaultPreviewBounds(),
                  bulbRadius: 0.035
                }
              }]
            });
          }}><Plus size={14} /></button>
        </div>
        <div className="layout-fixture-list">
          {document.fixtures.map((fixture) => (
            <button
              key={fixture.objectKey}
              className={fixture.objectKey === selected?.objectKey ? "active" : ""}
              onClick={() => onSelectObject(fixture.objectKey)}
            >
              <span>{fixture.objectKey}</span>
              <small>{fixture.name}</small>
            </button>
          ))}
        </div>
      </aside>
      <div className="fixture-preview-pane">
        <div className="layout-canvas-toolbar">
          <button title="Fit to view" onClick={() => setViewBox(fitViewBox(bounds, viewportAspect, viewBoxOptions))}><LocateFixed size={15} /></button>
          <button title="Zoom in" onClick={() => setViewBox((box) => zoomViewBox(box, 0.82, 0.02))}><ZoomIn size={15} /></button>
          <button title="Zoom out" onClick={() => setViewBox((box) => zoomViewBox(box, 1.18, 0.02))}><ZoomOut size={15} /></button>
          <span>fixture units</span>
        </div>
        <svg
          ref={svgRef}
          className="fixture-preview"
          viewBox={`${viewBox.x} ${viewBox.y} ${viewBox.width} ${viewBox.height}`}
          onMouseDown={(event) => {
            if (event.button !== 0) return;
            panRef.current = { x: event.clientX, y: event.clientY, viewBox };
          }}
          onMouseMove={(event) => {
            const pan = panRef.current;
            if (!pan) return;
            const dx = ((event.clientX - pan.x) / svgPixelWidth(svgRef.current)) * pan.viewBox.width;
            const dy = ((event.clientY - pan.y) / svgPixelHeight(svgRef.current)) * pan.viewBox.height;
            setViewBox({ ...pan.viewBox, x: pan.viewBox.x - dx, y: pan.viewBox.y - dy });
          }}
          onMouseUp={() => {
            panRef.current = null;
          }}
          onMouseLeave={() => {
            panRef.current = null;
          }}
          onWheel={(event) => {
            event.preventDefault();
            setViewBox((box) => zoomViewBox(box, event.deltaY < 0 ? 0.9 : 1.1, 0.02));
          }}
        >
          <defs>
            <pattern id="fixture-grid" width="1" height="1" patternUnits="userSpaceOnUse">
              <path d="M 1 0 L 0 0 0 1" fill="none" stroke="rgba(255,255,255,0.07)" strokeWidth="0.015" />
            </pattern>
          </defs>
          <rect x={viewBox.x} y={viewBox.y} width={viewBox.width} height={viewBox.height} fill="url(#fixture-grid)" />
          <line x1={viewBox.x} y1="0" x2={viewBox.x + viewBox.width} y2="0" className="layout-axis" />
          <line x1="0" y1={viewBox.y} x2="0" y2={viewBox.y + viewBox.height} className="layout-axis" />
          <g transform="scale(1 -1)" className="layout-fixture-shape selected">
            {selected ? renderGeometryPlan(selected.renderPlan) : null}
          </g>
          <ScaleBar viewBox={viewBox} units={viewBox.width === 1 ? "unit" : "units"} svg={svgRef.current} />
        </svg>
      </div>
      <aside className="layout-inspector">
        {selected ? (
          <section className="layout-details">
            <div className="layout-details-heading">
              <input
                aria-label="Fixture object key"
                value={selected.objectKey}
                onChange={(event) => {
                  const objectKey = event.target.value;
                  onSelectObject(objectKey);
                  void onDocumentChange({
                    ...document,
                    selectedObjectKey: objectKey,
                    fixtures: document.fixtures.map((fixture) => fixture.objectKey === selected.objectKey ? { ...fixture, objectKey } : fixture)
                  });
                }}
              />
              <button title="Duplicate" onClick={() => {
                const objectKey = uniqueId(`${selected.objectKey}_copy`, document.fixtures.map((fixture) => fixture.objectKey));
                onSelectObject(objectKey);
                void onDocumentChange({
                  ...document,
                  selectedObjectKey: objectKey,
                  fixtures: [...document.fixtures, { ...selected, objectKey, name: `${selected.name} Copy` }]
                });
              }}><Copy size={14} /></button>
              <button title="Delete" onClick={() => {
                const fixtures = document.fixtures.filter((fixture) => fixture.objectKey !== selected.objectKey);
                onSelectObject(fixtures[0]?.objectKey ?? null);
                void onDocumentChange({ ...document, selectedObjectKey: fixtures[0]?.objectKey ?? null, fixtures });
              }}><Trash2 size={14} /></button>
            </div>
            <label>
              Name
              <input value={selected.name} onChange={(event) => void commitFixture(selected.objectKey, (fixture) => ({ ...fixture, name: event.target.value }))} />
            </label>
            <label>
              Color
              <select value={selected.colorModel} onChange={(event) => void commitFixture(selected.objectKey, (fixture) => ({ ...fixture, colorModel: event.target.value as ColorModel }))}>
                {colorModels.map((model) => <option key={model} value={model}>{model}</option>)}
              </select>
            </label>
            <NumberField label="Bulb size" value={normalizedBulbSize(selected.bulbSize)} step={0.1} min={0.05} onChange={(bulbSize) => void commitFixture(selected.objectKey, (fixture) => ({ ...fixture, bulbSize }))} />
            <label>
              Geometry
              <select value={selected.geometry.type} onChange={(event) => void commitFixture(selected.objectKey, (fixture) => ({
                ...fixture,
                geometry: geometryForType(event.target.value as Geometry["type"], fixture)
              }))}>
                <option value="points">points</option>
                <option value="lines">lines</option>
                <option value="arc">arc</option>
              </select>
            </label>
            <GeometryEditor geometry={selected.geometry} onChange={(geometry) => void commitFixture(selected.objectKey, (fixture) => ({ ...fixture, geometry }))} />
          </section>
        ) : (
          <section className="layout-details empty">No fixture selected</section>
        )}
      </aside>
    </div>
  );
}

function GeometryEditor({ geometry, onChange }: { geometry: Geometry; onChange: (geometry: Geometry) => void }) {
  if (geometry.type === "points") {
    return <PointList points={geometry.points} onChange={(points) => onChange({ ...geometry, points })} />;
  }
  if (geometry.type === "lines") {
    return (
      <>
        <PointList points={geometry.points} onChange={(points) => onChange({ ...geometry, points })} />
        <NumberField label="Pixels" value={geometry.pixels} min={1} onChange={(pixels) => onChange({ ...geometry, pixels })} />
      </>
    );
  }
  return (
    <>
      <PointEditor label="Center" point={geometry.center} onChange={(center) => onChange({ ...geometry, center })} />
      <NumberField label="Radius" value={geometry.radius ?? 0} onChange={(radius) => onChange({ ...geometry, radius })} />
      <NumberField label="Start" value={geometry.startDegrees ?? 0} onChange={(startDegrees) => onChange({ ...geometry, startDegrees })} />
      <NumberField label="End" value={geometry.endDegrees ?? 0} onChange={(endDegrees) => onChange({ ...geometry, endDegrees })} />
      <NumberField label="Pixels" value={geometry.pixels} onChange={(pixels) => onChange({ ...geometry, pixels })} />
    </>
  );
}

function PointList({ points, onChange }: { points: Point3[]; onChange: (points: Point3[]) => void }) {
  return (
    <div className="geometry-list">
      {points.map((point, index) => (
        <div key={index} className="geometry-row">
          <PointEditor label={`Point ${index + 1}`} point={point} onChange={(next) => onChange(points.map((item, itemIndex) => itemIndex === index ? next : item))} />
          <button title="Remove point" onClick={() => onChange(points.filter((_, itemIndex) => itemIndex !== index))}><Trash2 size={13} /></button>
        </div>
      ))}
      <button onClick={() => onChange([...points, { x: 0, y: 0, z: 0 }])}>Add point</button>
    </div>
  );
}

function PointEditor({ label, point, onChange }: { label: string; point: Point3; onChange: (point: Point3) => void }) {
  return (
    <fieldset className="point-editor">
      <legend>{label}</legend>
      {(["x", "y", "z"] as const).map((axis) => (
        <NumericInput key={axis} step={0.1} value={point[axis] ?? 0} onChange={(value) => onChange({ ...point, [axis]: value })} />
      ))}
    </fieldset>
  );
}

function NumberField({ label, value, step, min, onChange }: { label: string; value: number; step?: number; min?: number; onChange: (value: number) => void }) {
  return (
    <label>
      {label}
      <NumericInput step={step} min={min} value={value} onChange={onChange} />
    </label>
  );
}

function NumericInput({
  value,
  step,
  min,
  onChange
}: {
  value: number;
  step?: number;
  min?: number;
  onChange: (value: number) => void;
}) {
  const [draft, setDraft] = useState(() => formatNumberInput(value));
  const [focused, setFocused] = useState(false);

  useEffect(() => {
    if (!focused) setDraft(formatNumberInput(value));
  }, [focused, value]);

  return (
    <input
      type="number"
      step={step}
      min={min}
      value={draft}
      onFocus={() => setFocused(true)}
      onChange={(event) => {
        const next = event.target.value;
        setDraft(next);
        if (next === "") return;
        const parsed = Number(next);
        if (Number.isFinite(parsed)) onChange(parsed);
      }}
      onBlur={() => {
        setFocused(false);
        const parsed = Number(draft);
        setDraft(draft !== "" && Number.isFinite(parsed) ? formatNumberInput(parsed) : formatNumberInput(value));
      }}
    />
  );
}

function formatNumberInput(value: number) {
  return Number.isFinite(value) ? String(value) : "0";
}

function normalizedBulbSize(value: number | null | undefined) {
  return Math.max(0.05, value ?? defaultBulbSize);
}

function geometryForType(type: Geometry["type"], current: FixtureDefinitionDocument): Geometry {
  if (type === "points" && current.geometry.type === "lines") return { type, points: renderPointsAsGeometry(current.renderPlan.emitters) };
  if (type === "lines" && current.geometry.type === "points") return { type, points: current.geometry.points, pixels: Math.max(1, current.geometry.points.length) };
  return defaultGeometry(type);
}

function defaultGeometry(type: Geometry["type"]): Geometry {
  if (type === "lines") return { type, points: [{ x: -0.5, y: 0, z: 0 }, { x: 0.5, y: 0, z: 0 }], pixels: 2 };
  if (type === "arc") return { type, center: { x: 0, y: 0, z: 0 }, radius: 1, startDegrees: 0, endDegrees: 180, pixels: 8 };
  return { type, points: [{ x: 0, y: 0, z: 0 }] };
}

function defaultPreviewBounds() {
  return { minX: -1, minY: -1, maxX: 1, maxY: 1 };
}

function uniqueId(base: string, used: string[]) {
  const taken = new Set(used);
  if (!taken.has(base)) return base;
  let index = 2;
  while (taken.has(`${base}_${index}`)) index += 1;
  return `${base}_${index}`;
}
