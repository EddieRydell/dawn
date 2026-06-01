import { listen } from "@tauri-apps/api/event";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { commands } from "../api";
import type { AudioPlaybackStatus, GeometryRenderBoundsDto, PreviewSceneDto } from "../bindings";
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
  positionSeconds: number;
  durationSeconds: number;
  audioPlaybackStatus: AudioPlaybackStatus;
  status: string;
  timing: PreviewTiming;
};

type PreviewTiming = {
  backendSeconds: number;
  targetFps: number;
  activeFps: number;
  targetFrameMs: number;
  sleepPlannedMs: number;
  loopElapsedMs: number;
  loopIntervalMs: number;
  previewTransportLockMs: number;
  liveOutputLockMs: number;
  modelLockWaitMs: number;
  previewSnapshotMs: number;
  analysisCloneMs: number;
  audioPollMs: number;
  audioApplyMs: number;
  modelUpdateMs: number;
  renderMs: number;
  rendererBuildMs: number;
  frameEvaluateMs: number;
  frameFixtureCloneMs: number;
  frameEffectLoopMs: number;
  frameOutputMs: number;
  publishMs: number;
  eventEmitMs: number;
  liveOutputMs: number;
  eventIntervalMs: number;
  renderedActiveEffects: number;
  renderedSampledPixels: number;
  renderedFrame: boolean;
  publishedFrame: boolean;
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
  const previousFrameTelemetry = useRef<{ receivedAt: number; backendSeconds: number; currentTimeSeconds: number } | null>(null);
  const [scene, setScene] = useState<PreviewSceneDto | null>(null);
  const [state, setState] = useState<PreviewState | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [viewport, setViewport] = useState<Viewport>({ scale: 1, panX: 0, panY: 0 });
  const [metrics, setMetrics] = useState({
        fps: 0,
        backendSeconds: 0,
        currentTimeSeconds: 0,
        frontendIntervalMs: 0,
        backendIntervalMs: 0,
    frameStepMs: 0
  });
  const fpsSamples = useRef<number[]>([]);

  const pixelPositions = useMemo(() => {
    if (!scene) return [];
    return scene.fixtures.flatMap((fixture) =>
      fixture.pixels.map((pixel) => ({
        x: pixel.xMeters,
        y: pixel.yMeters,
        radius: fixture.bulbRadiusMeters,
        fixture: fixture.name
      }))
    );
  }, [scene]);

  const draw = useCallback(() => {
    drawHandle.current = 0;
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
        backendSeconds: frame?.backendSeconds ?? 0,
        currentTimeSeconds: frame?.currentTimeSeconds ?? 0,
        frontendIntervalMs: metrics.frontendIntervalMs,
        backendIntervalMs: metrics.backendIntervalMs,
        frameStepMs: metrics.frameStepMs
      });
    }
  }, [metrics.backendIntervalMs, metrics.frameStepMs, metrics.frontendIntervalMs, pixelPositions, scene, viewport]);

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
          const previous = previousFrameTelemetry.current;
          previousFrameTelemetry.current = {
            receivedAt: now,
            backendSeconds: message.backendSeconds,
            currentTimeSeconds: message.currentTimeSeconds
          };
          if (previous !== null) {
            setMetrics((current) => ({
              ...current,
              frontendIntervalMs: now - previous.receivedAt,
              backendIntervalMs: (message.backendSeconds - previous.backendSeconds) * 1000,
              frameStepMs: (message.currentTimeSeconds - previous.currentTimeSeconds) * 1000
            }));
          }
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
          {metrics.fps} fps | backend {formatNumber(metrics.backendSeconds)} s | target {state?.timing.activeFps ?? 0}/{state?.timing.targetFps ?? 0} fps
        </div>
        <div>
          frame step {formatNumber(metrics.frameStepMs)} ms | backend step {formatNumber(metrics.backendIntervalMs)} ms | receive step{" "}
          {formatNumber(metrics.frontendIntervalMs)} ms
        </div>
        <div>
          loop {formatNumber(state?.timing.loopElapsedMs ?? 0)} ms | interval {formatNumber(state?.timing.loopIntervalMs ?? 0)} ms | sleep{" "}
          {formatNumber(state?.timing.sleepPlannedMs ?? 0)} ms | target frame {formatNumber(state?.timing.targetFrameMs ?? 0)} ms
        </div>
        <div>
          model {formatNumber(state?.timing.modelUpdateMs ?? 0)} ms | audio {formatNumber(state?.timing.audioPollMs ?? 0)} ms | apply{" "}
          {formatNumber(state?.timing.audioApplyMs ?? 0)} ms | render{" "}
          {formatNumber(state?.timing.renderMs ?? 0)} ms | publish {formatNumber(state?.timing.publishMs ?? 0)} ms
        </div>
        <div>
          locks model {formatNumber(state?.timing.modelLockWaitMs ?? 0)} ms | transport{" "}
          {formatNumber(state?.timing.previewTransportLockMs ?? 0)} ms | live lock {formatNumber(state?.timing.liveOutputLockMs ?? 0)} ms
        </div>
        <div>
          clones analysis {formatNumber(state?.timing.analysisCloneMs ?? 0)} ms | snapshot{" "}
          {formatNumber(state?.timing.previewSnapshotMs ?? 0)} ms | event emit {formatNumber(state?.timing.eventEmitMs ?? 0)} ms
        </div>
        <div>
          frame build {formatNumber(state?.timing.rendererBuildMs ?? 0)} ms | eval {formatNumber(state?.timing.frameEvaluateMs ?? 0)} ms | clone{" "}
          {formatNumber(state?.timing.frameFixtureCloneMs ?? 0)} ms | effects {formatNumber(state?.timing.frameEffectLoopMs ?? 0)} ms | output{" "}
          {formatNumber(state?.timing.frameOutputMs ?? 0)} ms
        </div>
        <div>
          active effects {state?.timing.renderedActiveEffects ?? 0} | sampled pixels {state?.timing.renderedSampledPixels ?? 0} | live output{" "}
          {formatNumber(state?.timing.liveOutputMs ?? 0)} ms
        </div>
        <div>
          {formatSeconds(state?.positionSeconds ?? metrics.currentTimeSeconds)} | {state?.isPlaying === true ? "Playing" : "Stopped"} |{" "}
          {previewAudioStatusLabel(state?.audioPlaybackStatus) ?? state?.status ?? error ?? "Ready"}
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
  const minX = bounds.minXMeters;
  const minY = bounds.minYMeters;
  const maxX = bounds.maxXMeters;
  const maxY = bounds.maxYMeters;
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

function formatSeconds(value: number) {
  const totalSeconds = Math.max(0, Math.floor(value));
  const minutes = Math.floor(totalSeconds / 60);
  const seconds = totalSeconds % 60;
  return `${minutes}:${String(seconds).padStart(2, "0")}`;
}

function previewAudioStatusLabel(status: AudioPlaybackStatus | undefined) {
  switch (status) {
    case "loading":
      return "Loading audio...";
    case "loading_to_play":
      return "Loading audio - will play";
    case "ready":
      return "Audio ready";
    case "playing":
      return "Audio playing";
    case "missing":
      return "Audio missing";
    case "error":
      return "Audio error";
    case "ended":
      return "Audio ended";
    case "none":
    case undefined:
      return null;
  }
}
