import { useEffect, useMemo, useRef, useState } from "react";
import { commands } from "../api";
import { AppSnapshotDto } from "../bindings";
import { runSnapshotCommand } from "../store";

type Point3 = { x: number; y: number; z: number };
type Transform = { position: Point3; rotation: Point3; scale: Point3 };
type DragState =
  | null
  | { kind: "sequence"; id: number; startX: number; originalStartMs: number; laneIndex: number; resize: "none" | "left" | "right" }
  | { kind: "layout"; id: number; startX: number; startY: number; original: Transform; preview: Transform }
  | { kind: "fixturePoint"; objectKey: string; pointIndex: number; preview: Point3 };

export function GuiEditor({ snapshot }: { snapshot: AppSnapshotDto }) {
  const gui = snapshot.activeGuiDocument;
  const [selected, setSelected] = useState<string | null>(null);

  useEffect(() => {
    setSelected(null);
  }, [snapshot.activeFile, gui?.type]);

  if (!gui) {
    return <BlockedGui reason="GUI data is not available for this document." diagnostics={[]} />;
  }
  if (gui.type === "blocked") {
    return <BlockedGui reason={gui.reason} diagnostics={gui.diagnostics} />;
  }

  return (
    <div className="gui-editor-shell">
      {gui.type === "sequence" && (
        <SequenceCanvas document={gui.document} selected={selected} setSelected={setSelected} />
      )}
      {gui.type === "layout" && <LayoutCanvas document={gui.document} selected={selected} setSelected={setSelected} />}
      {gui.type === "fixture" && (
        <FixtureCanvas document={gui.document} selected={selected} setSelected={setSelected} />
      )}
      <GuiInspector gui={gui} selected={selected} />
    </div>
  );
}

function BlockedGui({
  reason,
  diagnostics
}: {
  reason: string;
  diagnostics: AppSnapshotDto["diagnostics"];
}) {
  return (
    <div className="gui-blocked">
      <strong>{reason}</strong>
      {diagnostics.length > 0 && (
        <div className="gui-diagnostics">
          {diagnostics.map((diagnostic, index) => (
            <div key={`${diagnostic.path}-${index}`}>
              {diagnostic.range ? `${diagnostic.range.start.line + 1}:${diagnostic.range.start.character + 1} ` : ""}
              {diagnostic.message}
            </div>
          ))}
        </div>
      )}
    </div>
  );
}

function SequenceCanvas({
  document,
  selected,
  setSelected
}: {
  document: any;
  selected: string | null;
  setSelected: (id: string | null) => void;
}) {
  const canvas = useRef<HTMLCanvasElement | null>(null);
  const drag = useRef<DragState>(null);
  const [preview, setPreview] = useState<{ id: number; startMs: number; durationMs: number; laneIndex: number } | null>(null);
  const pxPerMs = 0.08;
  const laneHeight = 42;
  const left = 128;

  useEffect(() => {
    const target = canvas.current;
    if (!target) return;
    const rect = target.getBoundingClientRect();
    const dpr = window.devicePixelRatio || 1;
    target.width = Math.max(1, Math.floor(rect.width * dpr));
    target.height = Math.max(1, Math.floor(rect.height * dpr));
    const ctx = target.getContext("2d");
    if (!ctx) return;
    ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
    ctx.clearRect(0, 0, rect.width, rect.height);
    ctx.fillStyle = "#17181b";
    ctx.fillRect(0, 0, rect.width, rect.height);
    ctx.font = "12px Inter, sans-serif";
    document.lanes.forEach((lane: any, index: number) => {
      const y = 28 + index * laneHeight;
      ctx.fillStyle = index % 2 === 0 ? "#1d1f23" : "#202226";
      ctx.fillRect(0, y, rect.width, laneHeight);
      ctx.fillStyle = "#c7c0b6";
      ctx.fillText(lane.label, 12, y + 25);
    });
    ctx.strokeStyle = "#373b42";
    ctx.beginPath();
    ctx.moveTo(left, 0);
    ctx.lineTo(left, rect.height);
    ctx.stroke();
    ctx.fillStyle = "#a8a29a";
    ctx.fillText("00:00", left + 4, 18);
    ctx.fillText(`${Math.round(document.durationMs / 1000)}s`, left + document.durationMs * pxPerMs - 22, 18);
    const clips = document.effects.map((effect: any) => {
      const activePreview = preview?.id === effect.id ? preview : null;
      if (!activePreview) return effect;
      return {
        ...effect,
        startMs: activePreview.startMs,
        durationMs: activePreview.durationMs,
        target: document.lanes[activePreview.laneIndex]?.target ?? effect.target,
        targetLabel: document.lanes[activePreview.laneIndex]?.label ?? effect.targetLabel
      };
    });
    for (const effect of clips) {
      const laneIndex = Math.max(0, document.lanes.findIndex((lane: any) => targetsEqual(lane.target, effect.target)));
      const x = left + effect.startMs * pxPerMs;
      const y = 35 + laneIndex * laneHeight;
      const width = Math.max(12, effect.durationMs * pxPerMs);
      ctx.fillStyle = selected === `effect:${effect.id}` ? "#6abf8a" : "#456a83";
      ctx.fillRect(x, y, width, 26);
      ctx.fillStyle = "#fffaf0";
      ctx.fillText(effect.script, x + 6, y + 17);
    }
    ctx.strokeStyle = "#d6a35a";
    ctx.beginPath();
    ctx.moveTo(left, 24);
    ctx.lineTo(left, rect.height);
    ctx.stroke();
  }, [document, left, preview, selected]);

  return (
    <canvas
      ref={canvas}
      className="gui-canvas"
      onMouseDown={(event) => {
        const hit = hitSequence(document, event.nativeEvent.offsetX, event.nativeEvent.offsetY, left, pxPerMs, laneHeight);
        if (!hit) {
          setSelected(null);
          return;
        }
        setSelected(`effect:${hit.effect.id}`);
        drag.current = {
          kind: "sequence",
          id: hit.effect.id,
          startX: event.nativeEvent.offsetX,
          originalStartMs: hit.effect.startMs,
          laneIndex: hit.laneIndex,
          resize: hit.resize
        };
        setPreview({
          id: hit.effect.id,
          startMs: hit.effect.startMs,
          durationMs: hit.effect.durationMs,
          laneIndex: hit.laneIndex
        });
      }}
      onMouseMove={(event) => {
        const current = drag.current;
        if (!current || current.kind !== "sequence") return;
        const effect = document.effects.find((candidate: any) => candidate.id === current.id);
        if (!effect) return;
        const deltaMs = Math.round((event.nativeEvent.offsetX - current.startX) / pxPerMs / 50) * 50;
        const laneIndex = clamp(Math.floor((event.nativeEvent.offsetY - 28) / laneHeight), 0, document.lanes.length - 1);
        if (current.resize === "left") {
          const startMs = clamp(current.originalStartMs + deltaMs, 0, effect.startMs + effect.durationMs - 50);
          setPreview({ id: effect.id, startMs, durationMs: effect.startMs + effect.durationMs - startMs, laneIndex });
        } else if (current.resize === "right") {
          setPreview({ id: effect.id, startMs: effect.startMs, durationMs: Math.max(50, effect.durationMs + deltaMs), laneIndex });
        } else {
          setPreview({ id: effect.id, startMs: clamp(current.originalStartMs + deltaMs, 0, document.durationMs), durationMs: effect.durationMs, laneIndex });
        }
      }}
      onMouseUp={() => {
        const current = drag.current;
        drag.current = null;
        if (!current || current.kind !== "sequence" || !preview) return;
        if (current.resize === "none") {
          void runSnapshotCommand(() =>
            commands.applySequenceGuiEdit({
              type: "moveEffect",
              id: preview.id,
              startMs: preview.startMs,
              target: document.lanes[preview.laneIndex]?.target ?? null
            })
          );
        } else {
          void runSnapshotCommand(() =>
            commands.applySequenceGuiEdit({
              type: "resizeEffect",
              id: preview.id,
              startMs: preview.startMs,
              durationMs: preview.durationMs
            })
          );
        }
        setPreview(null);
      }}
    />
  );
}

function LayoutCanvas({
  document,
  selected,
  setSelected
}: {
  document: any;
  selected: string | null;
  setSelected: (id: string | null) => void;
}) {
  const canvas = useRef<HTMLCanvasElement | null>(null);
  const drag = useRef<DragState>(null);
  const [revision, render] = useState(0);
  const viewport = useMemo(() => fitViewport(document.renderBounds), [document.renderBounds]);

  useEffect(() => {
    drawSpatialCanvas(canvas.current, document.renderBounds, (ctx, project) => {
      for (const fixture of document.fixtures) {
        const transform = (drag.current?.kind === "layout" && drag.current.id === fixture.id ? drag.current.preview : fixture.transform) as Transform;
        const center = project(transform.position);
        ctx.fillStyle = selected === `placement:${fixture.id}` ? "#6abf8a" : "#d6a35a";
        ctx.beginPath();
        ctx.arc(center.x, center.y, 7, 0, Math.PI * 2);
        ctx.fill();
        ctx.fillStyle = "#ebe7df";
        ctx.fillText(fixture.name, center.x + 10, center.y - 8);
        for (const emitter of fixture.resolvedFixture.renderPlan.emitters) {
          const point = project({
            x: transform.position.x + emitter.x * transform.scale.x,
            y: transform.position.y + emitter.y * transform.scale.y,
            z: transform.position.z + emitter.z * transform.scale.z
          });
          ctx.fillStyle = "#8ecae6";
          ctx.fillRect(point.x - 2, point.y - 2, 4, 4);
        }
      }
    });
  }, [document, selected, viewport, revision]);

  return (
    <canvas
      ref={canvas}
      className="gui-canvas"
      onMouseDown={(event) => {
        const world = unproject(event.nativeEvent.offsetX, event.nativeEvent.offsetY, canvas.current, document.renderBounds);
        const hit = nearestPlacement(document, world);
        if (!hit) {
          setSelected(null);
          return;
        }
        setSelected(`placement:${hit.id}`);
        drag.current = {
          kind: "layout",
          id: hit.id,
          startX: world.x,
          startY: world.y,
          original: hit.transform,
          preview: hit.transform
        };
      }}
      onMouseMove={(event) => {
        const current = drag.current;
        if (!current || current.kind !== "layout") return;
        const world = unproject(event.nativeEvent.offsetX, event.nativeEvent.offsetY, canvas.current, document.renderBounds);
        current.preview = {
          ...current.original,
          position: {
            ...current.original.position,
            x: round2(current.original.position.x + world.x - current.startX),
            y: round2(current.original.position.y + world.y - current.startY)
          }
        };
        render((value) => value + 1);
      }}
      onMouseUp={() => {
        const current = drag.current;
        drag.current = null;
        if (!current || current.kind !== "layout") return;
        void runSnapshotCommand(() =>
          commands.applyLayoutGuiEdit({
            type: "updatePlacementTransform",
            id: current.id,
            transform: current.preview
          })
        );
      }}
    />
  );
}

function FixtureCanvas({
  document,
  selected,
  setSelected
}: {
  document: any;
  selected: string | null;
  setSelected: (id: string | null) => void;
}) {
  const canvas = useRef<HTMLCanvasElement | null>(null);
  const drag = useRef<DragState>(null);
  const [revision, render] = useState(0);
  const fixture = document.fixtures.find((candidate: any) => candidate.objectKey === document.selectedObjectKey) ?? document.fixtures[0];

  useEffect(() => {
    if (!fixture) return;
    drawSpatialCanvas(canvas.current, fixture.renderPlan.bounds, (ctx, project) => {
      for (const guide of fixture.renderPlan.guides) {
        if (guide.type !== "line") continue;
        const from = project(guide.from);
        const to = project(guide.to);
        ctx.strokeStyle = "#456a83";
        ctx.beginPath();
        ctx.moveTo(from.x, from.y);
        ctx.lineTo(to.x, to.y);
        ctx.stroke();
      }
      fixture.renderPlan.emitters.forEach((point: Point3, index: number) => {
        const preview = drag.current?.kind === "fixturePoint" && drag.current.pointIndex === index ? drag.current.preview : point;
        const projected = project(preview);
        ctx.fillStyle = selected === `point:${index}` ? "#6abf8a" : "#d6a35a";
        ctx.beginPath();
        ctx.arc(projected.x, projected.y, 6, 0, Math.PI * 2);
        ctx.fill();
      });
    });
  }, [fixture, selected, revision]);

  if (!fixture) return <BlockedGui reason="No fixture definition is available." diagnostics={[]} />;

  return (
    <canvas
      ref={canvas}
      className="gui-canvas"
      onMouseDown={(event) => {
        if (fixture.geometry.type !== "points") return;
        const world = unproject(event.nativeEvent.offsetX, event.nativeEvent.offsetY, canvas.current, fixture.renderPlan.bounds);
        const index = nearestPoint(fixture.geometry.points, world);
        if (index === null) {
          setSelected(null);
          return;
        }
        setSelected(`point:${index}`);
        drag.current = { kind: "fixturePoint", objectKey: fixture.objectKey, pointIndex: index, preview: fixture.geometry.points[index] };
      }}
      onMouseMove={(event) => {
        const current = drag.current;
        if (!current || current.kind !== "fixturePoint") return;
        const world = unproject(event.nativeEvent.offsetX, event.nativeEvent.offsetY, canvas.current, fixture.renderPlan.bounds);
        current.preview = { x: round2(world.x), y: round2(world.y), z: current.preview.z };
        render((value) => value + 1);
      }}
      onMouseUp={() => {
        const current = drag.current;
        drag.current = null;
        if (!current || current.kind !== "fixturePoint") return;
        void runSnapshotCommand(() =>
          commands.applyFixtureGuiEdit({
            type: "movePoint",
            objectKey: current.objectKey,
            pointIndex: current.pointIndex,
            point: current.preview
          })
        );
      }}
    />
  );
}

function GuiInspector({ gui, selected }: { gui: any; selected: string | null }) {
  if (gui.type === "sequence") {
    const id = selected?.startsWith("effect:") ? Number(selected.split(":")[1]) : null;
    const effect = gui.document.effects.find((candidate: any) => candidate.id === id);
    return (
      <aside className="gui-inspector">
        <h2>Sequence</h2>
        {effect ? (
          <>
            <label>Effect<input readOnly value={effect.script} /></label>
            <label>Start<input readOnly value={`${effect.startMs} ms`} /></label>
            <label>Duration<input readOnly value={`${effect.durationMs} ms`} /></label>
            <button onClick={() => void runSnapshotCommand(() => commands.applySequenceGuiEdit({ type: "deleteEffect", id: effect.id }))}>Delete</button>
          </>
        ) : (
          <p>Select an effect.</p>
        )}
      </aside>
    );
  }
  if (gui.type === "layout") {
    const id = selected?.startsWith("placement:") ? Number(selected.split(":")[1]) : null;
    const placement = gui.document.fixtures.find((candidate: any) => candidate.id === id);
    return (
      <aside className="gui-inspector">
        <h2>Layout</h2>
        {placement ? (
          <>
            <label>Placement<input readOnly value={placement.name} /></label>
            <label>X<input readOnly value={placement.transform.position.x} /></label>
            <label>Y<input readOnly value={placement.transform.position.y} /></label>
            <label>Fixture<input readOnly value={placement.resolvedFixture.name} /></label>
          </>
        ) : (
          <p>Select a placement.</p>
        )}
      </aside>
    );
  }
  const fixture = gui.document.fixtures.find((candidate: any) => candidate.objectKey === gui.document.selectedObjectKey) ?? gui.document.fixtures[0];
  return (
    <aside className="gui-inspector">
      <h2>Fixture</h2>
      {fixture ? (
        <>
          <label>Name<input readOnly value={fixture.name} /></label>
          <label>
            Bulb
            <input
              type="number"
              min={0.05}
              step={0.05}
              defaultValue={fixture.bulbSize}
              onBlur={(event) =>
                void runSnapshotCommand(() =>
                  commands.applyFixtureGuiEdit({
                    type: "updateBulbSize",
                    objectKey: fixture.objectKey,
                    bulbSize: Number(event.currentTarget.value)
                  })
                )
              }
            />
          </label>
          <label>Geometry<input readOnly value={fixture.geometrySummary} /></label>
          <p>{selected?.startsWith("point:") ? `Point ${Number(selected.split(":")[1]) + 1}` : "Select a point."}</p>
        </>
      ) : (
        <p>No fixture.</p>
      )}
    </aside>
  );
}

function hitSequence(document: any, x: number, y: number, left: number, pxPerMs: number, laneHeight: number) {
  for (const effect of document.effects) {
    const laneIndex = document.lanes.findIndex((lane: any) => targetsEqual(lane.target, effect.target));
    const clipX = left + effect.startMs * pxPerMs;
    const clipY = 35 + laneIndex * laneHeight;
    const width = Math.max(12, effect.durationMs * pxPerMs);
    if (x >= clipX && x <= clipX + width && y >= clipY && y <= clipY + 26) {
      const resize: "left" | "right" | "none" =
        x - clipX < 8 ? "left" : clipX + width - x < 8 ? "right" : "none";
      return {
        effect,
        laneIndex,
        resize
      };
    }
  }
  return null;
}

function targetsEqual(left: any, right: any) {
  return left?.kind === right?.kind && left?.name === right?.name;
}

function drawSpatialCanvas(
  canvas: HTMLCanvasElement | null,
  bounds: any,
  draw: (ctx: CanvasRenderingContext2D, project: (point: Point3) => { x: number; y: number }) => void
) {
  if (!canvas) return;
  const rect = canvas.getBoundingClientRect();
  const dpr = window.devicePixelRatio || 1;
  canvas.width = Math.max(1, Math.floor(rect.width * dpr));
  canvas.height = Math.max(1, Math.floor(rect.height * dpr));
  const ctx = canvas.getContext("2d");
  if (!ctx) return;
  ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
  ctx.clearRect(0, 0, rect.width, rect.height);
  ctx.fillStyle = "#17181b";
  ctx.fillRect(0, 0, rect.width, rect.height);
  ctx.font = "12px Inter, sans-serif";
  const project = (point: Point3) => projectPoint(point, rect.width, rect.height, bounds);
  drawGrid(ctx, rect.width, rect.height);
  draw(ctx, project);
}

function drawGrid(ctx: CanvasRenderingContext2D, width: number, height: number) {
  ctx.strokeStyle = "#2c3036";
  ctx.lineWidth = 1;
  for (let x = 0; x < width; x += 32) {
    ctx.beginPath();
    ctx.moveTo(x, 0);
    ctx.lineTo(x, height);
    ctx.stroke();
  }
  for (let y = 0; y < height; y += 32) {
    ctx.beginPath();
    ctx.moveTo(0, y);
    ctx.lineTo(width, y);
    ctx.stroke();
  }
}

function fitViewport(bounds: any) {
  return bounds;
}

function projectPoint(point: Point3, width: number, height: number, bounds: any) {
  const padding = 42;
  const spanX = Math.max(1, bounds.maxX - bounds.minX);
  const spanY = Math.max(1, bounds.maxY - bounds.minY);
  const scale = Math.min((width - padding * 2) / spanX, (height - padding * 2) / spanY);
  return {
    x: padding + (point.x - bounds.minX) * scale,
    y: height - padding - (point.y - bounds.minY) * scale
  };
}

function unproject(x: number, y: number, canvas: HTMLCanvasElement | null, bounds: any): Point3 {
  const rect = canvas?.getBoundingClientRect();
  const width = rect?.width ?? 1;
  const height = rect?.height ?? 1;
  const padding = 42;
  const spanX = Math.max(1, bounds.maxX - bounds.minX);
  const spanY = Math.max(1, bounds.maxY - bounds.minY);
  const scale = Math.min((width - padding * 2) / spanX, (height - padding * 2) / spanY);
  return {
    x: bounds.minX + (x - padding) / scale,
    y: bounds.minY + (height - padding - y) / scale,
    z: 0
  };
}

function nearestPlacement(document: any, point: Point3) {
  let best: any = null;
  let bestDistance = Infinity;
  for (const placement of document.fixtures) {
    const distance = Math.hypot(placement.transform.position.x - point.x, placement.transform.position.y - point.y);
    if (distance < bestDistance && distance < 1.2) {
      best = placement;
      bestDistance = distance;
    }
  }
  return best;
}

function nearestPoint(points: Point3[], point: Point3) {
  let best: number | null = null;
  let bestDistance = Infinity;
  points.forEach((candidate, index) => {
    const distance = Math.hypot(candidate.x - point.x, candidate.y - point.y);
    if (distance < bestDistance && distance < 0.8) {
      best = index;
      bestDistance = distance;
    }
  });
  return best;
}

function round2(value: number) {
  return Math.round(value * 100) / 100;
}

function clamp(value: number, min: number, max: number) {
  return Math.max(min, Math.min(max, value));
}
