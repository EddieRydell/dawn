import { useEffect, useMemo, useRef, useState } from "react";

import { commands } from "../../../api";

import type { LayoutDocument } from "../../../types";

import { runSnapshotCommand } from "../../../store";

import { denormalizeTransform, drawSpatialCanvas, nearestPlacement, normalizeBounds, normalizePoint, normalizeTransform, round6, unproject, type GuiFocus, type Transform } from "../shared";

type LayoutDragState = { kind: "layout"; id: number; startX: number; startY: number; original: Transform; draft: Transform } | null;

export function LayoutCanvas({
  document,
  selected,
  setSelected
}: {
  document: LayoutDocument;
  selected: GuiFocus;
  setSelected: (id: GuiFocus) => void;
}) {
  const canvas = useRef<HTMLCanvasElement | null>(null);
  const drag = useRef<LayoutDragState>(null);
  const [revision, render] = useState(0);
  const viewport = useMemo(() => normalizeBounds(document.renderBounds), [document.renderBounds]);

  useEffect(() => {
    drawSpatialCanvas(canvas.current, viewport, (ctx, project) => {
      for (const fixture of document.fixtures) {
        const transform = drag.current?.kind === "layout" && drag.current.id === fixture.id ? drag.current.draft : normalizeTransform(fixture.transform);
        const center = project(transform.position);
        ctx.fillStyle = selected?.type === "placement" && selected.id === fixture.id ? "#6abf8a" : "#d6a35a";
        ctx.beginPath();
        ctx.arc(center.x, center.y, 7, 0, Math.PI * 2);
        ctx.fill();
        ctx.fillStyle = "#ebe7df";
        ctx.fillText(fixture.name, center.x + 10, center.y - 8);
        for (const emitter of fixture.resolvedFixture.renderPlan.emitters) {
          const point3 = normalizePoint(emitter);
          const point = project({
            x: transform.position.x + point3.x * transform.scale.x,
            y: transform.position.y + point3.y * transform.scale.y,
            z: transform.position.z + point3.z * transform.scale.z
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
        const world = unproject(event.nativeEvent.offsetX, event.nativeEvent.offsetY, canvas.current, viewport);
        const hit = nearestPlacement(document, world);
        if (hit === null) {
          setSelected(null);
          return;
        }
        setSelected({ type: "placement", id: hit.id });
        drag.current = {
          kind: "layout",
          id: hit.id,
          startX: world.x,
          startY: world.y,
          original: normalizeTransform(hit.transform),
          draft: normalizeTransform(hit.transform)
        };
      }}
      onMouseMove={(event) => {
        const current = drag.current;
        if (!current) return;
        const world = unproject(event.nativeEvent.offsetX, event.nativeEvent.offsetY, canvas.current, viewport);
        current.draft = {
          ...current.original,
          position: {
            ...current.original.position,
            x: round6(current.original.position.x + world.x - current.startX),
            y: round6(current.original.position.y + world.y - current.startY)
          }
        };
        render((value) => value + 1);
      }}
      onMouseUp={() => {
        const current = drag.current;
        drag.current = null;
        if (!current) return;
        void runSnapshotCommand(() =>
          commands.applyLayoutGuiEdit({
            type: "updatePlacementTransform",
            id: current.id,
            transform: denormalizeTransform(current.draft)
          })
        );
      }}
    />
  );
}
