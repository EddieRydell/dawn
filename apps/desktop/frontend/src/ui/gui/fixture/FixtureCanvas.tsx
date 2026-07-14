import { useEffect, useMemo, useRef, useState, type PointerEvent as ReactPointerEvent } from "react";
import { commands } from "../../../api";
import type { PropDocument } from "../../../types";
import { runGuiEditCommand } from "../../../store";
import { THEME_COLORS, THEME_METRICS } from "../../../theme";
import { BlockedGui } from "../BlockedGui";
import { denormalizePoint, drawSpatialCanvas, nearestPoint, normalizeBounds, normalizePoint, round6, unproject, type GuiFocus, type Point3 } from "../shared";
import { SpatialControls, useSpacePressed, useSpatialViewport } from "../SpatialViewport";

type Gesture = { type: "empty"; x: number; y: number } | { type: "pan"; x: number; y: number } | { type: "point"; objectKey: string; pointIndex: number; draft: Point3 } | null;

export function FixtureCanvas({ document, selected, setSelected }: { document: PropDocument; selected: GuiFocus; setSelected: (id: GuiFocus) => void }) {
  const canvas = useRef<HTMLCanvasElement | null>(null);
  const gesture = useRef<Gesture>(null);
  const [revision, render] = useState(0);
  const [menu, setMenu] = useState<{ x: number; y: number } | null>(null);
  const fixture = document.fixtures.find((candidate) => candidate.objectKey === document.selectedObjectKey) ?? document.fixtures[0];
  const bounds = useMemo(() => fixture === undefined ? null : normalizeBounds(fixture.renderPlan.bounds), [fixture]);
  const spatial = useSpatialViewport(bounds ?? { minX: 0, minY: 0, maxX: 1, maxY: 1 }, fixture?.objectKey, `${document.path}:${fixture?.objectKey ?? ""}`);
  const spacePressed = useSpacePressed();

  useEffect(() => {
    if (!fixture || !bounds) return;
    const rect = canvas.current?.getBoundingClientRect(); if (rect) spatial.resize(rect.width, rect.height);
    drawSpatialCanvas(canvas.current, bounds, (ctx, project) => {
      for (const guide of fixture.renderPlan.guides) {
        if (guide.type !== "line") continue;
        const from = project(normalizePoint(guide.from)); const to = project(normalizePoint(guide.to));
        ctx.strokeStyle = THEME_COLORS.canvasGuide; ctx.beginPath(); ctx.moveTo(from.x, from.y); ctx.lineTo(to.x, to.y); ctx.stroke();
      }
      fixture.renderPlan.emitters.forEach((point, index) => {
        const source = gesture.current?.type === "point" && gesture.current.pointIndex === index ? gesture.current.draft : normalizePoint(point);
        const projected = project(source);
        ctx.fillStyle = selected?.type === "point" && selected.index === index ? THEME_COLORS.accent : THEME_COLORS.playhead;
        ctx.beginPath(); ctx.arc(projected.x, projected.y, THEME_METRICS.spatialPointRadius, 0, Math.PI * 2); ctx.fill();
      });
    }, spatial.view);
  }, [bounds, fixture, revision, selected, spatial]);

  if (!fixture || !bounds) return <BlockedGui reason="No fixture definition is available." diagnostics={[]} />;
  const worldAt = (event: ReactPointerEvent<HTMLCanvasElement>) => { const rect = event.currentTarget.getBoundingClientRect(); return unproject(event.clientX - rect.left, event.clientY - rect.top, canvas.current, bounds, spatial.view); };
  return <div className="spatial-canvas-shell">
    <canvas ref={canvas} className="gui-canvas" tabIndex={0}
      onKeyDown={(event) => { if (event.key === "Home") { event.preventDefault(); spatial.reset(); } }}
      onContextMenu={(event) => { event.preventDefault(); setMenu({ x: event.nativeEvent.offsetX, y: event.nativeEvent.offsetY }); }}
      onPointerDown={(event) => {
        if (event.button === 2) return;
        event.currentTarget.setPointerCapture(event.pointerId); setMenu(null);
        if (event.button === 1 || spacePressed.current) { gesture.current = { type: "pan", x: event.clientX, y: event.clientY }; return; }
        if (fixture.geometry.type !== "points") return;
        const points = fixture.geometry.points.map(normalizePoint); const index = nearestPoint(points, worldAt(event), THEME_METRICS.spatialHitRadius / spatial.view.scale);
        if (index === null) { gesture.current = { type: "empty", x: event.clientX, y: event.clientY }; return; }
        const point = points[index]; if (!point) return;
        setSelected({ type: "point", index }); gesture.current = { type: "point", objectKey: fixture.objectKey, pointIndex: index, draft: point };
      }}
      onPointerMove={(event) => {
        const current = gesture.current; if (!current) return;
        if (current.type === "empty" && Math.hypot(event.clientX - current.x, event.clientY - current.y) > THEME_METRICS.spatialPanThreshold) gesture.current = { type: "pan", x: current.x, y: current.y };
        const active = gesture.current;
        if (active?.type === "pan") { spatial.panBy(event.clientX - active.x, event.clientY - active.y); gesture.current = { ...active, x: event.clientX, y: event.clientY }; return; }
        if (active?.type !== "point") return;
        const world = worldAt(event); active.draft = { x: round6(world.x), y: round6(world.y), z: active.draft.z }; render((value) => value + 1);
      }}
      onPointerUp={() => { const current = gesture.current; gesture.current = null; if (current?.type === "empty") setSelected(null); if (current?.type === "point") void runGuiEditCommand(() => commands.applyPropGuiEdit({ type: "movePoint", objectKey: current.objectKey, pointIndex: current.pointIndex, point: denormalizePoint(current.draft) })); }}
      onPointerCancel={() => { gesture.current = null; }}
      onWheel={(event) => { event.preventDefault(); const rect = event.currentTarget.getBoundingClientRect(); spatial.zoomAt(Math.exp(-event.deltaY * 0.0015), event.clientX - rect.left, event.clientY - rect.top); }}
    />
    {menu && <div className="spatial-context-menu" style={{ left: menu.x, top: menu.y }}><button onClick={() => { spatial.reset(); setMenu(null); }}>Fit view</button><button onClick={() => { setSelected(null); setMenu(null); }}>Deselect</button></div>}
    <SpatialControls view={spatial.view} reset={spatial.reset} zoomAt={(factor) => { const rect = canvas.current?.getBoundingClientRect(); spatial.zoomAt(factor, (rect?.width ?? 0) / 2, (rect?.height ?? 0) / 2); }} />
  </div>;
}
