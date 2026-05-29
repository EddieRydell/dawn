import { listen } from "@tauri-apps/api/event";
import { useEffect, useMemo, useRef, useState, type KeyboardEvent, type MouseEvent } from "react";
import { ChevronLeft, ChevronRight, Pause, Play, SkipBack, Square } from "lucide-react";
import { commands } from "../api";
import type {
  ActiveGuiDocumentDto,
  AppSnapshotDto,
  ColorCurvePointDto,
  FixtureDocumentDto,
  FloatCurvePointDto,
  GeometryRenderBoundsDto,
  GeometryRenderPointDto,
  LayoutDocumentDto,
  LayoutFixturePlacementDto,
  LayoutTargetDto,
  Point3Dto,
  SequenceDocumentDto,
  SequenceEffectDto,
  SequenceEffectParamDto,
  SequenceEffectParamValueDto,
  SequenceEffectPreviewDto,
  TransformDto
} from "../bindings";
import { runSnapshotCommand } from "../store";

type Point3 = { x: number; y: number; z: number };
type Transform = { position: Point3; rotation: Point3; scale: Point3 };
type EditedFloatCurvePoint = { time: number; value: number };
type EditedColorCurvePoint = { time: number; value: string };
type ReadyGuiDocumentDto = Exclude<ActiveGuiDocumentDto, { type: "blocked" }>;
type SequencePreview = { id: number; startMs: number; durationMs: number; laneIndex: number };
type DragState =
  | null
  | { kind: "sequence"; id: number; startX: number; originalStartMs: number; laneIndex: number; resize: "none" | "left" | "right" }
  | { kind: "sequenceScrub" }
  | { kind: "layout"; id: number; startX: number; startY: number; original: Transform; preview: Transform }
  | { kind: "fixturePoint"; objectKey: string; pointIndex: number; preview: Point3 };

export function GuiEditor({ snapshot }: { snapshot: AppSnapshotDto }) {
  const gui = snapshot.activeGuiDocument;

  if (!gui) {
    return <BlockedGui reason="GUI data is not available for this document." diagnostics={[]} />;
  }
  if (gui.type === "blocked") {
    return <BlockedGui reason={gui.reason} diagnostics={gui.diagnostics} />;
  }

  const editorKey = guiEditorKey(snapshot.activeFile, gui);
  return <GuiEditorInner key={editorKey} gui={gui} snapshot={snapshot} />;
}

function GuiEditorInner({ gui, snapshot }: { gui: ReadyGuiDocumentDto; snapshot: AppSnapshotDto }) {
  const [selected, setSelected] = useState<string | null>(null);

  return (
    <div className="gui-editor-shell">
      {gui.type === "sequence" && (
        <SequenceEditor
          key={`${gui.document.path}:${gui.document.objectKey}`}
          document={gui.document}
          preview={snapshot.preview}
          selected={selected}
          setSelected={setSelected}
        />
      )}
      {gui.type === "layout" && <LayoutCanvas document={gui.document} selected={selected} setSelected={setSelected} />}
      {gui.type === "fixture" && (
        <FixtureCanvas document={gui.document} selected={selected} setSelected={setSelected} />
      )}
      <GuiInspector gui={gui} selected={selected} />
    </div>
  );
}

function SequenceEditor({
  document,
  preview,
  selected,
  setSelected
}: {
  document: SequenceDocumentDto;
  preview: AppSnapshotDto["preview"];
  selected: string | null;
  setSelected: (id: string | null) => void;
}) {
  const livePreview = useSequencePreview(preview);
  const unsupported = document.durationMs <= 0;
  const handleKeyDown = (event: KeyboardEvent<HTMLDivElement>) => {
    if (unsupported || isEditableShortcutTarget(event.target)) return;
    if (event.key === " ") {
      event.preventDefault();
      void runSnapshotCommand(livePreview.isPlaying ? commands.previewPause : commands.previewPlay);
    } else if (event.key.toLowerCase() === "s") {
      event.preventDefault();
      void runSnapshotCommand(commands.previewStop);
    } else if (event.key === "Home") {
      event.preventDefault();
      void runSnapshotCommand(commands.previewRewindToZero);
    } else if (event.key === "ArrowLeft") {
      event.preventDefault();
      stepSequenceFrame(document, livePreview.positionMs, livePreview.durationMs, -1);
    } else if (event.key === "ArrowRight") {
      event.preventDefault();
      stepSequenceFrame(document, livePreview.positionMs, livePreview.durationMs, 1);
    }
  };
  return (
    <div className="sequence-editor" tabIndex={-1} onKeyDown={handleKeyDown}>
      <SequenceCanvas
        document={document}
        previewPositionMs={livePreview.positionMs}
        previewHomeMs={livePreview.homeMs}
        selected={selected}
        setSelected={setSelected}
      />
    </div>
  );
}

export function SequenceTransportControls({
  document,
  preview
}: {
  document: SequenceDocumentDto;
  preview: AppSnapshotDto["preview"];
}) {
  const livePreview = useSequencePreview(preview);
  const unsupported = document.durationMs <= 0;
  const stepFrame = (direction: -1 | 1) => {
    stepSequenceFrame(document, livePreview.positionMs, livePreview.durationMs, direction);
  };
  return (
    <div className="sequence-toolbar" aria-label="Sequence transport">
      <button
        type="button"
        title={livePreview.isPlaying ? "Pause" : "Play"}
        disabled={unsupported}
        onClick={() => void runSnapshotCommand(livePreview.isPlaying ? commands.previewPause : commands.previewPlay)}
      >
        {livePreview.isPlaying ? <Pause size={15} /> : <Play size={15} />}
      </button>
      <button type="button" title="Stop" disabled={unsupported} onClick={() => void runSnapshotCommand(commands.previewStop)}>
        <Square size={14} />
      </button>
      <button type="button" title="Rewind to zero" disabled={unsupported} onClick={() => void runSnapshotCommand(commands.previewRewindToZero)}>
        <SkipBack size={15} />
      </button>
      <button
        type="button"
        title="Step backward"
        disabled={unsupported}
        onClick={() => {
          stepFrame(-1);
        }}
      >
        <ChevronLeft size={16} />
      </button>
      <button
        type="button"
        title="Step forward"
        disabled={unsupported}
        onClick={() => {
          stepFrame(1);
        }}
      >
        <ChevronRight size={16} />
      </button>
      <span className="sequence-time-readout">
        {formatMs(livePreview.positionMs)} / {formatMs(livePreview.durationMs || document.durationMs)} | Home {formatMs(livePreview.homeMs)}
      </span>
    </div>
  );
}

function useSequencePreview(preview: AppSnapshotDto["preview"]) {
  const [eventPreview, setEventPreview] = useState<AppSnapshotDto["preview"] | null>(null);

  useEffect(() => {
    let dispose: (() => void) | undefined;
    let disposed = false;
    void (async () => {
      dispose = await listen<AppSnapshotDto["preview"]>("preview_state_changed", (event) => {
        if (!disposed) {
          setEventPreview(event.payload);
        }
      });
    })();
    return () => {
      disposed = true;
      dispose?.();
    };
  }, []);

  return eventPreview ?? preview;
}

function guiEditorKey(activeFile: string | null, gui: ReadyGuiDocumentDto) {
  switch (gui.type) {
    case "sequence":
    case "layout":
      return `${activeFile ?? ""}:${gui.type}:${gui.document.path}:${gui.document.objectKey}`;
    case "fixture":
      return `${activeFile ?? ""}:${gui.type}:${gui.document.path}:${gui.document.selectedObjectKey ?? ""}`;
  }
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
  previewPositionMs,
  previewHomeMs,
  selected,
  setSelected
}: {
  document: SequenceDocumentDto;
  previewPositionMs: number;
  previewHomeMs: number;
  selected: string | null;
  setSelected: (id: string | null) => void;
}) {
  const canvas = useRef<HTMLCanvasElement | null>(null);
  const drag = useRef<DragState>(null);
  const [preview, setPreview] = useState<SequencePreview | null>(null);
  const [canvasSize, setCanvasSize] = useState({ width: 0, height: 0 });
  const [viewport, setViewport] = useState({ pxPerMs: 0.08, laneHeight: 42, scrollXMs: 0, scrollY: 0 });
  const [previewImages, setPreviewImages] = useState<Map<number, SequencePreviewImage>>(() => new Map());
  const [previewRequestTick, setPreviewRequestTick] = useState(0);
  const previewImagesRef = useRef(previewImages);
  const inFlightPreviewSignatures = useRef<Set<string>>(new Set());
  const initializedViewportKey = useRef<string | null>(null);
  const left = 128;
  const top = 28;
  const effectPreviewSignatures = useMemo(() => sequencePreviewSignatures(document), [document]);
  const effectPreviewSignaturesRef = useRef(effectPreviewSignatures);

  useEffect(() => {
    previewImagesRef.current = previewImages;
  }, [previewImages]);

  useEffect(() => {
    effectPreviewSignaturesRef.current = effectPreviewSignatures;
  }, [effectPreviewSignatures]);

  useEffect(() => {
    const target = canvas.current;
    if (!target) return;
    const updateSize = () => {
      const rect = target.getBoundingClientRect();
      setCanvasSize({ width: rect.width, height: rect.height });
      const timelineWidth = Math.max(1, rect.width - left);
      const key = `${document.durationMs}:${document.lanes.length}`;
      if (rect.width > 0 && initializedViewportKey.current !== key) {
        initializedViewportKey.current = key;
        setViewport({
          pxPerMs: clamp(timelineWidth / Math.max(1, document.durationMs), 0.02, 0.6),
          laneHeight: 42,
          scrollXMs: 0,
          scrollY: 0
        });
      }
    };
    const frame = window.requestAnimationFrame(updateSize);
    const observer = new ResizeObserver(updateSize);
    observer.observe(target);
    return () => {
      window.cancelAnimationFrame(frame);
      observer.disconnect();
    };
  }, [document.durationMs, document.lanes.length, left]);

  const visibleClips = useMemo(
    () => buildSequenceClipLayout(document, preview, viewport, left, top),
    [document, left, preview, top, viewport]
  );

  useEffect(() => {
    const target = canvas.current;
    if (!target || canvasSize.width <= 0 || canvasSize.height <= 0) return;

    const timelineWidth = Math.max(1, canvasSize.width - left);
    const visibleEffectIds = Array.from(
      new Set(
        visibleClips
          .filter(
            (clip) =>
              clip.rect.x + clip.rect.width >= left &&
              clip.rect.x <= canvasSize.width &&
              clip.rect.y + clip.rect.height >= top &&
              clip.rect.y <= canvasSize.height
          )
          .map((clip) => clip.effect.id)
      )
    );
    if (timelineWidth <= 0 || visibleEffectIds.length === 0) return;

    const missingEffects = visibleEffectIds
      .map((id) => ({ id, signature: effectPreviewSignatures.get(id) }))
      .filter((effect): effect is { id: number; signature: string } => {
        if (effect.signature === undefined) return false;
        if (previewImagesRef.current.get(effect.id)?.signature === effect.signature) return false;
        return !inFlightPreviewSignatures.current.has(effect.signature);
      });
    if (missingEffects.length === 0) return;

    const missingEffectIds = missingEffects.map((effect) => effect.id);
    const requestedSignatures = new Map(missingEffects.map((effect) => [effect.id, effect.signature]));
    for (const signature of requestedSignatures.values()) {
      inFlightPreviewSignatures.current.add(signature);
    }

    let cancelled = false;
    void commands
      .getSequenceEffectPreviews(document.path, document.objectKey, missingEffectIds)
      .then((batch) => {
        if (cancelled) return;
        setPreviewImages((current) => {
          const next = new Map(current);
          const returnedIds = new Set(batch.previews.map((raster) => raster.effectId));
          for (const [requestedId, signature] of requestedSignatures) {
            if (effectPreviewSignaturesRef.current.get(requestedId) !== signature) continue;
            if (!returnedIds.has(requestedId) && next.get(requestedId)?.signature !== signature) {
              next.set(requestedId, { signature, status: "unavailable" });
            }
          }
          for (const raster of batch.previews) {
            const signature = requestedSignatures.get(raster.effectId);
            if (signature === undefined) continue;
            if (effectPreviewSignaturesRef.current.get(raster.effectId) !== signature) continue;
            next.set(raster.effectId, {
              signature,
              status: "ready",
              canvas: previewCanvasFromRaster(raster)
            });
          }
          return next;
        });
      })
      .catch(() => {
        if (cancelled) return;
        setPreviewImages((current) => {
          const next = new Map(current);
          for (const [id, signature] of requestedSignatures) {
            if (effectPreviewSignaturesRef.current.get(id) !== signature) continue;
            if (next.get(id)?.signature !== signature) {
              next.set(id, { signature, status: "unavailable" });
            }
          }
          return next;
        });
      })
      .finally(() => {
        for (const signature of requestedSignatures.values()) {
          inFlightPreviewSignatures.current.delete(signature);
        }
        setPreviewRequestTick((tick) => tick + 1);
      });

    return () => {
      cancelled = true;
    };
  }, [canvasSize.height, canvasSize.width, document.objectKey, document.path, effectPreviewSignatures, left, previewRequestTick, top, visibleClips]);

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
    ctx.fillStyle = "#111214";
    ctx.fillRect(0, 0, rect.width, rect.height);
    ctx.font = "12px Inter, sans-serif";

    const timelineWidth = Math.max(1, rect.width - left);
    const laneCount = document.lanes.length;
    const totalLaneHeight = laneCount * viewport.laneHeight;
    const maxScrollXMs = Math.max(0, document.durationMs - timelineWidth / viewport.pxPerMs);
    const maxScrollY = Math.max(0, totalLaneHeight - Math.max(1, rect.height - top));
    const scrollXMs = clamp(viewport.scrollXMs, 0, maxScrollXMs);
    const scrollY = clamp(viewport.scrollY, 0, maxScrollY);

    ctx.fillStyle = "#17181b";
    ctx.fillRect(0, 0, left, rect.height);
    ctx.fillStyle = "#111214";
    ctx.fillRect(left, top, timelineWidth, rect.height - top);

    ctx.save();
    ctx.beginPath();
    ctx.rect(0, top, rect.width, rect.height - top);
    ctx.clip();
      document.lanes.forEach((lane, index) => {
      const y = top + index * viewport.laneHeight - scrollY;
      if (y > rect.height || y + viewport.laneHeight < top) return;
      ctx.fillStyle = index % 2 === 0 ? "#111214" : "#15171a";
      ctx.fillRect(left, y, timelineWidth, viewport.laneHeight);
      ctx.strokeStyle = "#24272c";
      ctx.beginPath();
      ctx.moveTo(left, y + viewport.laneHeight + 0.5);
      ctx.lineTo(rect.width, y + viewport.laneHeight + 0.5);
      ctx.stroke();
      ctx.fillStyle = "#17181b";
      ctx.fillRect(0, y, left, viewport.laneHeight);
      ctx.fillStyle = "#c7c0b6";
      ctx.fillText(lane.label, 12, y + viewport.laneHeight / 2 + 4);
    });
    ctx.restore();

    ctx.strokeStyle = "#373b42";
    ctx.beginPath();
    ctx.moveTo(left, 0);
    ctx.lineTo(left, rect.height);
    ctx.stroke();

    ctx.fillStyle = "#17181b";
    ctx.fillRect(0, 0, rect.width, top);
    ctx.strokeStyle = "#2c3036";
    ctx.beginPath();
    ctx.moveTo(0, top + 0.5);
    ctx.lineTo(rect.width, top + 0.5);
    ctx.stroke();

    drawTimelineGrid(ctx, left, top, rect.width, rect.height, viewport.pxPerMs, scrollXMs, document.frameRate);

    ctx.save();
    ctx.beginPath();
    ctx.rect(left, top, timelineWidth, rect.height - top);
    ctx.clip();
    for (const clip of visibleClips) {
      if (clip.rect.x + clip.rect.width < left || clip.rect.x > rect.width || clip.rect.y + clip.rect.height < top || clip.rect.y > rect.height) {
        continue;
      }
      ctx.fillStyle = "#696b70";
      ctx.fillRect(clip.rect.x, clip.rect.y, clip.rect.width, clip.rect.height);
      const previewImage = validPreviewImage(previewImages.get(clip.effect.id), effectPreviewSignatures.get(clip.effect.id));
      if (previewImage?.status === "ready") {
        ctx.save();
        ctx.imageSmoothingEnabled = false;
        ctx.drawImage(
          previewImage.canvas,
          clip.rect.x + 1,
          clip.rect.y + 1,
          Math.max(0, clip.rect.width - 2),
          Math.max(0, clip.rect.height - 2)
        );
        ctx.restore();
      }
      ctx.strokeStyle = selected === `effect:${clip.effect.id}` ? "#f0f0f0" : "#8a8d93";
      ctx.lineWidth = selected === `effect:${clip.effect.id}` ? 2 : 1;
      ctx.strokeRect(clip.rect.x + 0.5, clip.rect.y + 0.5, Math.max(0, clip.rect.width - 1), Math.max(0, clip.rect.height - 1));
    }
    ctx.restore();

    const playheadX = left + (clamp(previewPositionMs, 0, document.durationMs) - scrollXMs) * viewport.pxPerMs;
    const homeX = left + (clamp(previewHomeMs, 0, document.durationMs) - scrollXMs) * viewport.pxPerMs;
    if (homeX >= left && homeX <= rect.width) {
      ctx.strokeStyle = "#6abf8a";
      ctx.lineWidth = 1;
      ctx.setLineDash([4, 4]);
      ctx.beginPath();
      ctx.moveTo(homeX + 0.5, top);
      ctx.lineTo(homeX + 0.5, rect.height);
      ctx.stroke();
      ctx.setLineDash([]);
      ctx.fillStyle = "#6abf8a";
      ctx.fillRect(homeX - 3, top, 7, 4);
    }
    if (playheadX >= left && playheadX <= rect.width) {
      ctx.strokeStyle = "#f0c46b";
      ctx.lineWidth = 1;
      ctx.beginPath();
      ctx.moveTo(playheadX + 0.5, top);
      ctx.lineTo(playheadX + 0.5, rect.height);
      ctx.stroke();
    }

    ctx.strokeStyle = "#d6a35a";
    ctx.lineWidth = 1;
    ctx.beginPath();
    ctx.moveTo(left + 0.5, top);
    ctx.lineTo(left, rect.height);
    ctx.stroke();
  }, [document, effectPreviewSignatures, left, top, viewport, visibleClips, selected, previewImages, previewPositionMs, previewHomeMs]);

  const seekFromCanvas = (event: MouseEvent<HTMLCanvasElement>) => {
    const x = event.nativeEvent.offsetX;
    if (x < left) return;
    const positionMs = clamp(Math.round((viewport.scrollXMs + (x - left) / viewport.pxPerMs) / 10) * 10, 0, document.durationMs);
    void runSnapshotCommand(() => commands.previewSeek(positionMs));
  };

  return (
    <canvas
      ref={canvas}
      className="gui-canvas"
      tabIndex={0}
      onMouseDown={(event) => {
        event.currentTarget.focus();
        const hit = hitSequence(visibleClips, event.nativeEvent.offsetX, event.nativeEvent.offsetY);
        if (!hit) {
          setSelected(null);
          if (event.nativeEvent.offsetX >= left) {
            drag.current = { kind: "sequenceScrub" };
            seekFromCanvas(event);
          }
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
        if (current?.kind === "sequenceScrub") {
          seekFromCanvas(event);
          return;
        }
        if (!current || current.kind !== "sequence") return;
        const effect = document.effects.find((candidate) => candidate.id === current.id);
        if (effect === undefined) return;
        const deltaMs = Math.round((event.nativeEvent.offsetX - current.startX) / viewport.pxPerMs / 50) * 50;
        const laneIndex = clamp(Math.floor((event.nativeEvent.offsetY - top + viewport.scrollY) / viewport.laneHeight), 0, document.lanes.length - 1);
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
      onWheel={(event) => {
        const rect = event.currentTarget.getBoundingClientRect();
        const offsetX = event.clientX - rect.left;
        const offsetY = event.clientY - rect.top;
        const timelineWidth = Math.max(1, rect.width - left);
        const visibleHeight = Math.max(1, rect.height - top);
        const laneCount = document.lanes.length;

        event.preventDefault();
        setViewport((current) => {
          const maxScrollXMs = Math.max(0, document.durationMs - timelineWidth / current.pxPerMs);
          const maxScrollY = Math.max(0, laneCount * current.laneHeight - visibleHeight);
          if (event.ctrlKey && event.shiftKey) {
            const anchorY = clamp(offsetY - top, 0, visibleHeight);
            const anchorContentY = current.scrollY + anchorY;
            const nextLaneHeight = clamp(current.laneHeight * Math.exp(-event.deltaY * 0.002), 24, 120);
            const laneRatio = anchorContentY / current.laneHeight;
            const nextScrollY = laneRatio * nextLaneHeight - anchorY;
            return {
              ...current,
              laneHeight: nextLaneHeight,
              scrollY: clamp(nextScrollY, 0, Math.max(0, laneCount * nextLaneHeight - visibleHeight))
            };
          }
          if (event.ctrlKey) {
            const anchorX = clamp(offsetX - left, 0, timelineWidth);
            const anchorTime = current.scrollXMs + anchorX / current.pxPerMs;
            const nextPxPerMs = clamp(current.pxPerMs * Math.exp(-event.deltaY * 0.002), 0.02, 12);
            const nextScrollXMs = anchorTime - anchorX / nextPxPerMs;
            return {
              ...current,
              pxPerMs: nextPxPerMs,
              scrollXMs: clamp(nextScrollXMs, 0, Math.max(0, document.durationMs - timelineWidth / nextPxPerMs))
            };
          }
          if (event.shiftKey) {
            return {
              ...current,
              scrollXMs: clamp(current.scrollXMs + event.deltaY / current.pxPerMs, 0, maxScrollXMs)
            };
          }
          return {
            ...current,
            scrollY: clamp(current.scrollY + event.deltaY, 0, maxScrollY)
          };
        });
      }}
    />
  );
}

function LayoutCanvas({
  document,
  selected,
  setSelected
}: {
  document: LayoutDocumentDto;
  selected: string | null;
  setSelected: (id: string | null) => void;
}) {
  const canvas = useRef<HTMLCanvasElement | null>(null);
  const drag = useRef<DragState>(null);
  const [revision, render] = useState(0);
  const viewport = useMemo(() => normalizeBounds(document.renderBounds), [document.renderBounds]);

  useEffect(() => {
    drawSpatialCanvas(canvas.current, viewport, (ctx, project) => {
      for (const fixture of document.fixtures) {
        const transform = drag.current?.kind === "layout" && drag.current.id === fixture.id ? drag.current.preview : normalizeTransform(fixture.transform);
        const center = project(transform.position);
        ctx.fillStyle = selected === `placement:${fixture.id}` ? "#6abf8a" : "#d6a35a";
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
        setSelected(`placement:${hit.id}`);
        drag.current = {
          kind: "layout",
          id: hit.id,
          startX: world.x,
          startY: world.y,
          original: normalizeTransform(hit.transform),
          preview: normalizeTransform(hit.transform)
        };
      }}
      onMouseMove={(event) => {
        const current = drag.current;
        if (!current || current.kind !== "layout") return;
        const world = unproject(event.nativeEvent.offsetX, event.nativeEvent.offsetY, canvas.current, viewport);
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
  document: FixtureDocumentDto;
  selected: string | null;
  setSelected: (id: string | null) => void;
}) {
  const canvas = useRef<HTMLCanvasElement | null>(null);
  const drag = useRef<DragState>(null);
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
        const projected = project(drag.current?.kind === "fixturePoint" && drag.current.pointIndex === index ? drag.current.preview : normalizedPoint);
        ctx.fillStyle = selected === `point:${index}` ? "#6abf8a" : "#d6a35a";
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
        setSelected(`point:${index}`);
        drag.current = { kind: "fixturePoint", objectKey: fixture.objectKey, pointIndex: index, preview: point };
      }}
      onMouseMove={(event) => {
        const current = drag.current;
        if (!current || current.kind !== "fixturePoint") return;
        const world = unproject(event.nativeEvent.offsetX, event.nativeEvent.offsetY, canvas.current, renderBounds);
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

function GuiInspector({ gui, selected }: { gui: ReadyGuiDocumentDto; selected: string | null }) {
  if (gui.type === "sequence") {
    const id = selected !== null && selected.startsWith("effect:") ? Number(selected.split(":")[1]) : null;
    const effect = gui.document.effects.find((candidate) => candidate.id === id);
    return (
      <aside className="gui-inspector">
        <h2>Sequence</h2>
        {effect !== undefined ? (
          <>
            <label>Effect<input readOnly value={effect.script} /></label>
            <label>Start<input readOnly value={`${effect.startMs} ms`} /></label>
            <label>Duration<input readOnly value={`${effect.durationMs} ms`} /></label>
            <label>Target<input readOnly value={effect.targetLabel} /></label>
            {effect.params.length > 0 && (
              <div className="effect-param-section">
                <h3>Parameters</h3>
                {effect.params.map((param) => (
                  <EffectParamInput key={`${effect.id}:${param.name}`} effectId={effect.id} param={param} />
                ))}
              </div>
            )}
            <button onClick={() => void runSnapshotCommand(() => commands.applySequenceGuiEdit({ type: "deleteEffect", id: effect.id }))}>Delete</button>
          </>
        ) : (
          <p>Select an effect.</p>
        )}
      </aside>
    );
  }
  if (gui.type === "layout") {
    const id = selected !== null && selected.startsWith("placement:") ? Number(selected.split(":")[1]) : null;
    const placement = gui.document.fixtures.find((candidate) => candidate.id === id);
    const transform = placement === undefined ? null : normalizeTransform(placement.transform);
    return (
      <aside className="gui-inspector">
        <h2>Layout</h2>
        {placement !== undefined && transform !== null ? (
          <>
            <label>Placement<input readOnly value={placement.name} /></label>
            <label>X<input readOnly value={transform.position.x} /></label>
            <label>Y<input readOnly value={transform.position.y} /></label>
            <label>Fixture<input readOnly value={placement.resolvedFixture.name} /></label>
          </>
        ) : (
          <p>Select a placement.</p>
        )}
      </aside>
    );
  }
  const fixture = gui.document.fixtures.find((candidate) => candidate.objectKey === gui.document.selectedObjectKey) ?? gui.document.fixtures[0];
  return (
    <aside className="gui-inspector">
      <h2>Fixture</h2>
      {fixture !== undefined ? (
        <>
          <label>Name<input readOnly value={fixture.name} /></label>
          <label>
            Bulb
            <input
              type="number"
              min={0.05}
              step={0.05}
              defaultValue={fixture.bulbSize ?? ""}
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
          <p>{selected !== null && selected.startsWith("point:") ? `Point ${Number(selected.split(":")[1]) + 1}` : "Select a point."}</p>
        </>
      ) : (
        <p>No fixture.</p>
      )}
    </aside>
  );
}

function EffectParamInput({ effectId, param }: { effectId: number; param: SequenceEffectParamDto }) {
  const commit = (value: SequenceEffectParamValueDto) =>
    runSnapshotCommand(() =>
      commands.applySequenceGuiEdit({
        type: "updateEffectParam",
        id: effectId,
        name: param.name,
        value
      })
    ).then(() => undefined);

  if (!param.editable) {
    return <label>{param.name}<input readOnly value="Unavailable" /></label>;
  }

  switch (param.value.type) {
    case "int":
      return <NumberParam key={`${param.name}:${param.value.value}`} param={param} value={param.value.value} step={1} commit={(value) => commit({ type: "int", value: Math.max(0, Math.round(value)) })} />;
    case "float":
      return <NumberParam key={`${param.name}:${param.value.value ?? 0}`} param={param} value={param.value.value ?? 0} step={0.05} commit={(value) => commit({ type: "float", value })} />;
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
      return <ColorParam key={`${param.name}:${param.value.value.toLowerCase()}`} name={param.name} value={param.value.value} commit={(value) => commit({ type: "color", value })} />;
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
      return <FloatCurveParamShell name={param.name} points={normalizeFloatCurvePoints(param.value.points)} commit={(points) => commit({ type: "floatCurve", points })} />;
    case "colorCurve":
      return <ColorCurveParamShell name={param.name} points={normalizeColorCurvePoints(param.value.points)} commit={(points) => commit({ type: "colorCurve", points })} />;
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

function ColorParam({ name, value, commit }: { name: string; value: string; commit: (value: string) => Promise<void> }) {
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
  const scheduleCommit = (candidate: string) => {
    window.clearTimeout(colorCommitTimer.current);
    colorCommitTimer.current = window.setTimeout(() => { commitDraft(candidate); }, 200);
  };
  const colorCommitTimer = useRef<number | undefined>(undefined);
  useEffect(() => () => { window.clearTimeout(colorCommitTimer.current); }, []);
  return (
    <label>
      {name}
      <div className="effect-param-color">
        <input
          type="color"
          value={isHexColor(draft) ? draft : committedValue}
          onChange={(event) => {
            const next = event.currentTarget.value;
            setDraft(next);
            scheduleCommit(next);
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

function FloatCurveParamShell(props: {
  name: string;
  points: EditedFloatCurvePoint[];
  commit: (points: EditedFloatCurvePoint[]) => Promise<void>;
}) {
  return <FloatCurveParam key={`${props.name}:${curvePointsSignature(props.points)}`} {...props} />;
}

function ColorCurveParamShell(props: {
  name: string;
  points: EditedColorCurvePoint[];
  commit: (points: EditedColorCurvePoint[]) => Promise<void>;
}) {
  return <ColorCurveParam key={`${props.name}:${curvePointsSignature(props.points)}`} {...props} />;
}

function FloatCurveParam({
  name,
  points,
  commit
}: {
  name: string;
  points: EditedFloatCurvePoint[];
  commit: (points: EditedFloatCurvePoint[]) => Promise<void>;
}) {
  const [drafts, setDrafts] = useState(points);
  const pointsSignature = curvePointsSignature(points);
  const lastCommittedSignature = useRef(pointsSignature);
  const update = (next: EditedFloatCurvePoint[]) => {
    if (next.length > 0 && next.every((point) => Number.isFinite(point.time) && Number.isFinite(point.value))) {
      const sorted = sortCurvePoints(next);
      const signature = curvePointsSignature(sorted);
      setDrafts(sorted);
      if (signature !== lastCommittedSignature.current) {
        lastCommittedSignature.current = signature;
        void commit(sorted);
      }
    }
  };
  const setDraftPoint = (index: number, point: EditedFloatCurvePoint) => {
    setDrafts((current) => replaceAt(current, index, point));
  };
  const commitDraftPoint = (index: number) => {
    const point = drafts[index];
    if (!point) return;
    update(replaceAt(drafts, index, { time: clamp(point.time, 0, 1), value: point.value }));
  };
  return (
    <div className="effect-param-group">
      <div className="effect-param-name">{name}</div>
      {drafts.map((point, index) => (
        <div key={index} className="curve-point-row">
          <input
            type="number"
            min={0}
            max={1}
            step={0.01}
            value={point.time}
            onChange={(event) => { setDraftPoint(index, { ...point, time: Number(event.currentTarget.value) }); }}
            onBlur={() => { commitDraftPoint(index); }}
            onKeyDown={(event) => {
              if (event.key === "Enter") {
                commitDraftPoint(index);
                event.currentTarget.blur();
              }
            }}
          />
          <input
            type="number"
            step={0.05}
            value={point.value}
            onChange={(event) => { setDraftPoint(index, { ...point, value: Number(event.currentTarget.value) }); }}
            onBlur={() => { commitDraftPoint(index); }}
            onKeyDown={(event) => {
              if (event.key === "Enter") {
                commitDraftPoint(index);
                event.currentTarget.blur();
              }
            }}
          />
          <button type="button" disabled={drafts.length <= 1} onClick={() => { update(drafts.filter((_, pointIndex) => pointIndex !== index)); }}>-</button>
        </div>
      ))}
      <button type="button" onClick={() => { update([...drafts, { time: 1, value: drafts[drafts.length - 1]?.value ?? 0 }]); }}>Add point</button>
    </div>
  );
}

function ColorCurveParam({
  name,
  points,
  commit
}: {
  name: string;
  points: EditedColorCurvePoint[];
  commit: (points: EditedColorCurvePoint[]) => Promise<void>;
}) {
  const [drafts, setDrafts] = useState(points);
  const colorCommitTimers = useRef<Map<number, number>>(new Map());
  const lastCommittedValues = useRef(points.map((point) => point.value.toLowerCase()));
  useEffect(
    () => () => {
      for (const timer of colorCommitTimers.current.values()) {
        window.clearTimeout(timer);
      }
    },
    []
  );
  const update = (next: EditedColorCurvePoint[]) => {
    if (next.length > 0 && next.every((point) => Number.isFinite(point.time) && isHexColor(point.value))) {
      const sorted = sortCurvePoints(next).map((point) => ({ ...point, value: point.value.toLowerCase() }));
      setDrafts(sorted);
      lastCommittedValues.current = sorted.map((point) => point.value);
      void commit(sorted);
    }
  };
  const setDraftPoint = (index: number, point: EditedColorCurvePoint) => {
    setDrafts((current) => replaceAt(current, index, point));
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
  const scheduleColorCommit = (index: number, candidate: string) => {
    const existing = colorCommitTimers.current.get(index);
    if (existing !== undefined) {
      window.clearTimeout(existing);
    }
    colorCommitTimers.current.set(index, window.setTimeout(() => { commitDraftValue(index, candidate); }, 200));
  };
  return (
    <div className="effect-param-group">
      <div className="effect-param-name">{name}</div>
      {drafts.map((point, index) => (
        <div key={index} className="curve-point-row color-curve-point-row">
          <input
            type="number"
            min={0}
            max={1}
            step={0.01}
            value={point.time}
            onChange={(event) => { setDraftPoint(index, { ...point, time: Number(event.currentTarget.value) }); }}
            onBlur={() => { update(replaceAt(drafts, index, { ...point, time: clamp(point.time, 0, 1) })); }}
            onKeyDown={(event) => {
              if (event.key === "Enter") {
                update(replaceAt(drafts, index, { ...point, time: clamp(point.time, 0, 1) }));
                event.currentTarget.blur();
              }
            }}
          />
          <input
            type="color"
            value={isHexColor(point.value) ? point.value : (points[index]?.value ?? "#ffffff")}
            onChange={(event) => {
              const next = event.currentTarget.value;
              setDraftPoint(index, { ...point, value: next });
              scheduleColorCommit(index, next);
            }}
            onBlur={() => { commitDraftValue(index); }}
          />
          <input
            value={point.value}
            onChange={(event) => { setDraftPoint(index, { ...point, value: event.currentTarget.value }); }}
            onBlur={() => { commitDraftValue(index); }}
            onKeyDown={(event) => {
              if (event.key === "Enter") {
                commitDraftValue(index);
                event.currentTarget.blur();
              }
            }}
          />
          <button type="button" disabled={drafts.length <= 1} onClick={() => { update(drafts.filter((_, pointIndex) => pointIndex !== index)); }}>-</button>
        </div>
      ))}
      <button type="button" onClick={() => { update([...drafts, { time: 1, value: drafts[drafts.length - 1]?.value ?? "#ffffff" }]); }}>Add point</button>
    </div>
  );
}

type SequenceViewport = {
  pxPerMs: number;
  laneHeight: number;
  scrollXMs: number;
  scrollY: number;
};

type SequenceClipLayout = {
  effect: SequenceEffectDto;
  laneIndex: number;
  rect: { x: number; y: number; width: number; height: number };
};

type SequenceClip = {
  effect: SequenceEffectDto;
  laneIndex: number;
};

type SequenceClipWithSlot = SequenceClip & { slot: number };

type SequenceHit = {
  effect: SequenceEffectDto;
  laneIndex: number;
  resize: "left" | "right" | "none";
};

type SequencePreviewImage = {
  signature: string;
} & ({ status: "ready"; canvas: HTMLCanvasElement } | { status: "unavailable" });

function buildSequenceClipLayout(
  document: SequenceDocumentDto,
  preview: SequencePreview | null,
  viewport: SequenceViewport,
  left: number,
  top: number
): SequenceClipLayout[] {
  const clips = document.effects.map((effect): SequenceClip => {
    const activePreview = preview?.id === effect.id ? preview : null;
    if (activePreview === null) {
      return {
        effect,
        laneIndex: Math.max(0, document.lanes.findIndex((lane) => targetsEqual(lane.target, effect.target)))
      };
    }
    const previewLane = document.lanes[activePreview.laneIndex];
    return {
      effect: {
        ...effect,
        startMs: activePreview.startMs,
        durationMs: activePreview.durationMs,
        target: previewLane?.target ?? effect.target,
        targetLabel: previewLane?.label ?? effect.targetLabel
      },
      laneIndex: activePreview.laneIndex
    };
  });

  const byLane = new Map<number, SequenceClip[]>();
  for (const clip of clips) {
    if (clip.laneIndex < 0) continue;
    const laneClips = byLane.get(clip.laneIndex) ?? [];
    laneClips.push(clip);
    byLane.set(clip.laneIndex, laneClips);
  }

  const layouts: SequenceClipLayout[] = [];
  for (const [laneIndex, laneClips] of byLane) {
    const groups = groupOverlappingClips(laneClips);
    for (const group of groups) {
      const assigned = assignOverlapSlots(group);
      const slotCount = Math.max(1, Math.max(...assigned.map((clip) => clip.slot)) + 1);
      const slotHeight = viewport.laneHeight / slotCount;
      for (const clip of assigned) {
        const startMs = clip.effect.startMs;
        const endMs = startMs + clip.effect.durationMs;
        const x = left + (startMs - viewport.scrollXMs) * viewport.pxPerMs;
        const width = Math.max(12, (endMs - startMs) * viewport.pxPerMs);
        layouts.push({
          effect: clip.effect,
          laneIndex,
          rect: {
            x,
            y: top + laneIndex * viewport.laneHeight - viewport.scrollY + clip.slot * slotHeight + 2,
            width,
            height: Math.max(8, slotHeight - 4)
          }
        });
      }
    }
  }
  return layouts;
}

function groupOverlappingClips(clips: SequenceClip[]) {
  const sorted = [...clips].sort(compareClipsByTime);
  const groups: SequenceClip[][] = [];
  let current: SequenceClip[] = [];
  let currentEnd = -Infinity;
  for (const clip of sorted) {
    const start = clip.effect.startMs;
    const end = clip.effect.startMs + clip.effect.durationMs;
    if (current.length === 0 || start < currentEnd) {
      current.push(clip);
      currentEnd = Math.max(currentEnd, end);
      continue;
    }
    groups.push(current);
    current = [clip];
    currentEnd = end;
  }
  if (current.length > 0) groups.push(current);
  return groups;
}

function assignOverlapSlots(group: SequenceClip[]): SequenceClipWithSlot[] {
  const sorted = [...group].sort(compareClipsByTime);
  const slotEnds: number[] = [];
  return sorted.map((clip) => {
    const start = clip.effect.startMs;
    const end = clip.effect.startMs + clip.effect.durationMs;
    let slot = slotEnds.findIndex((slotEnd) => slotEnd <= start);
    if (slot === -1) slot = slotEnds.length;
    slotEnds[slot] = end;
    return { ...clip, slot };
  });
}

function compareClipsByTime(left: { effect: SequenceEffectDto }, right: { effect: SequenceEffectDto }) {
  return (
    left.effect.startMs - right.effect.startMs ||
    left.effect.startMs + left.effect.durationMs - (right.effect.startMs + right.effect.durationMs) ||
    left.effect.id - right.effect.id
  );
}

function hitSequence(clips: SequenceClipLayout[], x: number, y: number): SequenceHit | null {
  for (const clip of [...clips].reverse()) {
    const { rect } = clip;
    if (x >= rect.x && x <= rect.x + rect.width && y >= rect.y && y <= rect.y + rect.height) {
      const resize: "left" | "right" | "none" =
        x - rect.x < 8 ? "left" : rect.x + rect.width - x < 8 ? "right" : "none";
      return {
        effect: clip.effect,
        laneIndex: clip.laneIndex,
        resize
      };
    }
  }
  return null;
}

function drawTimelineGrid(
  ctx: CanvasRenderingContext2D,
  left: number,
  top: number,
  width: number,
  height: number,
  pxPerMs: number,
  scrollXMs: number,
  frameRate: number
) {
  const tick = chooseTimelineTick(pxPerMs, frameRate);
  const firstMinor = Math.floor(scrollXMs / tick.minorMs) * tick.minorMs;
  ctx.lineWidth = 1;
  for (let time = firstMinor; ; time += tick.minorMs) {
    const x = left + (time - scrollXMs) * pxPerMs;
    if (x > width) break;
    if (x < left) continue;
    const labeled = isMultipleOf(time, tick.labelMs);
    ctx.strokeStyle = labeled ? "#343941" : "#1f2227";
    ctx.beginPath();
    ctx.moveTo(x + 0.5, labeled ? 0 : top);
    ctx.lineTo(x + 0.5, height);
    ctx.stroke();
    if (labeled) {
      ctx.fillStyle = "#a8a29a";
      ctx.fillText(formatTimelineMs(time, tick.labelMs), x + 5, 18);
    }
  }
}

function chooseTimelineTick(pxPerMs: number, frameRate: number) {
  const frameMs = Math.max(1, Math.round(1000 / Math.max(1, frameRate)));
  const minorCandidates = Array.from(new Set([
    frameMs,
    frameMs * 2,
    frameMs * 5,
    frameMs * 10,
    50,
    100,
    250,
    500,
    1000,
    2500,
    5000,
    10000,
    30000,
    60000
  ])).sort((left, right) => left - right);
  const minorMs = minorCandidates.find((candidate) => candidate * pxPerMs >= 24) ?? 60000;
  const labelMs = minorCandidates.find((candidate) => candidate >= minorMs && candidate * pxPerMs >= 110) ?? minorMs * 5;
  return { minorMs, labelMs };
}

function isMultipleOf(value: number, interval: number) {
  return Math.abs(value / interval - Math.round(value / interval)) < 0.0001;
}

function formatMs(ms: number) {
  const totalSeconds = Math.max(0, Math.round(ms / 1000));
  const minutes = Math.floor(totalSeconds / 60);
  const seconds = totalSeconds % 60;
  return `${minutes}:${String(seconds).padStart(2, "0")}`;
}

function formatTimelineMs(ms: number, intervalMs: number) {
  if (intervalMs < 1000) {
    const totalMs = Math.max(0, Math.round(ms));
    const minutes = Math.floor(totalMs / 60000);
    const seconds = Math.floor((totalMs % 60000) / 1000);
    const millis = totalMs % 1000;
    return `${minutes}:${String(seconds).padStart(2, "0")}.${String(millis).padStart(3, "0")}`;
  }
  return formatMs(ms);
}

function isEditableShortcutTarget(target: EventTarget | null) {
  if (!(target instanceof HTMLElement)) return false;
  if (target.isContentEditable) return true;
  if (target.closest(".cm-editor")) return true;
  return target.closest("input, textarea, select") !== null;
}

function stepSequenceFrame(document: SequenceDocumentDto, positionMs: number, previewDurationMs: number, direction: -1 | 1) {
  const frameMs = Math.max(1, Math.round(1000 / Math.max(1, document.frameRate)));
  const nextPositionMs = clamp(positionMs + direction * frameMs, 0, previewDurationMs || document.durationMs);
  void runSnapshotCommand(() => commands.previewSeek(nextPositionMs));
}

function targetsEqual(left: LayoutTargetDto, right: LayoutTargetDto) {
  return left.kind === right.kind && left.name === right.name;
}

function sequencePreviewSignatures(document: SequenceDocumentDto) {
  return new Map<number, string>(
    document.effects.map((effect) => [
      effect.id,
      JSON.stringify({
        path: document.path,
        objectKey: document.objectKey,
        frameRate: document.frameRate,
        id: effect.id,
        durationMs: effect.durationMs,
        target: effect.target,
        script: effect.script,
        params: effect.params
      })
    ])
  );
}

function replaceAt<T>(items: T[], index: number, value: T) {
  return items.map((item, itemIndex) => (itemIndex === index ? value : item));
}

function sortCurvePoints<T extends { time: number }>(points: T[]) {
  return [...points].sort((left, right) => left.time - right.time);
}

function curvePointsSignature(points: Array<{ time: number; value: number | string }>) {
  return JSON.stringify(points);
}

function normalizeFloatCurvePoints(points: FloatCurvePointDto[]): EditedFloatCurvePoint[] {
  const normalized = points
    .filter((point): point is { time: number; value: number } => point.time !== null && point.value !== null)
    .filter((point) => Number.isFinite(point.time) && Number.isFinite(point.value))
    .map((point) => ({ time: clamp(point.time, 0, 1), value: point.value }));
  return normalized.length > 0 ? normalized : [{ time: 0, value: 0 }];
}

function normalizeColorCurvePoints(points: ColorCurvePointDto[]): EditedColorCurvePoint[] {
  const normalized = points
    .filter((point): point is { time: number; value: string } => point.time !== null && isHexColor(point.value))
    .filter((point) => Number.isFinite(point.time))
    .map((point) => ({ time: clamp(point.time, 0, 1), value: point.value.toLowerCase() }));
  return normalized.length > 0 ? normalized : [{ time: 0, value: "#ffffff" }];
}

function isHexColor(value: string | null | undefined): value is string {
  return /^#[0-9a-fA-F]{6}$/.test(value ?? "");
}

function previewCanvasFromRaster(raster: SequenceEffectPreviewDto) {
  const canvas = document.createElement("canvas");
  canvas.width = Math.max(1, raster.columns);
  canvas.height = Math.max(1, raster.rows);
  const ctx = canvas.getContext("2d");
  if (!ctx) return canvas;
  const image = ctx.createImageData(canvas.width, canvas.height);
  for (let row = 0; row < raster.rows; row += 1) {
    for (let column = 0; column < raster.columns; column += 1) {
      const sourceIndex = row * raster.columns + column;
      const color = raster.colors[sourceIndex] ?? 0;
      const targetIndex = sourceIndex * 4;
      image.data[targetIndex] = (color >> 16) & 0xff;
      image.data[targetIndex + 1] = (color >> 8) & 0xff;
      image.data[targetIndex + 2] = color & 0xff;
      image.data[targetIndex + 3] = 0xff;
    }
  }
  ctx.putImageData(image, 0, 0);
  return canvas;
}

function validPreviewImage(image: SequencePreviewImage | undefined, signature: string | undefined) {
  if (image === undefined || signature === undefined) return undefined;
  return image.signature === signature ? image : undefined;
}

function normalizePoint(point: Point3Dto | GeometryRenderPointDto): Point3 {
  return {
    x: point.x ?? 0,
    y: point.y ?? 0,
    z: point.z ?? 0
  };
}

function normalizeTransform(transform: TransformDto): Transform {
  return {
    position: normalizePoint(transform.position),
    rotation: normalizePoint(transform.rotation),
    scale: {
      x: transform.scale.x ?? 1,
      y: transform.scale.y ?? 1,
      z: transform.scale.z ?? 1
    }
  };
}

type RenderBounds = {
  minX: number;
  minY: number;
  maxX: number;
  maxY: number;
};

function normalizeBounds(bounds: GeometryRenderBoundsDto): RenderBounds {
  return {
    minX: bounds.minX ?? 0,
    minY: bounds.minY ?? 0,
    maxX: bounds.maxX ?? 0,
    maxY: bounds.maxY ?? 0
  };
}

function drawSpatialCanvas(
  canvas: HTMLCanvasElement | null,
  bounds: RenderBounds,
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

function projectPoint(point: Point3, width: number, height: number, bounds: RenderBounds) {
  const padding = 42;
  const spanX = Math.max(1, bounds.maxX - bounds.minX);
  const spanY = Math.max(1, bounds.maxY - bounds.minY);
  const scale = Math.min((width - padding * 2) / spanX, (height - padding * 2) / spanY);
  return {
    x: padding + (point.x - bounds.minX) * scale,
    y: height - padding - (point.y - bounds.minY) * scale
  };
}

function unproject(x: number, y: number, canvas: HTMLCanvasElement | null, bounds: RenderBounds): Point3 {
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

function nearestPlacement(document: LayoutDocumentDto, point: Point3): LayoutFixturePlacementDto | null {
  let best: LayoutFixturePlacementDto | null = null;
  let bestDistance = Infinity;
  for (const placement of document.fixtures) {
    const transform = normalizeTransform(placement.transform);
    const distance = Math.hypot(transform.position.x - point.x, transform.position.y - point.y);
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
  for (let index = 0; index < points.length; index += 1) {
    const candidate = points[index];
    if (candidate === undefined) continue;
    const distance = Math.hypot(candidate.x - point.x, candidate.y - point.y);
    if (distance < bestDistance && distance < 0.8) {
      best = index;
      bestDistance = distance;
    }
  }
  return best;
}

function round2(value: number) {
  return Math.round(value * 100) / 100;
}

function clamp(value: number, min: number, max: number) {
  return Math.max(min, Math.min(max, value));
}
