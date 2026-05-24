import { useEffect, useMemo, useRef, useState, type MouseEvent } from "react";
import { Copy, Crosshair, LocateFixed, Trash2, ZoomIn, ZoomOut } from "lucide-react";
import type {
  FixtureCatalogItem,
  Geometry,
  LayoutDocument,
  LayoutFixturePlacement,
  Point3,
  ResolvedLayoutFixture
} from "../generated/bindings";

type LayoutViewerProps = {
  document: LayoutDocument;
  selectedFixtureId: string | null;
  highlightedGroup: string | null;
  onSelectFixture: (fixtureId: string | null) => void;
  onHighlightGroup: (groupName: string | null) => void;
  onDocumentChange: (document: LayoutDocument) => Promise<void>;
};

type ViewBox = { x: number; y: number; width: number; height: number };
type Bounds = { minX: number; minY: number; maxX: number; maxY: number };
type SvgPoint = { x: number; y: number };
type FixtureRenderPoint = SvgPoint & { key: string };

const defaultBulbSize = 1;
const bulbSizeUnitRadius = 0.035;

export function LayoutViewer({
  document,
  selectedFixtureId,
  highlightedGroup,
  onSelectFixture,
  onHighlightGroup,
  onDocumentChange
}: LayoutViewerProps) {
  const svgRef = useRef<SVGSVGElement | null>(null);
  const panRef = useRef<{ x: number; y: number; viewBox: ViewBox } | null>(null);
  const dragRef = useRef<{ fixtureId: string; offset: SvgPoint } | null>(null);
  const documentKeyRef = useRef<string | null>(null);
  const [dragPreview, setDragPreview] = useState<{ fixtureId: string; position: Point3 } | null>(null);
  const bounds = useMemo(() => documentBounds(document), [document]);
  const [viewportAspect, setViewportAspect] = useState(16 / 9);
  const [viewBox, setViewBox] = useState<ViewBox>(() => fitViewBox(bounds, 16 / 9));

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
    const documentKey = `${document.path}::${document.objectKey}`;
    if (documentKeyRef.current === documentKey) return;
    documentKeyRef.current = documentKey;
    setViewBox(fitViewBox(bounds, viewportAspect));
  }, [bounds, document.objectKey, document.path, viewportAspect]);

  useEffect(() => {
    setViewBox((current) => matchViewBoxAspect(current, viewportAspect));
  }, [viewportAspect]);

  const selectedFixture = document.fixtures.find((fixture) => fixture.id === selectedFixtureId) ?? null;
  const renderedDocument = dragPreview
    ? updatePlacement(document, dragPreview.fixtureId, (fixture) => ({
      ...fixture,
      transform: { ...fixture.transform, position: dragPreview.position }
    }))
    : document;
  const highlightedMembers = useMemo(() => {
    const group = document.groups.find((item) => item.name === highlightedGroup);
    return new Set(group?.members ?? []);
  }, [document.groups, highlightedGroup]);

  const commitDrag = (event: MouseEvent<SVGSVGElement>) => {
    const drag = dragRef.current;
    if (!drag) return;
    const point = svgEventPoint(event, svgRef.current);
    const position = { x: point.x - drag.offset.x, y: -point.y - drag.offset.y, z: 0 };
    setDragPreview(null);
    void onDocumentChange(updatePlacement(document, drag.fixtureId, (fixture) => ({
      ...fixture,
      transform: { ...fixture.transform, position }
    })));
  };

  return (
    <div className="layout-viewer">
      <div className="layout-canvas-column">
        <div className="layout-canvas-toolbar">
          <button title="Fit to view" onClick={() => setViewBox(fitViewBox(bounds, viewportAspect))}><LocateFixed size={15} /></button>
          <button title="Zoom in" onClick={() => setViewBox((box) => zoomViewBox(box, 0.82))}><ZoomIn size={15} /></button>
          <button title="Zoom out" onClick={() => setViewBox((box) => zoomViewBox(box, 1.18))}><ZoomOut size={15} /></button>
          <span>{document.units}</span>
        </div>
        <svg
          ref={svgRef}
          className="layout-canvas"
          viewBox={`${viewBox.x} ${viewBox.y} ${viewBox.width} ${viewBox.height}`}
          onMouseDown={(event) => {
            if (event.button !== 0 || (event.target as Element).closest(".layout-fixture-shape")) return;
            panRef.current = { x: event.clientX, y: event.clientY, viewBox };
          }}
          onMouseMove={(event) => {
            const drag = dragRef.current;
            if (drag) {
              const point = svgEventPoint(event, svgRef.current);
              setDragPreview({
                fixtureId: drag.fixtureId,
                position: { x: point.x - drag.offset.x, y: -point.y - drag.offset.y, z: 0 }
              });
              return;
            }
            const pan = panRef.current;
            if (!pan) return;
            const dx = ((event.clientX - pan.x) / svgPixelWidth(svgRef.current)) * pan.viewBox.width;
            const dy = ((event.clientY - pan.y) / svgPixelHeight(svgRef.current)) * pan.viewBox.height;
            setViewBox({ ...pan.viewBox, x: pan.viewBox.x - dx, y: pan.viewBox.y - dy });
          }}
          onMouseUp={(event) => {
            commitDrag(event);
            dragRef.current = null;
            panRef.current = null;
          }}
          onMouseLeave={(event) => {
            commitDrag(event);
            dragRef.current = null;
            panRef.current = null;
          }}
          onWheel={(event) => {
            event.preventDefault();
            setViewBox((box) => zoomViewBox(box, event.deltaY < 0 ? 0.9 : 1.1));
          }}
          onClick={(event) => {
            if (event.target === event.currentTarget) onSelectFixture(null);
          }}
        >
          <defs>
            <pattern id="layout-grid" width="1" height="1" patternUnits="userSpaceOnUse">
              <path d="M 1 0 L 0 0 0 1" fill="none" stroke="rgba(255,255,255,0.07)" strokeWidth="0.015" />
            </pattern>
          </defs>
          <rect x={viewBox.x} y={viewBox.y} width={viewBox.width} height={viewBox.height} fill="url(#layout-grid)" />
          <line x1={viewBox.x} y1="0" x2={viewBox.x + viewBox.width} y2="0" className="layout-axis" />
          <line x1="0" y1={viewBox.y} x2="0" y2={viewBox.y + viewBox.height} className="layout-axis" />
          <g transform="scale(1 -1)">
            {renderedDocument.fixtures.map((fixture) => (
              <FixtureShape
                key={fixture.id}
                fixture={fixture}
                selected={fixture.id === selectedFixtureId}
                highlighted={highlightedMembers.has(fixture.id)}
                onMouseDown={(event) => {
                  const point = svgEventPoint(event, svgRef.current);
                  dragRef.current = {
                    fixtureId: fixture.id,
                    offset: {
                      x: point.x - (fixture.transform.position.x ?? 0),
                      y: -point.y - (fixture.transform.position.y ?? 0)
                    }
                  };
                  onSelectFixture(fixture.id);
                }}
              />
            ))}
          </g>
          <ScaleBar viewBox={viewBox} units={document.units} svg={svgRef.current} />
        </svg>
      </div>
      <aside className="layout-inspector">
        <section className="layout-list-section">
          <h3>Fixtures</h3>
          <select
            className="layout-object-select"
            defaultValue=""
            disabled={!document.fixtureCatalog.length}
            onChange={(event) => {
              const item = document.fixtureCatalog.find((fixture) => fixture.importString === event.target.value);
              if (!item) return;
              const next = addPlacementFromCatalog(document, item, {
                x: viewBox.x + viewBox.width / 2,
                y: -(viewBox.y + viewBox.height / 2),
                z: 0
              });
              const id = next.fixtures[next.fixtures.length - 1]?.id ?? null;
              onSelectFixture(id);
              void onDocumentChange(next);
              event.currentTarget.value = "";
            }}
          >
            <option value="">Add fixture</option>
            {document.fixtureCatalog.map((item) => (
              <option key={item.importString} value={item.importString}>{item.displayName} ({item.objectKey})</option>
            ))}
          </select>
          <div className="layout-fixture-list">
            {document.fixtures.map((fixture) => (
              <button
                key={fixture.id}
                className={fixture.id === selectedFixtureId ? "active" : highlightedMembers.has(fixture.id) ? "highlighted" : ""}
                onClick={() => onSelectFixture(fixture.id)}
              >
                <span>{fixture.id}</span>
                <small>{fixture.resolvedFixture.name}</small>
              </button>
            ))}
          </div>
        </section>
        <section className="layout-list-section">
          <h3>Groups</h3>
          <div className="layout-group-list">
            <button className={!highlightedGroup ? "active" : ""} onClick={() => onHighlightGroup(null)}>All</button>
            {document.groups.map((group) => (
              <button key={group.name} className={group.name === highlightedGroup ? "active" : ""} onClick={() => onHighlightGroup(group.name)}>
                <span>{group.name}</span>
                <small>{group.members.length}</small>
              </button>
            ))}
          </div>
        </section>
        <FixtureDetails
          fixture={selectedFixture}
          document={document}
          onSelectFixture={onSelectFixture}
          onDocumentChange={onDocumentChange}
        />
      </aside>
    </div>
  );
}

function ScaleBar({ viewBox, units, svg }: { viewBox: ViewBox; units: string; svg: SVGSVGElement | null }) {
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
      <text x={x} y={y - tick * 1.8} fontSize={fontSize} strokeWidth={labelStrokeWidth}>{formatScaleLength(length)} {unitLabel(units)}</text>
    </g>
  );
}

function FixtureShape({
  fixture,
  selected,
  highlighted,
  onMouseDown
}: {
  fixture: LayoutFixturePlacement;
  selected: boolean;
  highlighted: boolean;
  onMouseDown: (event: MouseEvent<SVGGElement>) => void;
}) {
  const className = `layout-fixture-shape${selected ? " selected" : ""}${highlighted ? " highlighted" : ""}`;
  return (
    <g
      className={className}
      onMouseDown={(event) => {
        event.stopPropagation();
        onMouseDown(event);
      }}
      onClick={(event) => event.stopPropagation()}
    >
      {renderGeometry(fixture.resolvedFixture.geometry, fixture) ?? (
        <circle cx={fixture.transform.position.x ?? 0} cy={fixture.transform.position.y ?? 0} r={fixtureBulbRadius(fixture)} />
      )}
    </g>
  );
}

function FixtureDetails({
  fixture,
  document,
  onSelectFixture,
  onDocumentChange
}: {
  fixture: LayoutFixturePlacement | null;
  document: LayoutDocument;
  onSelectFixture: (fixtureId: string | null) => void;
  onDocumentChange: (document: LayoutDocument) => Promise<void>;
}) {
  if (!fixture) {
    return <section className="layout-details empty"><Crosshair size={18} /><span>No fixture selected</span></section>;
  }

  return (
    <section className="layout-details">
      <div className="layout-details-heading">
        <input
          aria-label="Placement id"
          value={fixture.id}
          onChange={(event) => {
            const nextId = event.target.value;
            onSelectFixture(nextId);
            void onDocumentChange(updatePlacement(document, fixture.id, (item) => ({ ...item, id: nextId })));
          }}
        />
        <button title="Duplicate" onClick={() => {
          const next = duplicatePlacement(document, fixture);
          onSelectFixture(next.fixtures[next.fixtures.length - 1]?.id ?? null);
          void onDocumentChange(next);
        }}><Copy size={14} /></button>
        <button title="Delete" onClick={() => {
          onSelectFixture(null);
          void onDocumentChange({
            ...document,
            fixtures: document.fixtures.filter((item) => item.id !== fixture.id),
            groups: document.groups.map((group) => ({ ...group, members: group.members.filter((member) => member !== fixture.id) }))
          });
        }}><Trash2 size={14} /></button>
      </div>
      <label>
        Fixture
        <select
          value={fixture.fixture.type === "import" ? fixture.fixture.import : ""}
          onChange={(event) => {
            const catalogItem = document.fixtureCatalog.find((item) => item.importString === event.target.value);
            if (!catalogItem) return;
            void onDocumentChange(updatePlacement(document, fixture.id, (item) => ({
              ...item,
              fixture: { type: "import", import: catalogItem.importString, objectKey: catalogItem.objectKey, sourcePath: catalogItem.sourcePath },
              resolvedFixture: catalogToResolvedFixture(catalogItem)
            })));
          }}
        >
          <option value="">{authoredFixtureLabel(fixture)}</option>
          {document.fixtureCatalog.map((item) => (
            <option key={item.importString} value={item.importString}>{item.displayName} ({item.objectKey})</option>
          ))}
        </select>
      </label>
      <PointEditor label="Position" point={fixture.transform.position} onChange={(position) =>
        onDocumentChange(updatePlacement(document, fixture.id, (item) => ({ ...item, transform: { ...item.transform, position } })))
      } />
      <PointEditor label="Rotation" point={fixture.transform.rotation ?? { x: 0, y: 0, z: 0 }} onChange={(rotation) =>
        onDocumentChange(updatePlacement(document, fixture.id, (item) => ({ ...item, transform: { ...item.transform, rotation } })))
      } />
      <PointEditor label="Scale" point={fixture.transform.scale ?? { x: 1, y: 1, z: 1 }} onChange={(scale) =>
        onDocumentChange(updatePlacement(document, fixture.id, (item) => ({ ...item, transform: { ...item.transform, scale } })))
      } />
      <dl>
        <dt>Resolved</dt>
        <dd>{fixture.resolvedFixture.name}</dd>
        <dt>Geometry</dt>
        <dd>{fixture.resolvedFixture.geometrySummary}</dd>
        <dt>Source</dt>
        <dd>{fixture.resolvedFixture.sourcePath}{fixture.resolvedFixture.objectKey ? `::${fixture.resolvedFixture.objectKey}` : ""}</dd>
      </dl>
    </section>
  );
}

function PointEditor({ label, point, onChange }: { label: string; point: Point3; onChange: (point: Point3) => void }) {
  return (
    <fieldset className="point-editor">
      <legend>{label}</legend>
      {(["x", "y", "z"] as const).map((axis) => (
        <NumericInput
          key={axis}
          ariaLabel={`${label} ${axis}`}
          step={0.1}
          value={point[axis] ?? 0}
          onChange={(value) => onChange({ ...point, [axis]: value })}
        />
      ))}
    </fieldset>
  );
}

function NumericInput({
  value,
  step,
  ariaLabel,
  onChange
}: {
  value: number;
  step?: number;
  ariaLabel?: string;
  onChange: (value: number) => void;
}) {
  const [draft, setDraft] = useState(() => formatNumberInput(value));
  const [focused, setFocused] = useState(false);

  useEffect(() => {
    if (!focused) setDraft(formatNumberInput(value));
  }, [focused, value]);

  return (
    <input
      aria-label={ariaLabel}
      type="number"
      step={step}
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

function renderGeometry(geometry: Geometry, fixture: LayoutFixturePlacement) {
  const bulbRadius = fixtureBulbRadius(fixture);
  switch (geometry.type) {
    case "points":
      return renderEmitterPoints(pointsToRenderPoints(geometry.points, fixture), bulbRadius);
    case "lines": {
      const emitters = pointsToRenderPoints(samplePolylinePoints(geometry.points, geometry.pixels), fixture);
      return (
        <>
          {geometry.points.slice(0, -1).map((point, index) => {
            const from = transformPoint(point, fixture);
            const to = transformPoint(geometry.points[index + 1], fixture);
            return from && to ? <line key={index} className="layout-fixture-guide" x1={from.x} y1={from.y} x2={to.x} y2={to.y} /> : null;
          })}
          {renderEmitterPoints(emitters, bulbRadius)}
        </>
      );
    }
    case "arc": {
      const center = transformPoint(geometry.center, fixture);
      if (!center || geometry.radius == null || geometry.startDegrees == null || geometry.endDegrees == null) return null;
      const arcPoints = sampleArcPoints(geometry.center, geometry.radius, geometry.startDegrees, geometry.endDegrees, geometry.pixels);
      const emitters = pointsToRenderPoints(arcPoints, fixture);
      const start = emitters[0];
      const end = emitters[emitters.length - 1];
      if (!start || !end) return null;
      const largeArc = Math.abs(geometry.endDegrees - geometry.startDegrees) > 180 ? 1 : 0;
      const scale = fixture.transform.scale ?? { x: 1, y: 1, z: 1 };
      const radiusX = Math.abs(geometry.radius * (scale.x ?? 1));
      const radiusY = Math.abs(geometry.radius * (scale.y ?? 1));
      const rotation = fixture.transform.rotation?.z ?? 0;
      return (
        <>
          <path className="layout-fixture-guide" d={`M ${start.x} ${start.y} A ${radiusX} ${radiusY} ${rotation} ${largeArc} 1 ${end.x} ${end.y}`} />
          {renderEmitterPoints(emitters, bulbRadius)}
        </>
      );
    }
  }
}

function renderEmitterPoints(points: FixtureRenderPoint[], radius: number) {
  return points.map((point) => (
    <circle key={point.key} className="layout-fixture-emitter" cx={point.x} cy={point.y} r={radius} />
  ));
}

function fixtureBulbRadius(fixture: LayoutFixturePlacement) {
  return bulbRadius(fixture.resolvedFixture.bulbSize);
}

function normalizedBulbSize(value: number | null | undefined) {
  return Math.max(0.05, value ?? defaultBulbSize);
}

function bulbRadius(value: number | null | undefined) {
  return normalizedBulbSize(value) * bulbSizeUnitRadius;
}

function pointsToRenderPoints(points: Point3[], fixture: LayoutFixturePlacement): FixtureRenderPoint[] {
  return points.flatMap((point, index) => {
    const transformed = transformPoint(point, fixture);
    return transformed ? [{ ...transformed, key: `emitter-${index}` }] : [];
  });
}

function samplePolylinePoints(points: Point3[], pixels: number): Point3[] {
  const count = Math.max(1, Math.floor(pixels));
  if (points.length === 0) return [];
  if (points.length === 1) return [{ ...points[0] }];

  const segments = points.slice(0, -1).map((from, index) => ({
    from,
    to: points[index + 1],
    length: pointDistance(from, points[index + 1])
  }));
  const totalLength = segments.reduce((sum, segment) => sum + segment.length, 0);
  if (totalLength === 0) return Array.from({ length: count }, () => ({ ...points[0] }));

  if (count === 1) return [pointAtDistance(segments, totalLength / 2)];
  return Array.from({ length: count }, (_, index) =>
    pointAtDistance(segments, totalLength * (index / (count - 1)))
  );
}

function pointAtDistance(segments: { from: Point3; to: Point3; length: number }[], distance: number): Point3 {
  let remaining = distance;
  for (const segment of segments) {
    if (segment.length === 0) continue;
    if (remaining <= segment.length) return interpolatePoint(segment.from, segment.to, remaining / segment.length);
    remaining -= segment.length;
  }
  const last = segments[segments.length - 1]?.to ?? { x: 0, y: 0, z: 0 };
  return { ...last };
}

function interpolatePoint(from: Point3, to: Point3, t: number): Point3 {
  return {
    x: lerp(from.x ?? 0, to.x ?? 0, t),
    y: lerp(from.y ?? 0, to.y ?? 0, t),
    z: lerp(from.z ?? 0, to.z ?? 0, t)
  };
}

function pointDistance(from: Point3, to: Point3) {
  const dx = (to.x ?? 0) - (from.x ?? 0);
  const dy = (to.y ?? 0) - (from.y ?? 0);
  const dz = (to.z ?? 0) - (from.z ?? 0);
  return Math.sqrt(dx * dx + dy * dy + dz * dz);
}

function sampleArcPoints(center: Point3, radius: number, startDegrees: number, endDegrees: number, pixels: number): Point3[] {
  const count = Math.max(1, Math.floor(pixels));
  if (count === 1) {
    return [arcPoint(center, radius, (startDegrees + endDegrees) / 2)];
  }

  return Array.from({ length: count }, (_, index) =>
    arcPoint(center, radius, lerp(startDegrees, endDegrees, index / (count - 1)))
  );
}

function arcPoint(center: Point3, radius: number, degrees: number): Point3 {
  const radians = (degrees * Math.PI) / 180;
  return {
    x: (center.x ?? 0) + radius * Math.cos(radians),
    y: (center.y ?? 0) + radius * Math.sin(radians),
    z: center.z ?? 0
  };
}

function lerp(from: number, to: number, t: number) {
  return from + (to - from) * t;
}

function updatePlacement(document: LayoutDocument, id: string, update: (fixture: LayoutFixturePlacement) => LayoutFixturePlacement): LayoutDocument {
  return { ...document, fixtures: document.fixtures.map((fixture) => fixture.id === id ? update(fixture) : fixture) };
}

function duplicatePlacement(document: LayoutDocument, fixture: LayoutFixturePlacement): LayoutDocument {
  const id = uniqueId(`${fixture.id}_copy`, document.fixtures.map((item) => item.id));
  return {
    ...document,
    fixtures: [...document.fixtures, { ...fixture, id, transform: { ...fixture.transform, position: { ...(fixture.transform.position), x: (fixture.transform.position.x ?? 0) + 0.5 } } }]
  };
}

function addPlacementFromCatalog(document: LayoutDocument, item: FixtureCatalogItem, position: Point3): LayoutDocument {
  const id = uniqueId(slug(item.displayName || item.objectKey), document.fixtures.map((fixture) => fixture.id));
  return {
    ...document,
    fixtures: [
      ...document.fixtures,
      {
        id,
        fixture: { type: "import", import: item.importString, objectKey: item.objectKey, sourcePath: item.sourcePath },
        resolvedFixture: catalogToResolvedFixture(item),
        transform: { position, rotation: { x: 0, y: 0, z: 0 }, scale: { x: 1, y: 1, z: 1 } }
      }
    ]
  };
}

function catalogToResolvedFixture(item: FixtureCatalogItem): ResolvedLayoutFixture {
  return {
    name: item.displayName,
    colorModel: item.colorModel,
    bulbSize: item.bulbSize,
    geometry: item.geometry,
    geometrySummary: item.geometrySummary,
    sourcePath: item.sourcePath,
    objectKey: item.objectKey
  };
}

function transformPoint(point: Point3 | undefined, fixture: LayoutFixturePlacement): SvgPoint | null {
  if (!point || point.x == null || point.y == null) return null;
  const position = fixture.transform.position;
  const scale = fixture.transform.scale ?? { x: 1, y: 1, z: 1 };
  const rotation = fixture.transform.rotation ?? { x: 0, y: 0, z: 0 };
  const radians = ((rotation.z ?? 0) * Math.PI) / 180;
  const x = point.x * (scale.x ?? 1);
  const y = point.y * (scale.y ?? 1);
  return {
    x: (position.x ?? 0) + x * Math.cos(radians) - y * Math.sin(radians),
    y: (position.y ?? 0) + x * Math.sin(radians) + y * Math.cos(radians)
  };
}

function documentBounds(document: LayoutDocument): Bounds {
  const points = document.fixtures.flatMap(fixturePoints);
  if (!points.length) return { minX: -5, minY: -4, maxX: 5, maxY: 4 };
  return points.reduce((bounds, point) => ({
    minX: Math.min(bounds.minX, point.x),
    minY: Math.min(bounds.minY, point.y),
    maxX: Math.max(bounds.maxX, point.x),
    maxY: Math.max(bounds.maxY, point.y)
  }), { minX: points[0].x, minY: points[0].y, maxX: points[0].x, maxY: points[0].y });
}

function fixturePoints(fixture: LayoutFixturePlacement): SvgPoint[] {
  const geometry = fixture.resolvedFixture.geometry;
  if (geometry.type === "points") return geometry.points.map((point) => transformPoint(point, fixture)).filter(Boolean) as SvgPoint[];
  if (geometry.type === "lines") return geometry.points.map((point) => transformPoint(point, fixture)).filter(Boolean) as SvgPoint[];
  const center = transformPoint(geometry.center, fixture);
  if (!center || geometry.radius == null) return [transformPoint({ ...fixture.transform.position }, fixture)].filter(Boolean) as SvgPoint[];
  return [{ x: center.x - geometry.radius, y: center.y - geometry.radius }, { x: center.x + geometry.radius, y: center.y + geometry.radius }];
}

function fitViewBox(bounds: Bounds, viewportAspect: number): ViewBox {
  const width = Math.max(bounds.maxX - bounds.minX, 1);
  const height = Math.max(bounds.maxY - bounds.minY, 1);
  const padding = Math.max(width, height) * 0.18 + 0.5;
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
  const width = Math.max(viewBox.width * factor, 0.25);
  const height = Math.max(viewBox.height * factor, 0.25);
  return { x: viewBox.x + (viewBox.width - width) / 2, y: viewBox.y + (viewBox.height - height) / 2, width, height };
}

function matchViewBoxAspect(viewBox: ViewBox, viewportAspect: number): ViewBox {
  const aspect = Math.max(viewportAspect, 0.1);
  let width = viewBox.width;
  let height = viewBox.height;
  if (width / height > aspect) {
    height = width / aspect;
  } else {
    width = height * aspect;
  }
  return {
    x: viewBox.x + (viewBox.width - width) / 2,
    y: viewBox.y + (viewBox.height - height) / 2,
    width,
    height
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

function unitLabel(units: string) {
  return units === "meters" ? "m" : "ft";
}

function svgEventPoint(event: MouseEvent, svg: SVGSVGElement | null): SvgPoint {
  if (!svg) return { x: 0, y: 0 };
  const point = svg.createSVGPoint();
  point.x = event.clientX;
  point.y = event.clientY;
  const transformed = point.matrixTransform(svg.getScreenCTM()?.inverse());
  return { x: transformed.x, y: transformed.y };
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

function authoredFixtureLabel(fixture: LayoutFixturePlacement) {
  return fixture.fixture.type === "import" ? fixture.fixture.import : fixture.fixture.name;
}

function slug(value: string) {
  const candidate = value.toLowerCase().replace(/[^a-z0-9_]+/g, "_").replace(/^_+|_+$/g, "");
  return /^[a-z_]/.test(candidate) ? candidate : `fixture_${candidate || "new"}`;
}

function uniqueId(base: string, used: string[]) {
  const taken = new Set(used);
  if (!taken.has(base)) return base;
  let index = 2;
  while (taken.has(`${base}_${index}`)) index += 1;
  return `${base}_${index}`;
}
