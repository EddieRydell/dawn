import { Copy, Plus, Trash2 } from "lucide-react";
import type { ColorModel, FixtureDefinitionDocument, FixtureDocument, Geometry, LineSegment, Point3 } from "../generated/bindings";

type FixtureViewerProps = {
  document: FixtureDocument;
  selectedObjectKey: string | null;
  onSelectObject: (objectKey: string | null) => void;
  onDocumentChange: (document: FixtureDocument) => Promise<void>;
};

const colorModels: ColorModel[] = ["rgb", "rgba", "rgbw", "rgbaw", "white"];

export function FixtureViewer({ document, selectedObjectKey, onSelectObject, onDocumentChange }: FixtureViewerProps) {
  const selected = document.fixtures.find((fixture) => fixture.objectKey === selectedObjectKey) ?? document.fixtures[0] ?? null;

  const commitFixture = (objectKey: string, update: (fixture: FixtureDefinitionDocument) => FixtureDefinitionDocument) =>
    onDocumentChange({
      ...document,
      selectedObjectKey: objectKey,
      fixtures: document.fixtures.map((fixture) => fixture.objectKey === objectKey ? update(fixture) : fixture)
    });

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
        <svg className="fixture-preview" viewBox="-4 -4 8 8">
          <defs>
            <pattern id="fixture-grid" width="1" height="1" patternUnits="userSpaceOnUse">
              <path d="M 1 0 L 0 0 0 1" fill="none" stroke="rgba(255,255,255,0.07)" strokeWidth="0.015" />
            </pattern>
          </defs>
          <rect x="-4" y="-4" width="8" height="8" fill="url(#fixture-grid)" />
          <g transform="scale(1 -1)" className="layout-fixture-shape selected">
            {selected ? renderGeometry(selected.geometry) : null}
          </g>
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

function NumberField({ label, value, onChange }: { label: string; value: number; onChange: (value: number) => void }) {
  return (
    <label>
      {label}
      <input type="number" value={value} onChange={(event) => onChange(Number(event.target.value))} />
    </label>
  );
}

function renderGeometry(geometry: Geometry) {
  if (geometry.type === "points") return geometry.points.map((point, index) => <circle key={index} cx={point.x ?? 0} cy={point.y ?? 0} r="0.08" />);
  if (geometry.type === "line") return <line x1={geometry.from.x ?? 0} y1={geometry.from.y ?? 0} x2={geometry.to.x ?? 0} y2={geometry.to.y ?? 0} />;
  if (geometry.type === "lines") return geometry.lines.map((line, index) => {
    const from = geometry.points[line.from] ?? { x: 0, y: 0, z: 0 };
    const to = geometry.points[line.to] ?? { x: 0, y: 0, z: 0 };
    return <line key={index} x1={from.x ?? 0} y1={from.y ?? 0} x2={to.x ?? 0} y2={to.y ?? 0} />;
  });
  const center = geometry.center;
  const radius = geometry.radius ?? 1;
  const start = polar(center, radius, geometry.startDegrees ?? 0);
  const end = polar(center, radius, geometry.endDegrees ?? 180);
  return <path d={`M ${start.x} ${start.y} A ${radius} ${radius} 0 0 1 ${end.x} ${end.y}`} />;
}

function defaultGeometry(type: Geometry["type"]): Geometry {
  if (type === "line") return { type, from: { x: -0.5, y: 0, z: 0 }, to: { x: 0.5, y: 0, z: 0 }, pixels: 2 };
  if (type === "lines") return { type, points: [{ x: -0.5, y: 0, z: 0 }, { x: 0.5, y: 0, z: 0 }], lines: [{ from: 0, to: 1 }] };
  if (type === "arc") return { type, center: { x: 0, y: 0, z: 0 }, radius: 1, startDegrees: 0, endDegrees: 180, pixels: 8 };
  return { type, points: [{ x: 0, y: 0, z: 0 }] };
}

function polar(center: Point3, radius: number, degrees: number) {
  const radians = (degrees * Math.PI) / 180;
  return { x: (center.x ?? 0) + radius * Math.cos(radians), y: (center.y ?? 0) + radius * Math.sin(radians) };
}

function uniqueId(base: string, used: string[]) {
  const taken = new Set(used);
  if (!taken.has(base)) return base;
  let index = 2;
  while (taken.has(`${base}_${index}`)) index += 1;
  return `${base}_${index}`;
}
