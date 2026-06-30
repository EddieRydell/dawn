import { useEffect, useMemo, useRef, useState } from "react";

import { commands } from "../../../api";

import type { FixtureDocument } from "../../../types";

import { runGuiEditCommand } from "../../../store";

import { denormalizePoint, drawSpatialCanvas, nearestPoint, normalizeBounds, normalizePoint, round6, unproject, type GuiFocus, type Point3 } from "../shared";
import { BlockedGui } from "../BlockedGui";

type FixturePointDragState = { kind: "fixturePoint"; objectKey: string; pointIndex: number; draft: Point3 } | null;

export function FixtureCanvas({
  document,
  selected,
  setSelected
}: {
  document: FixtureDocument;
  selected: GuiFocus;
  setSelected: (id: GuiFocus) => void;
}) {
  const canvas = useRef<HTMLCanvasElement | null>(null);
  const drag = useRef<FixturePointDragState>(null);
  const [revision, render] = useState(0);
  const fixture = document.fixtures.find((candidate) => candidate.objectKey === document.selectedObjectKey) ?? document.fixtures[0];
  const renderBounds = useMemo(() => fixture === undefined ? null : normalizeBounds(fixture.renderPlan.bounds), [fixture]);

  useEffect(() => {
    if (fixture === undefined || renderBounds === null) return;
    drawSpatialCanvas(canvas.current, renderBounds, (ctx, project) => {
      for (const guide of fixture.renderPlan.guides) {
        if (guide.type !== "line") continue;
        const from = project(normalizePoint(guide.from));
        const to = project(normalizePoint(guide.to));
        ctx.strokeStyle = "#456a83";
        ctx.beginPath();
        ctx.moveTo(from.x, from.y);
        ctx.lineTo(to.x, to.y);
        ctx.stroke();
      }
      fixture.renderPlan.emitters.forEach((point, index) => {
        const normalizedPoint = normalizePoint(point);
        const projected = project(drag.current?.kind === "fixturePoint" && drag.current.pointIndex === index ? drag.current.draft : normalizedPoint);
        ctx.fillStyle = selected?.type === "point" && selected.index === index ? "#6abf8a" : "#d6a35a";
        ctx.beginPath();
        ctx.arc(projected.x, projected.y, 6, 0, Math.PI * 2);
        ctx.fill();
      });
    });
  }, [fixture, renderBounds, selected, revision]);

  if (fixture === undefined || renderBounds === null) return <BlockedGui reason="No fixture definition is available." diagnostics={[]} />;

  return (
    <canvas
      ref={canvas}
      className="gui-canvas"
      onMouseDown={(event) => {
        if (fixture.geometry.type !== "points") return;
        const world = unproject(event.nativeEvent.offsetX, event.nativeEvent.offsetY, canvas.current, renderBounds);
        const points = fixture.geometry.points.map(normalizePoint);
        const index = nearestPoint(points, world);
        if (index === null) {
          setSelected(null);
          return;
        }
        const point = points[index];
        if (point === undefined) return;
        setSelected({ type: "point", index });
        drag.current = { kind: "fixturePoint", objectKey: fixture.objectKey, pointIndex: index, draft: point };
      }}
      onMouseMove={(event) => {
        const current = drag.current;
        if (!current) return;
        const world = unproject(event.nativeEvent.offsetX, event.nativeEvent.offsetY, canvas.current, renderBounds);
        current.draft = { x: round6(world.x), y: round6(world.y), z: current.draft.z };
        render((value) => value + 1);
      }}
      onMouseUp={() => {
        const current = drag.current;
        drag.current = null;
        if (!current) return;
        void runGuiEditCommand(() =>
          commands.applyFixtureGuiEdit({
            type: "movePoint",
            objectKey: current.objectKey,
            pointIndex: current.pointIndex,
            point: denormalizePoint(current.draft)
          })
        );
      }}
    />
  );
}
