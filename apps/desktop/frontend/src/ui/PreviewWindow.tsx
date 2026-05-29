import { listen } from "@tauri-apps/api/event";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { commands } from "../api";
import type { GeometryRenderBoundsDto, PreviewSceneDto } from "../bindings";
import {
  disposePreviewTransport,
  getPreviewTransportMode,
  initPreviewTransport,
  subscribePreviewFrames,
  type SharedPreviewFrame
} from "../previewTransport";

type PreviewState = {
  sourceLabel: string;
  isPlaying: boolean;
  positionMs: number;
  durationMs: number;
  status: string;
};

type Viewport = {
  scale: number;
  panX: number;
  panY: number;
};

export function PreviewWindow() {
  const canvas = useRef<HTMLCanvasElement | null>(null);
  const drag = useRef<{ x: number; y: number; panX: number; panY: number } | null>(null);
  const latestFrame = useRef<SharedPreviewFrame | null>(null);
  const drawHandle = useRef(0);
  const lastHudUpdate = useRef(0);
  const requestDrawRef = useRef<() => void>(() => {});
  const [scene, setScene] = useState<PreviewSceneDto | null>(null);
  const [state, setState] = useState<PreviewState | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [viewport, setViewport] = useState<Viewport>({ scale: 1, panX: 0, panY: 0 });
  const [metrics, setMetrics] = useState({ fps: 0, backendMs: 0, renderMs: 0, currentTimeMs: 0 });
  const fpsSamples = useRef<number[]>([]);

  const pixelPositions = useMemo(() => {
    if (!scene) return [];
    return scene.fixtures.flatMap((fixture) =>
      fixture.pixels.map((pixel) => ({
        x: pixel.x ?? 0,
        y: pixel.y ?? 0,
        radius: fixture.bulbRadius ?? 0.1,
        fixture: fixture.name
      }))
    );
  }, [scene]);

  const draw = useCallback(() => {
    drawHandle.current = 0;
    const target = canvas.current;
    if (!target) return;
    const started = performance.now();
    const rect = target.getBoundingClientRect();
    const dpr = window.devicePixelRatio || 1;
    target.width = Math.max(1, Math.floor(rect.width * dpr));
    target.height = Math.max(1, Math.floor(rect.height * dpr));
    const ctx = target.getContext("2d");
    if (!ctx) return;
    ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
    ctx.clearRect(0, 0, rect.width, rect.height);
    ctx.fillStyle = "#000000";
    ctx.fillRect(0, 0, rect.width, rect.height);

    if (scene) {
      const project = buildProjector(scene.bounds, rect.width, rect.height, viewport);
      const frame = latestFrame.current?.frame;
      for (let index = 0; index < pixelPositions.length; index += 1) {
        const pixel = pixelPositions[index];
        if (!pixel) continue;
        const colorOffset = index * 3;
        const red = frame?.[colorOffset] ?? 0;
        const green = frame?.[colorOffset + 1] ?? 0;
        const blue = frame?.[colorOffset + 2] ?? 0;
        if (red === 0 && green === 0 && blue === 0) continue;
        const point = project(pixel.x, pixel.y);
        const radius = Math.max(3, pixel.radius * project.scale * 0.45);
        ctx.fillStyle = `rgb(${red}, ${green}, ${blue})`;
        ctx.beginPath();
        ctx.arc(point.x, point.y, radius, 0, Math.PI * 2);
        ctx.fill();
      }
    }

    const frame = latestFrame.current;
    const now = performance.now();
    if (now - lastHudUpdate.current >= 250) {
      lastHudUpdate.current = now;
      setMetrics({
        fps: fpsSamples.current.length,
        backendMs: frame?.backendMs ?? 0,
        renderMs: now - started,
        currentTimeMs: frame?.currentTimeMs ?? 0
      });
    }
  }, [pixelPositions, scene, viewport]);

  const requestDraw = useCallback(() => {
    if (drawHandle.current !== 0) return;
    drawHandle.current = requestAnimationFrame(draw);
  }, [draw]);

  useEffect(() => {
    requestDrawRef.current = requestDraw;
  }, [requestDraw]);

  useEffect(() => {
    let disposeFrames: (() => void) | undefined;
    let disposeEvents: (() => void) | undefined;
    const lifecycle = { disposed: false };
    void (async () => {
      try {
        const mode = await getPreviewTransportMode();
        if (mode !== "webview2_shared") {
          setError("Preview shared buffers are only available on Windows.");
          return;
        }
        const loadedScene = await commands.getPreviewScene();
        if (lifecycle.disposed) return;
        setScene(loadedScene);
        await initPreviewTransport();
        disposeFrames = subscribePreviewFrames((message) => {
          latestFrame.current = message;
          const now = performance.now();
          fpsSamples.current = [...fpsSamples.current.filter((sample) => now - sample < 1000), now];
          requestDrawRef.current();
        });
        disposeEvents = await listen<PreviewState>("preview_state_changed", (event) => {
          setState(event.payload);
        });
      } catch (loadError) {
        setError(String(loadError));
      }
    })();
    return () => {
      lifecycle.disposed = true;
      disposeFrames?.();
      disposeEvents?.();
      if (drawHandle.current !== 0) {
        cancelAnimationFrame(drawHandle.current);
        drawHandle.current = 0;
      }
      void disposePreviewTransport();
    };
  }, []);

  useEffect(() => {
    requestDraw();
  }, [requestDraw]);

  return (
    <div className="preview-window">
      <canvas
        ref={canvas}
        className="preview-canvas"
        onMouseDown={(event) => {
          drag.current = {
            x: event.clientX,
            y: event.clientY,
            panX: viewport.panX,
            panY: viewport.panY
          };
        }}
        onMouseMove={(event) => {
          const current = drag.current;
          if (!current) return;
          setViewport((view) => ({
            ...view,
            panX: current.panX + event.clientX - current.x,
            panY: current.panY + event.clientY - current.y
          }));
        }}
        onMouseUp={() => {
          drag.current = null;
        }}
        onMouseLeave={() => {
          drag.current = null;
        }}
        onWheel={(event) => {
          event.preventDefault();
          setViewport((current) => ({
            ...current,
            scale: clamp(current.scale * Math.exp(-event.deltaY * 0.0015), 0.25, 8)
          }));
        }}
      />
      <div className="preview-hud">
        <div>{state?.sourceLabel ?? scene?.sourceLabel ?? "No preview source"}</div>
        <div>
          {metrics.fps} fps | backend {formatNumber(metrics.backendMs)} ms | render {formatNumber(metrics.renderMs)} ms
        </div>
        <div>
          {formatMs(state?.positionMs ?? metrics.currentTimeMs)} | {state?.isPlaying === true ? "Playing" : "Stopped"} |{" "}
          {state?.status ?? error ?? "Ready"}
        </div>
      </div>
      <button
        className="preview-reset"
        type="button"
        onClick={() => {
          setViewport({ scale: 1, panX: 0, panY: 0 });
        }}
      >
        Reset
      </button>
    </div>
  );
}

function buildProjector(bounds: GeometryRenderBoundsDto, width: number, height: number, viewport: Viewport) {
  const padding = 56;
  const minX = bounds.minX ?? 0;
  const minY = bounds.minY ?? 0;
  const maxX = bounds.maxX ?? 0;
  const maxY = bounds.maxY ?? 0;
  const spanX = Math.max(1, maxX - minX);
  const spanY = Math.max(1, maxY - minY);
  const baseScale = Math.min((width - padding * 2) / spanX, (height - padding * 2) / spanY);
  const scale = baseScale * viewport.scale;
  const centerX = width / 2 + viewport.panX;
  const centerY = height / 2 + viewport.panY;
  const midX = (minX + maxX) / 2;
  const midY = (minY + maxY) / 2;
  return Object.assign(
    (x: number, y: number) => ({
      x: centerX + (x - midX) * scale,
      y: centerY - (y - midY) * scale
    }),
    { scale }
  );
}

function clamp(value: number, min: number, max: number) {
  return Math.max(min, Math.min(max, value));
}

function formatNumber(value: number) {
  return value.toFixed(1);
}

function formatMs(ms: number) {
  const totalSeconds = Math.max(0, Math.round(ms / 1000));
  const minutes = Math.floor(totalSeconds / 60);
  const seconds = totalSeconds % 60;
  return `${minutes}:${String(seconds).padStart(2, "0")}`;
}
