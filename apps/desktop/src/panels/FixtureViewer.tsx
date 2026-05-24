import { useEffect, useMemo, useRef, useState } from "react";
import { Copy, LocateFixed, Plus, Trash2, ZoomIn, ZoomOut } from "lucide-react";
import type { ColorModel, FixtureDefinitionDocument, FixtureDocument, Geometry, LineSegment, Point3 } from "../generated/bindings";

type FixtureViewerProps = {
  document: FixtureDocument;
  selectedObjectKey: string | null;
  onSelectObject: (objectKey: string | null) => void;
  onDocumentChange: (document: FixtureDocument) => Promise<void>;
};

const colorModels: ColorModel[] = ["rgb", "rgba", "rgbw", "rgbaw", "white"];
const defaultBulbSize = 1;
const bulbSizeUnitRadius = 0.035;

type ViewBox = { x: number; y: number; width: number; height: number };
type Bounds = { minX: number; minY: number; maxX: number; maxY: number };

export function FixtureViewer({ document, selectedObjectKey, onSelectObject, onDocumentChange }: FixtureViewerProps) {
  const svgRef = useRef<SVGSVGElement | null>(null);
  const panRef = useRef<{ x: number; y: number; viewBox: ViewBox } | null>(null);
  const selected = document.fixtures.find((fixture) => fixture.objectKey === selectedObjectKey) ?? document.fixtures[0] ?? null;
  const bounds = useMemo(() => selected ? geometryBounds(selected.geometry) : defaultBounds(), [selected]);
  const [viewportAspect, setViewportAspect] = useState(16 / 9);
  const [viewBox, setViewBox] = useState<ViewBox>(() => fitViewBox(bounds, 16 / 9));

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
    setViewBox(fitViewBox(bounds, viewportAspect));
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
                geometrySummary: ""
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
          <button title="Fit to view" onClick={() => setViewBox(fitViewBox(bounds, viewportAspect))}><LocateFixed size={15} /></button>
          <button title="Zoom in" onClick={() => setViewBox((box) => zoomViewBox(box, 0.82))}><ZoomIn size={15} /></button>
          <button title="Zoom out" onClick={() => setViewBox((box) => zoomViewBox(box, 1.18))}><ZoomOut size={15} /></button>
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
            setViewBox((box) => zoomViewBox(box, event.deltaY < 0 ? 0.9 : 1.1));
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
            {selected ? renderGeometry(selected.geometry, selected.bulbSize) : null}
          </g>
          <ScaleBar viewBox={viewBox} svg={svgRef.current} />
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
                geometry: defaultGeometry(event.target.value as Geometry["type"])
              }))}>
                <option value="points">points</option>
                <option value="line">line</option>
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

function ScaleBar({ viewBox, svg }: { viewBox: ViewBox; svg: SVGSVGElement | null }) {
  const length = niceScaleLength(viewBox.width * 0.18);
  const x = viewBox.x + viewBox.width * 0.055;
  const y = viewBox.y + viewBox.height * 0.9;
  const tick = viewBox.height * 0.018;
  const fontSize = screenPixelsToUserY(viewBox, svg, 12);
  const labelStrokeWidth = screenPixelsToUserY(viewBox, svg, 3);
  return (
    <g className="canvas-scale-bar">
      <line x1={x} y1={y} x2={x + length} y2={y} />
      <line x1={x} y1={y - tick} x2={x} y2={y + tick} />
      <line x1={x + length} y1={y - tick} x2={x + length} y2={y + tick} />
      <text x={x} y={y - tick * 1.8} fontSize={fontSize} strokeWidth={labelStrokeWidth}>{formatScaleLength(length)} {length === 1 ? "unit" : "units"}</text>
    </g>
  );
}

function GeometryEditor({ geometry, onChange }: { geometry: Geometry; onChange: (geometry: Geometry) => void }) {
  if (geometry.type === "points") {
    return <PointList points={geometry.points} onChange={(points) => onChange({ ...geometry, points })} />;
  }
  if (geometry.type === "line") {
    return (
      <>
        <PointEditor label="From" point={geometry.from} onChange={(from) => onChange({ ...geometry, from })} />
        <PointEditor label="To" point={geometry.to} onChange={(to) => onChange({ ...geometry, to })} />
        <NumberField label="Pixels" value={geometry.pixels} onChange={(pixels) => onChange({ ...geometry, pixels })} />
      </>
    );
  }
  if (geometry.type === "lines") {
    return (
      <>
        <PointList points={geometry.points} onChange={(points) => onChange({ ...geometry, points })} />
        <SegmentList segments={geometry.lines} onChange={(lines) => onChange({ ...geometry, lines })} />
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

function SegmentList({ segments, onChange }: { segments: LineSegment[]; onChange: (segments: LineSegment[]) => void }) {
  return (
    <div className="geometry-list">
      {segments.map((segment, index) => (
        <div key={index} className="geometry-row">
          <NumberField label="From" value={segment.from} onChange={(from) => onChange(segments.map((item, itemIndex) => itemIndex === index ? { ...item, from } : item))} />
          <NumberField label="To" value={segment.to} onChange={(to) => onChange(segments.map((item, itemIndex) => itemIndex === index ? { ...item, to } : item))} />
          <button title="Remove segment" onClick={() => onChange(segments.filter((_, itemIndex) => itemIndex !== index))}><Trash2 size={13} /></button>
        </div>
      ))}
      <button onClick={() => onChange([...segments, { from: 0, to: 0 }])}>Add segment</button>
    </div>
  );
}

function PointEditor({ label, point, onChange }: { label: string; point: Point3; onChange: (point: Point3) => void }) {
  return (
    <fieldset className="point-editor">
      <legend>{label}</legend>
      {(["x", "y", "z"] as const).map((axis) => (
        <input key={axis} type="number" step="0.1" value={point[axis] ?? 0} onChange={(event) => onChange({ ...point, [axis]: Number(event.target.value) })} />
      ))}
    </fieldset>
  );
}

function NumberField({ label, value, step, min, onChange }: { label: string; value: number; step?: number; min?: number; onChange: (value: number) => void }) {
  return (
    <label>
      {label}
      <input type="number" step={step} min={min} value={value} onChange={(event) => onChange(Number(event.target.value))} />
    </label>
  );
}

function renderGeometry(geometry: Geometry, bulbSize: number | null | undefined) {
  const radius = bulbRadius(bulbSize);
  if (geometry.type === "points") return renderEmitterPoints(geometry.points, radius);
  if (geometry.type === "line") {
    const points = sampleLinePoints(geometry.from, geometry.to, geometry.pixels);
    return (
      <>
        <line className="layout-fixture-guide" x1={geometry.from.x ?? 0} y1={geometry.from.y ?? 0} x2={geometry.to.x ?? 0} y2={geometry.to.y ?? 0} />
        {renderEmitterPoints(points, radius)}
      </>
    );
  }
  if (geometry.type === "lines") {
    return (
      <>
        {geometry.lines.map((line, index) => {
          const from = geometry.points[line.from] ?? { x: 0, y: 0, z: 0 };
          const to = geometry.points[line.to] ?? { x: 0, y: 0, z: 0 };
          return <line key={index} className="layout-fixture-guide" x1={from.x ?? 0} y1={from.y ?? 0} x2={to.x ?? 0} y2={to.y ?? 0} />;
        })}
        {renderEmitterPoints(geometry.points, radius)}
      </>
    );
  }
  const center = geometry.center;
  const arcRadius = geometry.radius ?? 1;
  const startDegrees = geometry.startDegrees ?? 0;
  const endDegrees = geometry.endDegrees ?? 180;
  const start = polar(center, arcRadius, startDegrees);
  const end = polar(center, arcRadius, endDegrees);
  const largeArc = Math.abs(endDegrees - startDegrees) > 180 ? 1 : 0;
  return (
    <>
      <path className="layout-fixture-guide" d={`M ${start.x} ${start.y} A ${arcRadius} ${arcRadius} 0 ${largeArc} 1 ${end.x} ${end.y}`} />
      {renderEmitterPoints(sampleArcPoints(center, arcRadius, startDegrees, endDegrees, geometry.pixels), radius)}
    </>
  );
}

function geometryBounds(geometry: Geometry): Bounds {
  const points = geometryPoints(geometry);
  if (!points.length) return defaultBounds();
  return points.reduce((bounds, point) => ({
    minX: Math.min(bounds.minX, point.x ?? 0),
    minY: Math.min(bounds.minY, point.y ?? 0),
    maxX: Math.max(bounds.maxX, point.x ?? 0),
    maxY: Math.max(bounds.maxY, point.y ?? 0)
  }), {
    minX: points[0].x ?? 0,
    minY: points[0].y ?? 0,
    maxX: points[0].x ?? 0,
    maxY: points[0].y ?? 0
  });
}

function geometryPoints(geometry: Geometry): Point3[] {
  if (geometry.type === "points") return geometry.points;
  if (geometry.type === "line") return [geometry.from, geometry.to];
  if (geometry.type === "lines") return geometry.points;
  const radius = geometry.radius ?? 1;
  const startDegrees = geometry.startDegrees ?? 0;
  const endDegrees = geometry.endDegrees ?? 180;
  return [
    { x: (geometry.center.x ?? 0) - radius, y: (geometry.center.y ?? 0) - radius, z: geometry.center.z ?? 0 },
    { x: (geometry.center.x ?? 0) + radius, y: (geometry.center.y ?? 0) + radius, z: geometry.center.z ?? 0 },
    ...sampleArcPoints(geometry.center, radius, startDegrees, endDegrees, geometry.pixels)
  ];
}

function defaultBounds(): Bounds {
  return { minX: -1, minY: -1, maxX: 1, maxY: 1 };
}

function fitViewBox(bounds: Bounds, viewportAspect: number): ViewBox {
  const width = Math.max(bounds.maxX - bounds.minX, 0.25);
  const height = Math.max(bounds.maxY - bounds.minY, 0.25);
  const padding = Math.max(width, height) * 0.35 + 0.25;
  let fittedWidth = width + padding * 2;
  let fittedHeight = height + padding * 2;
  const aspect = Math.max(viewportAspect, 0.1);
  if (fittedWidth / fittedHeight > aspect) {
    fittedHeight = fittedWidth / aspect;
  } else {
    fittedWidth = fittedHeight * aspect;
  }
  const centerX = (bounds.minX + bounds.maxX) / 2;
  const centerY = (bounds.minY + bounds.maxY) / 2;
  return {
    x: centerX - fittedWidth / 2,
    y: -(centerY + fittedHeight / 2),
    width: fittedWidth,
    height: fittedHeight
  };
}

function zoomViewBox(viewBox: ViewBox, factor: number): ViewBox {
  const width = Math.max(viewBox.width * factor, 0.02);
  const height = Math.max(viewBox.height * factor, 0.02);
  return { x: viewBox.x + (viewBox.width - width) / 2, y: viewBox.y + (viewBox.height - height) / 2, width, height };
}

function renderEmitterPoints(points: Point3[], radius: number) {
  return points.map((point, index) => (
    <circle key={index} className="layout-fixture-emitter" cx={point.x ?? 0} cy={point.y ?? 0} r={radius} />
  ));
}

function normalizedBulbSize(value: number | null | undefined) {
  return Math.max(0.05, value ?? defaultBulbSize);
}

function bulbRadius(value: number | null | undefined) {
  return normalizedBulbSize(value) * bulbSizeUnitRadius;
}

function sampleLinePoints(from: Point3, to: Point3, pixels: number): Point3[] {
  const count = Math.max(1, Math.floor(pixels));
  if (count === 1) {
    return [{
      x: ((from.x ?? 0) + (to.x ?? 0)) / 2,
      y: ((from.y ?? 0) + (to.y ?? 0)) / 2,
      z: ((from.z ?? 0) + (to.z ?? 0)) / 2
    }];
  }

  return Array.from({ length: count }, (_, index) => {
    const t = index / (count - 1);
    return {
      x: lerp(from.x ?? 0, to.x ?? 0, t),
      y: lerp(from.y ?? 0, to.y ?? 0, t),
      z: lerp(from.z ?? 0, to.z ?? 0, t)
    };
  });
}

function sampleArcPoints(center: Point3, radius: number, startDegrees: number, endDegrees: number, pixels: number): Point3[] {
  const count = Math.max(1, Math.floor(pixels));
  if (count === 1) {
    return [polar(center, radius, (startDegrees + endDegrees) / 2)];
  }

  return Array.from({ length: count }, (_, index) =>
    polar(center, radius, lerp(startDegrees, endDegrees, index / (count - 1)))
  );
}

function lerp(from: number, to: number, t: number) {
  return from + (to - from) * t;
}

function defaultGeometry(type: Geometry["type"]): Geometry {
  if (type === "line") return { type, from: { x: -0.5, y: 0, z: 0 }, to: { x: 0.5, y: 0, z: 0 }, pixels: 2 };
  if (type === "lines") return { type, points: [{ x: -0.5, y: 0, z: 0 }, { x: 0.5, y: 0, z: 0 }], lines: [{ from: 0, to: 1 }] };
  if (type === "arc") return { type, center: { x: 0, y: 0, z: 0 }, radius: 1, startDegrees: 0, endDegrees: 180, pixels: 8 };
  return { type, points: [{ x: 0, y: 0, z: 0 }] };
}

function polar(center: Point3, radius: number, degrees: number) {
  const radians = (degrees * Math.PI) / 180;
  return {
    x: (center.x ?? 0) + radius * Math.cos(radians),
    y: (center.y ?? 0) + radius * Math.sin(radians),
    z: center.z ?? 0
  };
}

function niceScaleLength(target: number) {
  if (!Number.isFinite(target) || target <= 0) return 1;
  const exponent = Math.floor(Math.log10(target));
  const magnitude = 10 ** exponent;
  const normalized = target / magnitude;
  const step = normalized >= 5 ? 5 : normalized >= 2 ? 2 : 1;
  return step * magnitude;
}

function formatScaleLength(length: number) {
  return Number.isInteger(length) ? `${length}` : length.toFixed(length < 0.1 ? 3 : length < 1 ? 2 : 1).replace(/0+$/, "").replace(/\.$/, "");
}

function svgPixelWidth(svg: SVGSVGElement | null) {
  return Math.max(svg?.clientWidth ?? 1, 1);
}

function svgPixelHeight(svg: SVGSVGElement | null) {
  return Math.max(svg?.clientHeight ?? 1, 1);
}

function screenPixelsToUserY(viewBox: ViewBox, svg: SVGSVGElement | null, pixels: number) {
  return (pixels / svgPixelHeight(svg)) * viewBox.height;
}

function uniqueId(base: string, used: string[]) {
  const taken = new Set(used);
  if (!taken.has(base)) return base;
  let index = 2;
  while (taken.has(`${base}_${index}`)) index += 1;
  return `${base}_${index}`;
}
