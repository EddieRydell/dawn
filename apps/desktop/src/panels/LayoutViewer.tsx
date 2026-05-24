import { useEffect, useMemo, useRef, useState, type MouseEvent } from "react";
import { Copy, Crosshair, LocateFixed, Trash2, ZoomIn, ZoomOut } from "lucide-react";
import type {
  FixtureCatalogItem,
  LayoutDocument,
  LayoutFixturePlacement,
  Point3,
  ResolvedLayoutFixture
} from "../generated/bindings";
import {
  fixtureTransform,
  fitViewBox,
  matchViewBoxAspect,
  renderGeometryPlan,
  ScaleBar,
  svgEventPoint,
  svgPixelHeight,
  svgPixelWidth,
  type SvgPoint,
  type ViewBox,
  zoomViewBox
} from "./geometryRender";

type LayoutViewerProps = {
  document: LayoutDocument;
  selectedFixtureId: string | null;
  highlightedGroup: string | null;
  onSelectFixture: (fixtureId: string | null) => void;
  onHighlightGroup: (groupName: string | null) => void;
  onDocumentChange: (document: LayoutDocument) => Promise<void>;
};

const viewBoxOptions = { minSize: 1, paddingScale: 0.18, paddingBase: 0.5 };

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
  const bounds = document.renderBounds;
  const [viewportAspect, setViewportAspect] = useState(16 / 9);
  const [viewBox, setViewBox] = useState<ViewBox>(() => fitViewBox(bounds, 16 / 9, viewBoxOptions));

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
    setViewBox(fitViewBox(bounds, viewportAspect, viewBoxOptions));
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
          <button title="Fit to view" onClick={() => setViewBox(fitViewBox(bounds, viewportAspect, viewBoxOptions))}><LocateFixed size={15} /></button>
          <button title="Zoom in" onClick={() => setViewBox((box) => zoomViewBox(box, 0.82, 0.25))}><ZoomIn size={15} /></button>
          <button title="Zoom out" onClick={() => setViewBox((box) => zoomViewBox(box, 1.18, 0.25))}><ZoomOut size={15} /></button>
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
            setViewBox((box) => zoomViewBox(box, event.deltaY < 0 ? 0.9 : 1.1, 0.25));
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
          <ScaleBar viewBox={viewBox} units={unitLabel(document.units)} svg={svgRef.current} />
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
      transform={fixtureTransform(fixture.transform)}
      onMouseDown={(event) => {
        event.stopPropagation();
        onMouseDown(event);
      }}
      onClick={(event) => event.stopPropagation()}
    >
      {renderGeometryPlan(fixture.resolvedFixture.renderPlan)}
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
    renderPlan: item.renderPlan,
    sourcePath: item.sourcePath,
    objectKey: item.objectKey
  };
}

function unitLabel(units: string) {
  return units === "meters" ? "m" : "ft";
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
