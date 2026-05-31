import { listen } from "@tauri-apps/api/event";
import { convertFileSrc } from "@tauri-apps/api/core";
import * as ContextMenu from "@radix-ui/react-context-menu";
import { useCallback, useEffect, useMemo, useRef, useState, type KeyboardEvent, type MouseEvent, type PointerEvent, type ReactNode } from "react";
import { ChevronLeft, ChevronRight, LoaderCircle, Minus, Music, Pause, Play, RadioTower, SkipBack, Square, Trash2, X } from "lucide-react";
import { commands } from "../api";
import type {
  ActiveGuiDocumentDto,
  AppSnapshotDto,
  AudioPlaybackStatus,
  ColorCurvePointDto,
  FixtureDocumentDto,
  FloatCurvePointDto,
  GeometryRenderBoundsDto,
  GeometryRenderPointDto,
  LayoutDocumentDto,
  LayoutFixturePlacementDto,
  LayoutTargetDto,
  Point3MetersDto,
  SequenceAudioDto,
  SequenceDocumentDto,
  SequenceEffectDto,
  SequenceEffectScopeDto,
  SequenceMarkCollectionDto,
  SequenceEffectParamDto,
  SequenceEffectParamValueDto,
  SequenceEffectScriptDto,
  SequenceEffectPreviewDto,
  SequenceMarkRefDto,
  SequenceSelectionDto,
  TransformDto
} from "../bindings";
import { runSnapshotCommand } from "../store";

type Point3 = { x: number; y: number; z: number };
type Transform = { position: Point3; rotation: Point3; scale: Point3 };
type PreviewTiming = {
  backendSeconds: number;
  targetFps: number;
  activeFps: number;
  targetFrameMs: number;
  sleepPlannedMs: number;
  loopIntervalMs: number;
  audioPositionSeconds: number | null;
  snapshotPositionSeconds: number;
  framePositionSeconds: number;
  snapshotMinusAudioMs: number | null;
  frameMinusAudioMs: number | null;
  loopElapsedMs: number;
  audioPollMs: number;
  audioApplyMs: number;
  modelUpdateMs: number;
  renderMs: number;
  publishMs: number;
  eventIntervalMs: number;
  hasSink: boolean;
  publishedFrame: boolean;
  renderedFrame: boolean;
};
type PreviewStateEvent = AppSnapshotDto["preview"] & { timing: PreviewTiming };
type LivePreview = AppSnapshotDto["preview"] & { timing?: PreviewTiming };
type EditedFloatCurvePoint = { time: number; value: number };
type EditedColorCurvePoint = { time: number; value: string };
type ReadyGuiDocumentDto = Exclude<ActiveGuiDocumentDto, { type: "blocked" }>;
type SequencePreview = { id: number; startSeconds: number; durationSeconds: number; laneIndex: number };
type MarkPreview = { collectionKey: string; index: number; timeSeconds: number; committedIndex?: number };
type MarkPreviewLookup = Map<string, MarkPreview>;
type SequenceContextMenu =
  | { kind: "blank"; laneIndex: number; startSeconds: number }
  | { kind: "effect"; laneIndex: number; startSeconds: number; effectId: number }
  | { kind: "mark"; laneIndex: number; startSeconds: number; collectionKey: string; index: number };
type SequenceHover =
  | null
  | { kind: "effect"; effectId: number; resize: "left" | "right" | "none" }
  | { kind: "mark"; collectionKey: string; index: number };
type SequenceSelection = SequenceSelectionDto | null;
type SequenceMarquee = { mode: "effects" | "marks"; startX: number; startY: number; x: number; y: number; active: boolean; shift: boolean; ctrl: boolean };
type MarkDisplayMode = "overlay" | "strip" | "hidden";
type DragState =
  | null
  | { kind: "sequence"; id: number; startX: number; originalStartSeconds: number; laneIndex: number; resize: "none" | "left" | "right" }
  | { kind: "mark"; collectionKey: string; index: number; startX: number; originalTimeSeconds: number }
  | { kind: "marquee"; state: SequenceMarquee }
  | { kind: "sequenceScrub" }
  | { kind: "layout"; id: number; startX: number; startY: number; original: Transform; preview: Transform }
  | { kind: "fixturePoint"; objectKey: string; pointIndex: number; preview: Point3 };

const DEFAULT_MARK_COLORS = ["#38bdf8", "#f97316", "#22c55e", "#e879f9", "#facc15", "#ef4444"];
const MIN_EFFECT_DURATION_SECONDS = 0.000000001;
let markDisplayMode: MarkDisplayMode = "overlay";

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
  const [sequenceSelection, setSequenceSelection] = useState<SequenceSelection>(null);
  const [activeMarkCollectionKey, setActiveMarkCollectionKey] = useState<string | null>(() =>
    gui.type === "sequence" ? gui.document.markCollections[0]?.key ?? null : null
  );
  const [visibleMarkCollectionKeys, setVisibleMarkCollectionKeys] = useState<Set<string>>(() =>
    new Set(gui.type === "sequence" ? gui.document.markCollections.map((collection) => collection.key) : [])
  );
  const livePreview = useSequencePreview(snapshot.preview);

  return (
    <div
      className="gui-editor-shell"
      onKeyDownCapture={(event) => {
        if (gui.type === "sequence" && !markSelectionConsumesKey(selected, event.key)) {
          handleSequencePlaybackShortcut(event, gui.document, livePreview, gui.document.durationSeconds <= 0);
        }
      }}
    >
      {gui.type === "sequence" && (
        <SequenceEditor
          key={`${gui.document.path}:${gui.document.objectKey}`}
          document={gui.document}
          preview={livePreview}
          selected={selected}
          setSelected={setSelected}
          sequenceSelection={sequenceSelection}
          setSequenceSelection={setSequenceSelection}
          activeMarkCollectionKey={activeMarkCollectionKey}
          setActiveMarkCollectionKey={setActiveMarkCollectionKey}
          visibleMarkCollectionKeys={visibleMarkCollectionKeys}
          setVisibleMarkCollectionKeys={setVisibleMarkCollectionKeys}
        />
      )}
      {gui.type === "layout" && <LayoutCanvas document={gui.document} selected={selected} setSelected={setSelected} />}
      {gui.type === "fixture" && (
        <FixtureCanvas document={gui.document} selected={selected} setSelected={setSelected} />
      )}
      <GuiInspector
        gui={gui}
        selected={selected}
        setSelected={setSelected}
        sequenceSelection={sequenceSelection}
        activeMarkCollectionKey={activeMarkCollectionKey}
        setActiveMarkCollectionKey={setActiveMarkCollectionKey}
        visibleMarkCollectionKeys={visibleMarkCollectionKeys}
        setVisibleMarkCollectionKeys={setVisibleMarkCollectionKeys}
      />
    </div>
  );
}

function SequenceEditor({
  document,
  preview,
  selected,
  setSelected,
  sequenceSelection,
  setSequenceSelection,
  activeMarkCollectionKey,
  setActiveMarkCollectionKey,
  visibleMarkCollectionKeys,
  setVisibleMarkCollectionKeys
}: {
  document: SequenceDocumentDto;
  preview: LivePreview;
  selected: string | null;
  setSelected: (id: string | null) => void;
  sequenceSelection: SequenceSelection;
  setSequenceSelection: (selection: SequenceSelection) => void;
  activeMarkCollectionKey: string | null;
  setActiveMarkCollectionKey: (key: string | null) => void;
  visibleMarkCollectionKeys: Set<string>;
  setVisibleMarkCollectionKeys: (keys: Set<string>) => void;
}) {
  const livePreview = preview;
  const unsupported = document.durationSeconds <= 0;
  const handleKeyDown = (event: KeyboardEvent<HTMLDivElement>) => {
    handleSequencePlaybackShortcut(event, document, livePreview, unsupported);
  };
  return (
    <div className="sequence-editor" tabIndex={-1} onKeyDown={handleKeyDown}>
      <SequenceCanvas
        document={document}
        previewPositionSeconds={livePreview.positionSeconds}
        previewHomeSeconds={livePreview.homeSeconds}
        selected={selected}
        setSelected={setSelected}
        sequenceSelection={sequenceSelection}
        setSequenceSelection={setSequenceSelection}
        activeMarkCollectionKey={activeMarkCollectionKey}
        setActiveMarkCollectionKey={setActiveMarkCollectionKey}
        visibleMarkCollectionKeys={visibleMarkCollectionKeys}
        setVisibleMarkCollectionKeys={setVisibleMarkCollectionKeys}
      />
    </div>
  );
}

export function SequenceTransportControls({
  document,
  preview,
  liveOutput
}: {
  document: SequenceDocumentDto;
  preview: AppSnapshotDto["preview"];
  liveOutput: AppSnapshotDto["liveOutput"];
}) {
  const livePreview = useSequencePreview(preview);
  const unsupported = document.durationSeconds <= 0;
  const audioStatus = useSequenceAudioStatus(livePreview);
  const timingSummary = previewTimingSummary(livePreview.timing);
  const audioLoading = isAudioLoadingStatus(livePreview.audioPlaybackStatus);
  const audioQueued = livePreview.audioPlaybackStatus === "loading_to_play";
  const playCommand = audioQueued || livePreview.isPlaying ? commands.previewPause : commands.previewPlay;
  const [mode, setMode] = useMarkDisplayMode();
  const stepFrame = (direction: -1 | 1) => {
    stepSequenceFrame(document, livePreview.positionSeconds, livePreview.durationSeconds, direction);
  };
  const setMarkMode = (nextMode: MarkDisplayMode) => {
    markDisplayMode = nextMode;
    window.dispatchEvent(new CustomEvent<MarkDisplayMode>("dawn-mark-display-mode", { detail: nextMode }));
    setMode(nextMode);
  };
  return (
    <div
      className="sequence-toolbar"
      aria-label="Sequence transport"
      onKeyDownCapture={(event) => {
        handleSequencePlaybackShortcut(event, document, livePreview, unsupported);
      }}
    >
      <button
        type="button"
        title={audioQueued ? "Cancel queued playback" : audioLoading ? "Play when audio loads" : livePreview.isPlaying ? "Pause" : "Play"}
        disabled={unsupported}
        onClick={() => void runSnapshotCommand(playCommand)}
      >
        {audioLoading ? <LoaderCircle className="sequence-loading-icon" size={15} /> : livePreview.isPlaying ? <Pause size={15} /> : <Play size={15} />}
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
      <button
        type="button"
        className={liveOutput.enabled ? "active" : ""}
        title={liveOutput.lastError ?? `Live output: ${liveOutput.status}`}
        onClick={() => void runSnapshotCommand(() => commands.setLiveOutputEnabled(!liveOutput.enabled))}
      >
        <RadioTower size={15} />
      </button>
      <button
        type="button"
        title="Choose audio"
        onClick={() => void runSnapshotCommand(commands.chooseSequenceAudio)}
      >
        <Music size={15} />
      </button>
      <button
        type="button"
        title="Clear audio"
        disabled={document.audio === null}
        onClick={() => void runSnapshotCommand(commands.clearSequenceAudio)}
      >
        <X size={15} />
      </button>
      <select
        className="mark-display-select"
        title="Mark display"
        value={mode}
        onChange={(event) => {
          setMarkMode(event.currentTarget.value as MarkDisplayMode);
        }}
      >
        <option value="overlay">Marks</option>
        <option value="strip">Strip</option>
        <option value="hidden">Hidden</option>
      </select>
      <span className="sequence-time-readout">
        {formatSeconds(livePreview.positionSeconds)} / {formatSeconds(livePreview.durationSeconds || document.durationSeconds)} | Home {formatSeconds(livePreview.homeSeconds)}
        {document.audio ? ` | ${document.audio.exists ? document.audio.fileName : "Missing audio"}` : ""}
        {liveOutput.enabled ? ` | Live ${liveOutput.status} (${liveOutput.activeUniverseCount})` : ""}
        {audioStatus !== null && <span className={`sequence-audio-status sequence-audio-status-${audioStatus.tone}`}>{audioStatus.label}</span>}
        {timingSummary !== null && <span className="sequence-timing-status">{timingSummary}</span>}
      </span>
    </div>
  );
}

function useSequencePreview(preview: AppSnapshotDto["preview"]): LivePreview {
  const [eventPreview, setEventPreview] = useState<PreviewStateEvent | null>(null);
  const [animatedPositionSeconds, setAnimatedPositionSeconds] = useState(preview.positionSeconds);
  const anchor = useRef({
    preview,
    positionSeconds: preview.positionSeconds,
    anchoredAt: 0
  });

  useEffect(() => {
    let dispose: (() => void) | undefined;
    let disposed = false;
    void (async () => {
      dispose = await listen<PreviewStateEvent>("preview_state_changed", (event) => {
        if (!disposed) {
          anchor.current = {
            preview: event.payload,
            positionSeconds: event.payload.positionSeconds,
            anchoredAt: performance.now()
          };
          setAnimatedPositionSeconds(event.payload.positionSeconds);
          setEventPreview(event.payload);
        }
      });
    })();
    return () => {
      disposed = true;
      dispose?.();
    };
  }, []);

  const livePreview = eventPreview ?? preview;
  const livePreviewRef = useRef(livePreview);

  useEffect(() => {
    livePreviewRef.current = livePreview;
    anchor.current = {
      preview: livePreview,
      positionSeconds: livePreview.positionSeconds,
      anchoredAt: performance.now()
    };
  }, [livePreview]);

  useEffect(() => {
    let frame = 0;
    const tick = () => {
      const latest = livePreviewRef.current;
      const current = anchor.current;
      if (!latest.isPlaying || !current.preview.isPlaying) {
        setAnimatedPositionSeconds(latest.positionSeconds);
        return;
      }
      const elapsedSeconds = current.anchoredAt > 0 ? (performance.now() - current.anchoredAt) / 1000 : 0;
      setAnimatedPositionSeconds(clamp(current.positionSeconds + elapsedSeconds, 0, current.preview.durationSeconds));
      frame = window.requestAnimationFrame(tick);
    };
    frame = window.requestAnimationFrame(tick);
    return () => {
      window.cancelAnimationFrame(frame);
    };
  }, [livePreview.isPlaying, livePreview.positionSeconds]);

  return livePreview.isPlaying
    ? {
        ...livePreview,
        positionSeconds: animatedPositionSeconds
      }
    : livePreview;
}

function useMarkDisplayMode() {
  const [mode, setMode] = useState<MarkDisplayMode>(markDisplayMode);

  useEffect(() => {
    const listener = (event: Event) => {
      setMode((event as CustomEvent<MarkDisplayMode>).detail);
    };
    window.addEventListener("dawn-mark-display-mode", listener);
    return () => {
      window.removeEventListener("dawn-mark-display-mode", listener);
    };
  }, []);

  return [mode, setMode] as const;
}

function useSequenceAudioStatus(preview: AppSnapshotDto["preview"]) {
  const [loadedNoticeVisible, setLoadedNoticeVisible] = useState(false);
  const previousStatus = useRef(preview.audioPlaybackStatus);

  useEffect(() => {
    if (isAudioLoadingStatus(previousStatus.current) && !isAudioLoadingStatus(preview.audioPlaybackStatus)) {
      setLoadedNoticeVisible(true);
    }
    previousStatus.current = preview.audioPlaybackStatus;
  }, [preview.audioPlaybackStatus]);

  useEffect(() => {
    const timer = window.setTimeout(() => {
      setLoadedNoticeVisible(false);
    }, 2500);
    return () => {
      window.clearTimeout(timer);
    };
  }, [loadedNoticeVisible]);

  switch (preview.audioPlaybackStatus) {
    case "loading":
      return { label: "Loading audio...", tone: "loading" };
    case "loading_to_play":
      return { label: "Loading audio - will play", tone: "loading" };
    case "playing":
      return { label: "Audio playing", tone: "ready" };
    case "ready":
      return preview.audio !== null ? { label: "Audio ready", tone: "ready" } : null;
    case "missing":
      return { label: "Audio missing", tone: "error" };
    case "error":
      return { label: "Audio error", tone: "error" };
    default:
      return loadedNoticeVisible ? { label: "Audio loaded", tone: "ready" } : null;
  }
}

function previewTimingSummary(timing: PreviewTiming | undefined) {
  if (timing === undefined || timing.backendSeconds === 0) return null;
  const frameAudio = timing.frameMinusAudioMs;
  const snapshotAudio = timing.snapshotMinusAudioMs;
  const parts = [
    `fps ${timing.activeFps}/${timing.targetFps}`,
    `target ${formatMs(timing.targetFrameMs)}`,
    `sleep ${formatMs(timing.sleepPlannedMs)}`,
    frameAudio === null ? null : `frame-audio ${formatSignedMs(frameAudio)}`,
    snapshotAudio === null ? null : `playhead-audio ${formatSignedMs(snapshotAudio)}`,
    `interval ${formatMs(timing.loopIntervalMs)}`,
    `loop ${formatMs(timing.loopElapsedMs)}`,
    `model ${formatMs(timing.modelUpdateMs)}`,
    `audio ${formatMs(timing.audioPollMs)}`,
    `apply ${formatMs(timing.audioApplyMs)}`,
    `render ${formatMs(timing.renderMs)}`,
    `publish ${formatMs(timing.publishMs)}`,
    `event ${formatMs(timing.eventIntervalMs)}`
  ].filter((part): part is string => part !== null);
  return ` | ${parts.join(" | ")}`;
}

function isAudioLoadingStatus(status: AudioPlaybackStatus) {
  return status === "loading" || status === "loading_to_play";
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
  previewPositionSeconds,
  previewHomeSeconds,
  selected,
  setSelected,
  sequenceSelection,
  setSequenceSelection,
  activeMarkCollectionKey,
  setActiveMarkCollectionKey,
  visibleMarkCollectionKeys,
  setVisibleMarkCollectionKeys
}: {
  document: SequenceDocumentDto;
  previewPositionSeconds: number;
  previewHomeSeconds: number;
  selected: string | null;
  setSelected: (id: string | null) => void;
  sequenceSelection: SequenceSelection;
  setSequenceSelection: (selection: SequenceSelection) => void;
  activeMarkCollectionKey: string | null;
  setActiveMarkCollectionKey: (key: string | null) => void;
  visibleMarkCollectionKeys: Set<string>;
  setVisibleMarkCollectionKeys: (keys: Set<string>) => void;
}) {
  const canvas = useRef<HTMLCanvasElement | null>(null);
  const drag = useRef<DragState>(null);
  const sequenceSelectionRef = useRef<SequenceSelection>(sequenceSelection);
  const [preview, setPreview] = useState<SequencePreview | null>(null);
  const [groupPreview, setGroupPreview] = useState<SequencePreview[]>([]);
  const [markPreviews, setMarkPreviews] = useState<MarkPreviewLookup>(() => new Map());
  const [sequenceContextMenu, setSequenceContextMenu] = useState<SequenceContextMenu | null>(null);
  const [hover, setHover] = useState<SequenceHover>(null);
  const [dragCursor, setDragCursor] = useState<"grabbing" | null>(null);
  const [selectedLaneIndex, setSelectedLaneIndex] = useState<number | null>(null);
  const [selectedTimeSeconds, setSelectedTimeSeconds] = useState<number | null>(null);
  const [marquee, setMarquee] = useState<SequenceMarquee | null>(null);
  const [canvasSize, setCanvasSize] = useState({ width: 0, height: 0 });
  const [viewport, setViewport] = useState({ pxPerSecond: 80, laneHeight: 42, scrollXSeconds: 0, scrollY: 0 });
  const [previewImages, setPreviewImages] = useState<Map<number, SequencePreviewImage>>(() => new Map());
  const [previewRequestTick, setPreviewRequestTick] = useState(0);
  const previewImagesRef = useRef(previewImages);
  const inFlightPreviewSignatures = useRef<Set<string>>(new Set());
  const initializedViewportKey = useRef<string | null>(null);
  const left = 128;
  const top = 66;
  const audioStripTop = 28;
  const audioStripHeight = top - audioStripTop;
  const waveform = useSequenceWaveform(document.audio);
  const [mode] = useMarkDisplayMode();
  const effectPreviewSignatures = useMemo(() => sequencePreviewSignatures(document), [document]);
  const effectPreviewSignaturesRef = useRef(effectPreviewSignatures);
  const visibleMarkCollections = useMemo(
    () => document.markCollections.filter((collection) => visibleMarkCollectionKeys.has(collection.key)),
    [document.markCollections, visibleMarkCollectionKeys]
  );
  const canvasCursor =
    dragCursor ?? (hover === null ? undefined : hover.kind === "mark" ? "pointer" : hover.resize === "none" ? "grab" : "ew-resize");

  const updateSequenceSelection = useCallback((selection: SequenceSelection) => {
    sequenceSelectionRef.current = selection;
    setSequenceSelection(selection);
  }, [setSequenceSelection]);

  useEffect(() => {
    sequenceSelectionRef.current = sequenceSelection;
  }, [sequenceSelection]);

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
      const key = `${document.durationSeconds}:${document.lanes.length}`;
      if (rect.width > 0 && initializedViewportKey.current !== key) {
        initializedViewportKey.current = key;
        setViewport({
          pxPerSecond: clamp(timelineWidth / Math.max(1, document.durationSeconds), 20, 600),
          laneHeight: 42,
          scrollXSeconds: 0,
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
  }, [document.durationSeconds, document.lanes.length, left]);

  const visibleClips = useMemo(
    () => buildSequenceClipLayout(document, groupPreview.length > 0 ? groupPreview : preview === null ? [] : [preview], viewport, left, top),
    [document, groupPreview, left, preview, top, viewport]
  );
  const selectedEffectIds = useMemo(() => new Set<number>(sequenceSelection?.type === "effects" ? sequenceSelection.ids : []), [sequenceSelection]);
  const selectedMarkKeys = useMemo(
    () => new Set<string>(sequenceSelection?.type === "marks" ? sequenceSelection.marks.map(markKey) : []),
    [sequenceSelection]
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
    const maxScrollXSeconds = Math.max(0, document.durationSeconds - timelineWidth / viewport.pxPerSecond);
    const maxScrollY = Math.max(0, totalLaneHeight - Math.max(1, rect.height - top));
    const scrollXSeconds = clamp(viewport.scrollXSeconds, 0, maxScrollXSeconds);
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
      if (selectedLaneIndex === index) {
        ctx.fillStyle = "rgb(106 191 138 / 12%)";
        ctx.fillRect(left, y, timelineWidth, viewport.laneHeight);
      }
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
    ctx.fillStyle = "#111214";
    ctx.fillRect(left, audioStripTop, timelineWidth, audioStripHeight);
    ctx.fillStyle = "#c7c0b6";
    ctx.fillText(document.audio?.fileName ?? "Audio", 12, audioStripTop + audioStripHeight / 2 + 4);
    ctx.strokeStyle = "#2c3036";
    ctx.beginPath();
    ctx.moveTo(0, top + 0.5);
    ctx.lineTo(rect.width, top + 0.5);
    ctx.stroke();

    drawWaveformStrip(
      ctx,
      waveform.audio,
      left,
      audioStripTop,
      timelineWidth,
      audioStripHeight,
      document.durationSeconds,
      viewport.pxPerSecond,
      scrollXSeconds
    );
    drawTimelineGrid(ctx, left, top, rect.width, rect.height, viewport.pxPerSecond, scrollXSeconds, document.frameRate);
    drawSequenceMarks(
      ctx,
      visibleMarkCollections,
      selected,
      selectedMarkKeys,
      mode,
      left,
      audioStripTop,
      audioStripHeight,
      timelineWidth,
      rect.height,
      viewport.pxPerSecond,
      scrollXSeconds,
      committedMarkPreviews(visibleMarkCollections, markPreviews)
    );

    ctx.save();
    ctx.beginPath();
    ctx.rect(left, top, timelineWidth, rect.height - top);
    ctx.clip();
    for (const clip of visibleClips) {
      if (clip.rect.x + clip.rect.width < left || clip.rect.x > rect.width || clip.rect.y + clip.rect.height < top || clip.rect.y > rect.height) {
        continue;
      }
      const hoverResize = hover?.kind === "effect" && hover.effectId === clip.effect.id ? hover.resize : null;
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
      if (hoverResize !== null) {
        ctx.fillStyle = "rgb(255 250 240 / 10%)";
        ctx.fillRect(clip.rect.x, clip.rect.y, clip.rect.width, clip.rect.height);
      }
      const clipSelected = selectedEffectIds.has(clip.effect.id) || selected === `effect:${clip.effect.id}`;
      ctx.strokeStyle = clipSelected ? "#f0f0f0" : hoverResize !== null ? "#d8d2c9" : "#8a8d93";
      ctx.lineWidth = clipSelected || hoverResize !== null ? 2 : 1;
      ctx.strokeRect(clip.rect.x + 0.5, clip.rect.y + 0.5, Math.max(0, clip.rect.width - 1), Math.max(0, clip.rect.height - 1));
      if (hoverResize === "left" || hoverResize === "right") {
        const handleX = hoverResize === "left" ? clip.rect.x : clip.rect.x + clip.rect.width;
        ctx.fillStyle = "#f0c46b";
        ctx.fillRect(handleX - 2, clip.rect.y + 4, 4, Math.max(4, clip.rect.height - 8));
      }
    }
    ctx.restore();

    const playheadX = left + (clamp(previewPositionSeconds, 0, document.durationSeconds) - scrollXSeconds) * viewport.pxPerSecond;
    const homeX = left + (clamp(previewHomeSeconds, 0, document.durationSeconds) - scrollXSeconds) * viewport.pxPerSecond;
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
    if (selectedTimeSeconds !== null) {
      const selectedX = left + (clamp(selectedTimeSeconds, 0, document.durationSeconds) - scrollXSeconds) * viewport.pxPerSecond;
      if (selectedX >= left && selectedX <= rect.width) {
        ctx.strokeStyle = "#8ecae6";
        ctx.lineWidth = 1;
        ctx.beginPath();
        ctx.moveTo(selectedX + 0.5, top);
        ctx.lineTo(selectedX + 0.5, rect.height);
        ctx.stroke();
      }
    }
    if (marquee?.active) {
      const box = normalizedRect(marquee.startX, marquee.startY, marquee.x, marquee.y);
      ctx.fillStyle = marquee.mode === "marks" ? "rgb(142 202 230 / 12%)" : "rgb(240 196 107 / 12%)";
      ctx.strokeStyle = marquee.mode === "marks" ? "#8ecae6" : "#f0c46b";
      ctx.lineWidth = 1;
      ctx.fillRect(box.x, box.y, box.width, box.height);
      ctx.strokeRect(box.x + 0.5, box.y + 0.5, Math.max(0, box.width - 1), Math.max(0, box.height - 1));
    }

    ctx.strokeStyle = "#d6a35a";
    ctx.lineWidth = 1;
    ctx.beginPath();
    ctx.moveTo(left + 0.5, top);
    ctx.lineTo(left, rect.height);
    ctx.stroke();
  }, [document, effectPreviewSignatures, left, top, audioStripTop, audioStripHeight, viewport, visibleClips, selected, selectedEffectIds, selectedMarkKeys, previewImages, previewPositionSeconds, previewHomeSeconds, selectedLaneIndex, selectedTimeSeconds, marquee, waveform.audio, visibleMarkCollections, mode, markPreviews, hover]);

  const seekFromCanvas = (event: MouseEvent<HTMLCanvasElement>) => {
    const x = event.nativeEvent.offsetX;
    if (x < left) return;
    const positionSeconds = clamp(Math.round((viewport.scrollXSeconds + (x - left) / viewport.pxPerSecond) / 0.01) * 0.01, 0, document.durationSeconds);
    void runSnapshotCommand(() => commands.previewSeek(positionSeconds));
  };
  const timeFromCanvasX = (x: number) => clamp(roundToNanosecond(viewport.scrollXSeconds + (x - left) / viewport.pxPerSecond), 0, document.durationSeconds);
  const addEffectFromContextMenu = async (script: SequenceEffectScriptDto, menu: SequenceContextMenu) => {
    const hasMarksParams = script.params.some((param) => param.kind === "marks");
    let markCollectionKey = hasMarksParams ? activeMarkCollectionKey ?? document.markCollections[0]?.key ?? null : null;
    if (hasMarksParams && markCollectionKey === null) {
      const newCollectionKey = nextCollectionKey("Marks", document.markCollections);
      await runSnapshotCommand(() =>
        commands.applySequenceGuiEdit({
          type: "createMarkCollection",
          key: newCollectionKey,
          name: "Marks",
          color: defaultMarkColor(document.markCollections.length)
        })
      );
      markCollectionKey = newCollectionKey;
      setActiveMarkCollectionKey(newCollectionKey);
      setVisibleMarkCollectionKeys(new Set([...visibleMarkCollectionKeys, newCollectionKey]));
    }
    const target = document.lanes[menu.laneIndex]?.target ?? document.lanes[0]?.target;
    if (target === undefined) return;
    const scope: SequenceEffectScopeDto = target.kind === "group" ? "wholeTarget" : "perFixture";
    await runSnapshotCommand(() =>
      commands.applySequenceGuiEdit({
        type: "addEffect",
        scriptPath: script.path,
        target,
        scope,
        startSeconds: menu.startSeconds,
        markCollectionKey
      })
    );
  };
  const addMarkFromContextMenu = async (collectionKey: string | null, menu: SequenceContextMenu) => {
    let targetCollectionKey = collectionKey;
    if (targetCollectionKey === null) {
      const newCollectionKey = nextCollectionKey("Marks", document.markCollections);
      await runSnapshotCommand(() =>
        commands.applySequenceGuiEdit({
          type: "createMarkCollection",
          key: newCollectionKey,
          name: "Marks",
          color: defaultMarkColor(document.markCollections.length)
        })
      );
      targetCollectionKey = newCollectionKey;
      setActiveMarkCollectionKey(targetCollectionKey);
      setVisibleMarkCollectionKeys(new Set([...visibleMarkCollectionKeys, targetCollectionKey]));
    }
    await runSnapshotCommand(() =>
      commands.applySequenceGuiEdit({
        type: "addMark",
        collectionKey: targetCollectionKey,
        timeSeconds: menu.startSeconds
      })
    );
  };
  const addMarkAtTime = async (timeSeconds: number) => {
    let collectionKey = activeMarkCollectionKey ?? document.markCollections[0]?.key ?? null;
    if (collectionKey === null) {
      const newCollectionKey = nextCollectionKey("Marks", document.markCollections);
      await runSnapshotCommand(() =>
        commands.applySequenceGuiEdit({
          type: "createMarkCollection",
          key: newCollectionKey,
          name: "Marks",
          color: defaultMarkColor(document.markCollections.length)
        })
      );
      collectionKey = newCollectionKey;
      setActiveMarkCollectionKey(newCollectionKey);
      setVisibleMarkCollectionKeys(new Set([...visibleMarkCollectionKeys, newCollectionKey]));
    }
    await runSnapshotCommand(() =>
      commands.applySequenceGuiEdit({
        type: "addMark",
        collectionKey,
        timeSeconds
      })
    );
    const nextIndex = [...(document.markCollections.find((collection) => collection.key === collectionKey)?.marksSeconds ?? []), timeSeconds]
      .map((markTimeSeconds, index) => ({ markTimeSeconds, index }))
      .sort((leftMark, rightMark) => leftMark.markTimeSeconds - rightMark.markTimeSeconds || leftMark.index - rightMark.index)
      .findIndex((mark) => mark.index === (document.markCollections.find((collection) => collection.key === collectionKey)?.marksSeconds.length ?? 0));
    updateSequenceSelection({ type: "marks", marks: [{ collectionKey, index: Math.max(0, nextIndex) }] });
    setSelected(`mark:${collectionKey}:${Math.max(0, nextIndex)}`);
  };
  const deleteSelectedEffect = async (effectId: number) => {
    await runSnapshotCommand(() => commands.applySequenceGuiEdit({ type: "deleteEffect", id: effectId }));
    setSelected(null);
    updateSequenceSelection(null);
  };
  const deleteContextMark = async (menu: Extract<SequenceContextMenu, { kind: "mark" }>) => {
    await runSnapshotCommand(() =>
      commands.applySequenceGuiEdit({
        type: "deleteMark",
        collectionKey: menu.collectionKey,
        index: menu.index
      })
    );
    setSelected(null);
    updateSequenceSelection(null);
  };
  const retargetContextEffect = async (effectId: number, target: LayoutTargetDto) => {
    await runSnapshotCommand(() => commands.applySequenceGuiEdit({ type: "retargetEffect", id: effectId, target }));
  };
  const markCollectionsForMenu = () => {
    if (activeMarkCollectionKey === null) return document.markCollections;
    return [
      ...document.markCollections.filter((collection) => collection.key === activeMarkCollectionKey),
      ...document.markCollections.filter((collection) => collection.key !== activeMarkCollectionKey)
    ];
  };

  return (
    <div className="sequence-canvas-shell">
      <ContextMenu.Root onOpenChange={(open) => { if (!open) setSequenceContextMenu(null); }}>
        <ContextMenu.Trigger asChild>
          <canvas
            ref={canvas}
            className="gui-canvas"
            style={canvasCursor === undefined ? undefined : { cursor: canvasCursor }}
            tabIndex={0}
      onKeyDown={(event) => {
        const selectedMark = parseSelectedMark(selected);
        const selectedEffectId = parseSelectedEffectId(selected);
        const activeSelection = sequenceSelection ?? selectionFromSingle(selected);
        if ((event.ctrlKey || event.metaKey) && !isTextEntryElement(event.target)) {
          const key = event.key.toLowerCase();
          if ((key === "c" || key === "x") && activeSelection !== null && selectionCount(activeSelection) > 0) {
            event.preventDefault();
            const editType = key === "c" ? "copy" : "cut";
            void commands.applySequenceSelectionEdit({ type: editType, selection: activeSelection }).then((result) => {
              updateSequenceSelection(result.selection);
              setSelected(singleSelectionString(result.selection));
            });
            return;
          }
          if (key === "v") {
            event.preventDefault();
            void commands.applySequenceSelectionEdit({
              type: "paste",
              anchor: { laneIndex: selectedLaneIndex as never, timeSeconds: selectedTimeSeconds as never }
            }).then((result) => {
              updateSequenceSelection(result.selection);
              setSelected(singleSelectionString(result.selection));
            });
            return;
          }
        }
        if (
          selectedMark !== null &&
          (event.key === "ArrowLeft" || event.key === "ArrowRight") &&
          !isTextEntryElement(event.target)
        ) {
          const collection = document.markCollections.find((candidate) => candidate.key === selectedMark.collectionKey);
          const timeSeconds = collection?.marksSeconds[selectedMark.index];
          if (collection === undefined || timeSeconds === undefined) return;
          event.preventDefault();
          event.stopPropagation();
          const deltaSeconds = (event.key === "ArrowLeft" ? -1 : 1) * (event.shiftKey ? 0.01 : 0.001);
          const nextTimeSeconds = clamp(timeSeconds + deltaSeconds, 0, document.durationSeconds);
          const nextIndex = markIndexAfterMove(collection, selectedMark.index, nextTimeSeconds);
          setMarkPreviews(new Map([[markKey(selectedMark), { collectionKey: selectedMark.collectionKey, index: selectedMark.index, timeSeconds: nextTimeSeconds, committedIndex: nextIndex }]]));
          void runSnapshotCommand(() =>
            commands.applySequenceGuiEdit({
              type: "moveMark",
              collectionKey: selectedMark.collectionKey,
              index: selectedMark.index,
              timeSeconds: nextTimeSeconds
            })
          ).then(() => {
            setSelected(`mark:${selectedMark.collectionKey}:${nextIndex}`);
            setMarkPreviews(new Map());
          });
          return;
        }
        if ((event.key !== "Delete" && event.key !== "Backspace") || isTextEntryElement(event.target)) return;
        event.preventDefault();
        if (activeSelection !== null && selectionCount(activeSelection) > 1) {
          void commands.applySequenceSelectionEdit({ type: "delete", selection: activeSelection }).then((result) => {
            updateSequenceSelection(result.selection);
            setSelected(null);
          });
          return;
        }
        if (selectedEffectId !== null) {
          void deleteSelectedEffect(selectedEffectId);
          return;
        }
        if (selectedMark === null) return;
        void runSnapshotCommand(() =>
          commands.applySequenceGuiEdit({
            type: "deleteMark",
            collectionKey: selectedMark.collectionKey,
            index: selectedMark.index
          })
        ).then(() => {
          setSelected(null);
        });
      }}
      onContextMenu={(event) => {
        const x = event.nativeEvent.offsetX;
        const y = event.nativeEvent.offsetY;
        if (x < left || y < top || document.lanes.length === 0) {
          event.preventDefault();
          setSequenceContextMenu(null);
          return;
        }
        const laneIndex = clamp(Math.floor((y - top + viewport.scrollY) / viewport.laneHeight), 0, document.lanes.length - 1);
        const startSeconds = timeFromCanvasX(x);
        const hit = hitSequence(visibleClips, x, y);
        if (hit !== null) {
          setSelected(`effect:${hit.effect.id}`);
          updateSequenceSelection({ type: "effects", ids: [hit.effect.id] });
          setSequenceContextMenu({ kind: "effect", laneIndex: hit.laneIndex, startSeconds, effectId: hit.effect.id });
          return;
        }
        const markHit = hitSequenceMark(visibleMarkCollections, mode, x, y, left, audioStripTop, audioStripHeight, canvasSize.height, viewport);
        if (markHit !== null) {
          setSelected(`mark:${markHit.collectionKey}:${markHit.index}`);
          updateSequenceSelection({ type: "marks", marks: [{ collectionKey: markHit.collectionKey, index: markHit.index }] });
          setActiveMarkCollectionKey(markHit.collectionKey);
          setSequenceContextMenu({ kind: "mark", laneIndex, startSeconds, collectionKey: markHit.collectionKey, index: markHit.index });
          return;
        }
        setSelected(null);
        updateSequenceSelection(null);
        setSequenceContextMenu({ kind: "blank", laneIndex, startSeconds });
      }}
      onMouseDown={(event) => {
        if (event.button !== 0) return;
        event.currentTarget.focus();
        const x = event.nativeEvent.offsetX;
        const y = event.nativeEvent.offsetY;
        setMarkPreviews(new Map());
        if (x >= left && y < top) {
          drag.current = { kind: "sequenceScrub" };
          seekFromCanvas(event);
          return;
        }
        if (x < left && y >= top && document.lanes.length > 0) {
          const laneIndex = clamp(Math.floor((y - top + viewport.scrollY) / viewport.laneHeight), 0, document.lanes.length - 1);
          const ids = document.effects.filter((effect) => document.lanes[laneIndex]?.target !== undefined && targetsEqual(effect.target, document.lanes[laneIndex].target)).map((effect) => effect.id);
          setSelectedLaneIndex(laneIndex);
          updateSequenceSelection(ids.length > 0 ? { type: "effects", ids } : null);
          setSelected(ids.length === 1 ? `effect:${ids[0]}` : null);
          return;
        }
        const hit = hitSequence(visibleClips, event.nativeEvent.offsetX, event.nativeEvent.offsetY);
        if (hit !== null) {
          const activeSelection = sequenceSelectionRef.current;
          const wasAlreadySelected = activeSelection?.type === "effects" && activeSelection.ids.includes(hit.effect.id);
          const nextSelection = wasAlreadySelected && !event.shiftKey && !event.ctrlKey && !event.metaKey
            ? activeSelection
            : nextEffectSelection(activeSelection?.type === "effects" ? activeSelection : null, hit.effect.id, event.shiftKey, event.ctrlKey || event.metaKey);
          updateSequenceSelection(nextSelection);
          setSelected(nextSelection.type === "effects" && nextSelection.ids.length === 1 ? `effect:${nextSelection.ids[0]}` : `effect:${hit.effect.id}`);
          setSelectedLaneIndex(hit.laneIndex);
          setDragCursor("grabbing");
          drag.current = {
            kind: "sequence",
            id: hit.effect.id,
            startX: event.nativeEvent.offsetX,
            originalStartSeconds: hit.effect.startSeconds,
            laneIndex: hit.laneIndex,
            resize: hit.resize
          };
          setPreview({
            id: hit.effect.id,
            startSeconds: hit.effect.startSeconds,
            durationSeconds: hit.effect.durationSeconds,
            laneIndex: hit.laneIndex
          });
          return;
        }
        const markHit = hitSequenceMark(visibleMarkCollections, mode, x, y, left, audioStripTop, audioStripHeight, canvasSize.height, viewport);
        if (markHit !== null) {
          const mark = { collectionKey: markHit.collectionKey, index: markHit.index };
          const activeSelection = sequenceSelectionRef.current;
          const wasAlreadySelected = activeSelection?.type === "marks" && activeSelection.marks.some((candidate) => candidate.collectionKey === mark.collectionKey && candidate.index === mark.index);
          const nextSelection = wasAlreadySelected && !event.shiftKey && !event.ctrlKey && !event.metaKey
            ? activeSelection
            : nextMarkSelection(activeSelection?.type === "marks" ? activeSelection : null, mark, event.shiftKey, event.ctrlKey || event.metaKey);
          updateSequenceSelection(nextSelection);
          setSelected(nextSelection.type === "marks" && nextSelection.marks.length === 1 ? `mark:${mark.collectionKey}:${mark.index}` : `mark:${mark.collectionKey}:${mark.index}`);
          setActiveMarkCollectionKey(markHit.collectionKey);
          drag.current = {
            kind: "mark",
            collectionKey: markHit.collectionKey,
            index: markHit.index,
            startX: x,
            originalTimeSeconds: markHit.timeSeconds
          };
          return;
        }
        if (x >= left && y >= top) {
          const laneIndex = clamp(Math.floor((y - top + viewport.scrollY) / viewport.laneHeight), 0, Math.max(0, document.lanes.length - 1));
          const timeSeconds = timeFromCanvasX(x);
          setSelectedLaneIndex(laneIndex);
          setSelectedTimeSeconds(timeSeconds);
          setSelected(null);
          updateSequenceSelection(null);
          const state = { mode: event.altKey ? "marks" as const : "effects" as const, startX: x, startY: y, x, y, active: false, shift: event.shiftKey, ctrl: event.ctrlKey || event.metaKey };
          drag.current = { kind: "marquee", state };
          setMarquee(state);
        }
      }}
      onMouseMove={(event) => {
        const current = drag.current;
        if (current?.kind === "sequenceScrub") {
          seekFromCanvas(event);
          return;
        }
        if (current?.kind === "marquee") {
          const next = {
            ...current.state,
            x: event.nativeEvent.offsetX,
            y: event.nativeEvent.offsetY,
            active: current.state.active || Math.hypot(event.nativeEvent.offsetX - current.state.startX, event.nativeEvent.offsetY - current.state.startY) >= 4
          };
          current.state = next;
          setMarquee(next);
          if (next.active) {
            const selectedByBox = next.mode === "effects"
              ? selectionFromMarqueeEffects(visibleClips, next)
              : selectionFromMarqueeMarks(visibleMarkCollections, mode, next, left, audioStripTop, audioStripHeight, canvasSize.height, viewport);
            updateSequenceSelection(mergeSequenceSelection(sequenceSelectionRef.current, selectedByBox, next.shift, next.ctrl));
            setSelected(null);
          }
          return;
        }
        if (current?.kind === "mark") {
          const deltaSeconds = roundToNanosecond((event.nativeEvent.offsetX - current.startX) / viewport.pxPerSecond);
          const timeSeconds = clamp(current.originalTimeSeconds + deltaSeconds, 0, document.durationSeconds);
          setSelected(`mark:${current.collectionKey}:${current.index}`);
          const collection = document.markCollections.find((candidate) => candidate.key === current.collectionKey);
          const committedIndex = collection === undefined ? current.index : markIndexAfterMove(collection, current.index, timeSeconds);
          const activeSelection = sequenceSelectionRef.current;
          if (activeSelection?.type === "marks" && activeSelection.marks.length > 1 && activeSelection.marks.some((mark) => mark.collectionKey === current.collectionKey && mark.index === current.index)) {
            const constrainedDelta = constrainMarkDelta(document, activeSelection.marks, deltaSeconds);
            setMarkPreviews(markMovePreviews(document, activeSelection.marks, constrainedDelta));
          } else {
            setMarkPreviews(new Map([[markKey({ collectionKey: current.collectionKey, index: current.index }), { collectionKey: current.collectionKey, index: current.index, timeSeconds, committedIndex }]]));
          }
          setPreview(null);
          setGroupPreview([]);
          return;
        }
        if (!current) {
          const x = event.nativeEvent.offsetX;
          const y = event.nativeEvent.offsetY;
          const hit = hitSequence(visibleClips, x, y);
          const markHit =
            hit === null
              ? hitSequenceMark(visibleMarkCollections, mode, x, y, left, audioStripTop, audioStripHeight, canvasSize.height, viewport)
              : null;
          const nextHover: SequenceHover =
            hit !== null
              ? { kind: "effect", effectId: hit.effect.id, resize: hit.resize }
              : markHit !== null
                ? { kind: "mark", collectionKey: markHit.collectionKey, index: markHit.index }
                : null;
          setHover((previous) =>
            sequenceHoverEqual(previous, nextHover) ? previous : nextHover
          );
          return;
        }
        if (current.kind !== "sequence") return;
        const effect = document.effects.find((candidate) => candidate.id === current.id);
        if (effect === undefined) return;
        const deltaSeconds = roundToNanosecond((event.nativeEvent.offsetX - current.startX) / viewport.pxPerSecond);
        const laneIndex =
          current.resize === "none"
            ? clamp(Math.floor((event.nativeEvent.offsetY - top + viewport.scrollY) / viewport.laneHeight), 0, document.lanes.length - 1)
            : current.laneIndex;
        const activeEffectSelection = sequenceSelectionRef.current;
        if (activeEffectSelection?.type === "effects" && activeEffectSelection.ids.includes(current.id) && activeEffectSelection.ids.length > 1) {
          if (current.resize === "none") {
            const constrainedDelta = constrainEffectMoveDelta(document, activeEffectSelection.ids, deltaSeconds);
            const laneDelta = constrainEffectLaneDelta(document, activeEffectSelection.ids, laneIndex - current.laneIndex);
            setGroupPreview(effectMovePreviews(document, activeEffectSelection.ids, constrainedDelta, laneDelta));
          } else {
            const constrainedDelta = constrainEffectResizeDelta(document, activeEffectSelection.ids, current.resize, deltaSeconds);
            setGroupPreview(effectResizePreviews(document, activeEffectSelection.ids, current.resize, constrainedDelta));
          }
          setPreview(null);
          return;
        }
        setGroupPreview([]);
        if (current.resize === "left") {
          const startSeconds = clamp(current.originalStartSeconds + deltaSeconds, 0, effect.startSeconds + effect.durationSeconds - MIN_EFFECT_DURATION_SECONDS);
          setPreview({ id: effect.id, startSeconds, durationSeconds: effect.startSeconds + effect.durationSeconds - startSeconds, laneIndex });
        } else if (current.resize === "right") {
          setPreview({ id: effect.id, startSeconds: effect.startSeconds, durationSeconds: Math.max(MIN_EFFECT_DURATION_SECONDS, effect.durationSeconds + deltaSeconds), laneIndex });
        } else {
          setPreview({ id: effect.id, startSeconds: clamp(current.originalStartSeconds + deltaSeconds, 0, Math.max(0, document.durationSeconds - effect.durationSeconds)), durationSeconds: effect.durationSeconds, laneIndex });
        }
      }}
      onMouseUp={(event) => {
        const current = drag.current;
        drag.current = null;
        setDragCursor(null);
        setMarquee(null);
        if (current?.kind === "marquee") {
          if (!current.state.active && current.state.mode === "marks") {
            void addMarkAtTime(timeFromCanvasX(current.state.startX));
          }
          return;
        }
        if (current?.kind === "mark") {
          const deltaSeconds = roundToNanosecond((event.nativeEvent.offsetX - current.startX) / viewport.pxPerSecond);
          const activeSelection = sequenceSelectionRef.current;
          if (activeSelection?.type === "marks" && activeSelection.marks.some((mark) => mark.collectionKey === current.collectionKey && mark.index === current.index)) {
            const constrainedDelta = constrainMarkDelta(document, activeSelection.marks, deltaSeconds);
            void commands.applySequenceSelectionEdit({
              type: "moveMarks",
              marks: activeSelection.marks,
              timeDeltaSeconds: constrainedDelta
            }).then((result) => {
              updateSequenceSelection(result.selection);
              setSelected(null);
              setMarkPreviews(new Map());
            });
            return;
          }
          const timeSeconds = clamp(current.originalTimeSeconds + deltaSeconds, 0, document.durationSeconds);
          const collection = document.markCollections.find((candidate) => candidate.key === current.collectionKey);
          const nextIndex = collection === undefined ? current.index : markIndexAfterMove(collection, current.index, timeSeconds);
          void runSnapshotCommand(() =>
            commands.applySequenceGuiEdit({
              type: "moveMark",
              collectionKey: current.collectionKey,
              index: current.index,
              timeSeconds
            })
          ).then(() => {
            setSelected(`mark:${current.collectionKey}:${nextIndex}`);
            setMarkPreviews(new Map());
          });
          return;
        }
        if (!current || current.kind !== "sequence") return;
        const activeSelection = sequenceSelectionRef.current;
        if (activeSelection?.type === "effects" && activeSelection.ids.length > 1 && activeSelection.ids.includes(current.id)) {
          const deltaSeconds = current.resize === "none"
            ? roundToNanosecond((event.nativeEvent.offsetX - current.startX) / viewport.pxPerSecond)
            : roundToNanosecond((event.nativeEvent.offsetX - current.startX) / viewport.pxPerSecond);
          const rawLaneIndex = clamp(Math.floor((event.nativeEvent.offsetY - top + viewport.scrollY) / viewport.laneHeight), 0, document.lanes.length - 1);
          const laneDelta = current.resize === "none" ? constrainEffectLaneDelta(document, activeSelection.ids, rawLaneIndex - current.laneIndex) : 0;
          const edit = current.resize === "none"
            ? { type: "moveEffects" as const, ids: activeSelection.ids, timeDeltaSeconds: constrainEffectMoveDelta(document, activeSelection.ids, deltaSeconds), laneDelta }
            : { type: "resizeEffects" as const, ids: activeSelection.ids, edge: current.resize, timeDeltaSeconds: constrainEffectResizeDelta(document, activeSelection.ids, current.resize, deltaSeconds) };
          void commands.applySequenceSelectionEdit(edit).then((result) => {
            updateSequenceSelection(result.selection);
            setSelected(null);
            setPreview(null);
            setGroupPreview([]);
          });
          return;
        }
        if (!preview) return;
        const committedPreview = preview;
        const edit = () =>
          current.resize === "none"
            ? commands.applySequenceGuiEdit({
                type: "moveEffect",
                id: committedPreview.id,
                startSeconds: committedPreview.startSeconds,
                target: document.lanes[committedPreview.laneIndex]?.target ?? null
              })
            : commands.applySequenceGuiEdit({
                type: "resizeEffect",
                id: committedPreview.id,
                startSeconds: committedPreview.startSeconds,
                durationSeconds: committedPreview.durationSeconds
              });
        void runSnapshotCommand(edit).finally(() => {
          setPreview((currentPreview) => (currentPreview === committedPreview ? null : currentPreview));
          setGroupPreview([]);
        });
      }}
      onMouseLeave={() => {
        if (drag.current === null) setHover(null);
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
          const maxScrollXSeconds = Math.max(0, document.durationSeconds - timelineWidth / current.pxPerSecond);
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
            const anchorTime = current.scrollXSeconds + anchorX / current.pxPerSecond;
            const nextPxPerSecond = clamp(current.pxPerSecond * Math.exp(-event.deltaY * 0.002), 20, 12000);
            const nextScrollXSeconds = anchorTime - anchorX / nextPxPerSecond;
            return {
              ...current,
              pxPerSecond: nextPxPerSecond,
              scrollXSeconds: clamp(nextScrollXSeconds, 0, Math.max(0, document.durationSeconds - timelineWidth / nextPxPerSecond))
            };
          }
          if (event.shiftKey) {
            return {
              ...current,
              scrollXSeconds: clamp(current.scrollXSeconds + event.deltaY / current.pxPerSecond, 0, maxScrollXSeconds)
            };
          }
          return {
            ...current,
            scrollY: clamp(current.scrollY + event.deltaY, 0, maxScrollY)
          };
        });
      }}
          />
        </ContextMenu.Trigger>
        {sequenceContextMenu !== null && (
          <ContextMenu.Portal>
            <ContextMenu.Content className="menu-content">
              <ContextMenu.Sub>
                <ContextMenu.SubTrigger className="menu-item">
                  Add Effect <span className="shortcut">›</span>
                </ContextMenu.SubTrigger>
                <ContextMenu.Portal>
                  <ContextMenu.SubContent className="menu-content">
                    {document.effectScripts.length === 0 ? (
                      <ContextMenu.Item className="menu-item" disabled>
                        No effect scripts
                      </ContextMenu.Item>
                    ) : (
                      document.effectScripts.map((script) => (
                        <ContextMenu.Item
                          key={script.path}
                          className="menu-item"
                          onSelect={() => void addEffectFromContextMenu(script, sequenceContextMenu)}
                        >
                          {script.name}
                        </ContextMenu.Item>
                      ))
                    )}
                  </ContextMenu.SubContent>
                </ContextMenu.Portal>
              </ContextMenu.Sub>
              <ContextMenu.Item
                className="menu-item"
                onSelect={() => {
                  void runSnapshotCommand(() => commands.previewSeek(sequenceContextMenu.startSeconds));
                }}
              >
                Set Playhead Here
              </ContextMenu.Item>
              <ContextMenu.Sub>
                <ContextMenu.SubTrigger className="menu-item">
                  Add Mark <span className="shortcut">›</span>
                </ContextMenu.SubTrigger>
                <ContextMenu.Portal>
                  <ContextMenu.SubContent className="menu-content">
                    {document.markCollections.length === 0 ? (
                      <ContextMenu.Item className="menu-item" onSelect={() => void addMarkFromContextMenu(null, sequenceContextMenu)}>
                        Marks
                      </ContextMenu.Item>
                    ) : (
                      markCollectionsForMenu().map((collection) => (
                        <ContextMenu.Item
                          key={collection.key}
                          className="menu-item"
                          onSelect={() => void addMarkFromContextMenu(collection.key, sequenceContextMenu)}
                        >
                          <span style={{ color: collection.color }}>{collection.name}</span>
                        </ContextMenu.Item>
                      ))
                    )}
                  </ContextMenu.SubContent>
                </ContextMenu.Portal>
              </ContextMenu.Sub>
              <ContextMenu.Item className="menu-item" disabled>
                Add Automation Clip
              </ContextMenu.Item>
              {sequenceContextMenu.kind === "effect" && (
                <>
                  <ContextMenu.Separator className="menu-separator" />
                  <ContextMenu.Sub>
                    <ContextMenu.SubTrigger className="menu-item">
                      Retarget Effect <span className="shortcut">›</span>
                    </ContextMenu.SubTrigger>
                    <ContextMenu.Portal>
                      <ContextMenu.SubContent className="menu-content">
                        {document.lanes.map((lane) => (
                          <ContextMenu.Item
                            key={`${lane.target.kind}:${lane.target.name}`}
                            className="menu-item"
                            onSelect={() => void retargetContextEffect(sequenceContextMenu.effectId, lane.target)}
                          >
                            {lane.label}
                          </ContextMenu.Item>
                        ))}
                      </ContextMenu.SubContent>
                    </ContextMenu.Portal>
                  </ContextMenu.Sub>
                  <ContextMenu.Item className="menu-item danger" onSelect={() => void deleteSelectedEffect(sequenceContextMenu.effectId)}>
                    <Trash2 size={14} /> Delete Effect
                  </ContextMenu.Item>
                </>
              )}
              {sequenceContextMenu.kind === "mark" && (
                <>
                  <ContextMenu.Separator className="menu-separator" />
                  <ContextMenu.Item className="menu-item danger" onSelect={() => void deleteContextMark(sequenceContextMenu)}>
                    <Trash2 size={14} /> Delete Mark
                  </ContextMenu.Item>
                </>
              )}
            </ContextMenu.Content>
          </ContextMenu.Portal>
        )}
      </ContextMenu.Root>
    </div>
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
            x: round6(current.original.position.x + world.x - current.startX),
            y: round6(current.original.position.y + world.y - current.startY)
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
            transform: denormalizeTransform(current.preview)
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
        current.preview = { x: round6(world.x), y: round6(world.y), z: current.preview.z };
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
            point: denormalizePoint(current.preview)
          })
        );
      }}
    />
  );
}

function GuiInspector({
  gui,
  selected,
  setSelected,
  sequenceSelection,
  activeMarkCollectionKey,
  setActiveMarkCollectionKey,
  visibleMarkCollectionKeys,
  setVisibleMarkCollectionKeys
}: {
  gui: ReadyGuiDocumentDto;
  selected: string | null;
  setSelected: (id: string | null) => void;
  sequenceSelection: SequenceSelection;
  activeMarkCollectionKey: string | null;
  setActiveMarkCollectionKey: (key: string | null) => void;
  visibleMarkCollectionKeys: Set<string>;
  setVisibleMarkCollectionKeys: (keys: Set<string>) => void;
}) {
  if (gui.type === "sequence") {
    const id = parseSelectedEffectId(selected);
    const effect = gui.document.effects.find((candidate) => candidate.id === id);
    const selectedMark = parseSelectedMark(selected);
    const selectedMarkCollection = selectedMark === null ? null : gui.document.markCollections.find((collection) => collection.key === selectedMark.collectionKey) ?? null;
    const activeCollection = gui.document.markCollections.find((collection) => collection.key === activeMarkCollectionKey) ?? gui.document.markCollections[0] ?? null;
    const selectedMarkTime = selectedMarkCollection?.marksSeconds[selectedMark?.index ?? -1];
    const createCollection = () => {
      const name = "Marks";
      const key = nextCollectionKey(name, gui.document.markCollections);
      void runSnapshotCommand(() =>
        commands.applySequenceGuiEdit({
          type: "createMarkCollection",
          key,
          name,
          color: defaultMarkColor(gui.document.markCollections.length)
        })
      ).then(() => {
        setActiveMarkCollectionKey(key);
        setVisibleMarkCollectionKeys(new Set([...visibleMarkCollectionKeys, key]));
      });
    };
    if (sequenceSelection !== null && selectionCount(sequenceSelection) > 1 && selectionCompatibleWithFocusedItem(sequenceSelection, selected)) {
      return (
        <InspectorScrollArea>
          <h2>{sequenceSelection.type === "effects" ? "Effects" : "Marks"}</h2>
          <div className="inspector-readout-grid">
            <Readout label="Selected" value={String(selectionCount(sequenceSelection))} />
          </div>
          <button
            type="button"
            onClick={() =>
              void commands.applySequenceSelectionEdit({ type: "delete", selection: sequenceSelection }).then(() => {
                setSelected(null);
              })
            }
          >
            Delete
          </button>
        </InspectorScrollArea>
      );
    }
    const deleteActiveCollection = () => {
      if (activeCollection === null) return;
      if (activeCollection.marksSeconds.length > 0 && !window.confirm(`Delete ${activeCollection.name} and ${activeCollection.marksSeconds.length} marks?`)) return;
      void runSnapshotCommand(() =>
        commands.applySequenceGuiEdit({
          type: "deleteMarkCollection",
          key: activeCollection.key
        })
      ).then(() => {
        setSelected(null);
        setActiveMarkCollectionKey(null);
      });
    };
    if (selectedMark !== null && selectedMarkCollection !== null && selectedMarkTime !== undefined) {
      return (
        <InspectorScrollArea>
          <h2>Mark</h2>
          <div className="inspector-readout-grid">
            <Readout label="Collection" value={selectedMarkCollection.name} />
            <Readout label="Time" value={formatSeconds(selectedMarkTime)} />
            <Readout label="Color" value={selectedMarkCollection.color} swatch={selectedMarkCollection.color} />
          </div>
          <button
            type="button"
            onClick={() =>
              void runSnapshotCommand(() =>
                commands.applySequenceGuiEdit({
                  type: "deleteMark",
                  collectionKey: selectedMark.collectionKey,
                  index: selectedMark.index
                })
              ).then(() => {
                setSelected(null);
              })
            }
          >
            Delete mark
          </button>
        </InspectorScrollArea>
      );
    }
    if (effect !== undefined) {
      const currentScriptPath = selectedEffectScriptPath(effect, gui.document.effectScripts);
      const resizeEffect = (startSeconds: number, durationSeconds: number) =>
        runSnapshotCommand(() =>
          commands.applySequenceGuiEdit({
            type: "resizeEffect",
            id: effect.id,
            startSeconds: Math.max(0, roundToNanosecond(startSeconds)),
            durationSeconds: Math.max(0.000000001, roundToNanosecond(durationSeconds))
          })
        );
      return (
        <InspectorScrollArea>
          <h2>Effect</h2>
          <div className="inspector-readout-grid">
            <div className="inspector-inline-row">
              <label>
                Start
                <input
                  key={`${effect.id}:start:${effect.startSeconds}`}
                  type="number"
                  min={0}
                  step="any"
                  defaultValue={effect.startSeconds}
                  onBlur={(event) => {
                    const nextStartSeconds = Number(event.currentTarget.value);
                    if (!Number.isFinite(nextStartSeconds) || roundToNanosecond(nextStartSeconds) === effect.startSeconds) return;
                    void resizeEffect(nextStartSeconds, effect.durationSeconds);
                  }}
                />
              </label>
              <label>
                Duration
                <input
                  key={`${effect.id}:duration:${effect.durationSeconds}`}
                  type="number"
                  min={0.000000001}
                  step="any"
                  defaultValue={effect.durationSeconds}
                  onBlur={(event) => {
                    const nextDurationSeconds = Number(event.currentTarget.value);
                    if (!Number.isFinite(nextDurationSeconds) || roundToNanosecond(nextDurationSeconds) === effect.durationSeconds) return;
                    void resizeEffect(effect.startSeconds, nextDurationSeconds);
                  }}
                />
              </label>
            </div>
          </div>
          <label>
            Effect type
            <select
              value={currentScriptPath}
              disabled={gui.document.effectScripts.length === 0}
              onChange={(event) =>
                void runSnapshotCommand(() =>
                  commands.applySequenceGuiEdit({
                    type: "changeEffectScript",
                    id: effect.id,
                    scriptPath: event.currentTarget.value
                  })
                )
              }
            >
              {currentScriptPath === "" && <option value="">{effect.script}</option>}
              {gui.document.effectScripts.map((script) => (
                <option key={script.path} value={script.path}>
                  {script.name}
                </option>
              ))}
            </select>
          </label>
          <label>
            Scope
            <select
              value={effect.scope}
              onChange={(event) =>
                void runSnapshotCommand(() =>
                  commands.applySequenceGuiEdit({
                    type: "setEffectScope",
                    id: effect.id,
                    scope: event.currentTarget.value as SequenceEffectScopeDto
                  })
                )
              }
            >
              <option value="perFixture">Per fixture</option>
              <option value="wholeTarget">Whole target</option>
            </select>
          </label>
          {effect.params.length > 0 && (
            <div className="effect-param-section">
              <h3>Parameters</h3>
              {effect.params.map((param) => (
                <EffectParamInput
                  key={`${effect.id}:${param.name}`}
                  effectId={effect.id}
                  param={param}
                  markCollections={gui.document.markCollections}
                />
              ))}
            </div>
          )}
          <button onClick={() => void runSnapshotCommand(() => commands.applySequenceGuiEdit({ type: "deleteEffect", id: effect.id }))}>Delete</button>
        </InspectorScrollArea>
      );
    }
    return (
      <InspectorScrollArea>
        <h2>Sequence</h2>
        <div className="mark-section">
          <h3>Marks</h3>
          <button type="button" className="neutral-button" onClick={createCollection}>Add collection</button>
          {gui.document.markCollections.length > 0 && (
            <>
              <label>
                Active
                <select
                  value={activeCollection?.key ?? ""}
                  onChange={(event) => {
                    setActiveMarkCollectionKey(event.currentTarget.value || null);
                  }}
                >
                  {gui.document.markCollections.map((collection) => (
                    <option key={collection.key} value={collection.key}>{collection.name}</option>
                  ))}
                </select>
              </label>
              {activeCollection !== null && (
                <>
                  <label>
                    Name
                    <input
                      key={`${activeCollection.key}:name`}
                      defaultValue={activeCollection.name}
                      onBlur={(event) => {
                        const name = event.currentTarget.value.trim() || activeCollection.name;
                        if (name === activeCollection.name) return;
                        void runSnapshotCommand(() =>
                          commands.applySequenceGuiEdit({ type: "renameMarkCollection", key: activeCollection.key, name })
                        );
                      }}
                    />
                  </label>
                  <ColorField
                    key={`${activeCollection.key}:color:${activeCollection.color.toLowerCase()}`}
                    label="Color"
                    value={activeCollection.color}
                    commit={(color) =>
                      runSnapshotCommand(() =>
                        commands.applySequenceGuiEdit({
                          type: "setMarkCollectionColor",
                          key: activeCollection.key,
                          color
                        })
                      ).then(() => undefined)
                    }
                  />
                </>
              )}
              <div className="mark-visibility-list">
                {gui.document.markCollections.map((collection) => (
                  <label key={collection.key} className="mark-collection-row">
                    <span className="color-swatch" style={{ background: collection.color }} />
                    <span>{collection.name}</span>
                    <input
                      type="checkbox"
                      checked={visibleMarkCollectionKeys.has(collection.key)}
                      onChange={(event) => {
                        const next = new Set(visibleMarkCollectionKeys);
                        if (event.currentTarget.checked) {
                          next.add(collection.key);
                        } else {
                          next.delete(collection.key);
                        }
                        setVisibleMarkCollectionKeys(next);
                      }}
                    />
                  </label>
                ))}
              </div>
              {activeCollection !== null && <button type="button" onClick={deleteActiveCollection}>Delete collection</button>}
            </>
          )}
        </div>
        <p>Select a mark or effect.</p>
      </InspectorScrollArea>
    );
  }
  if (gui.type === "layout") {
    const id = selected !== null && selected.startsWith("placement:") ? Number(selected.split(":")[1]) : null;
    const placement = gui.document.fixtures.find((candidate) => candidate.id === id);
    const transform = placement === undefined ? null : normalizeTransform(placement.transform);
    return (
      <InspectorScrollArea>
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
      </InspectorScrollArea>
    );
  }
  const fixture = gui.document.fixtures.find((candidate) => candidate.objectKey === gui.document.selectedObjectKey) ?? gui.document.fixtures[0];
  return (
    <InspectorScrollArea>
      <h2>Fixture</h2>
      {fixture !== undefined ? (
        <>
          <label>Name<input readOnly value={fixture.name} /></label>
          <label>
            Bulb
            <input
              type="number"
              min={0.001}
              step="any"
              defaultValue={fixture.bulbDiameterMeters}
              onBlur={(event) =>
                void runSnapshotCommand(() =>
                  commands.applyFixtureGuiEdit({
                    type: "updateBulbDiameter",
                    objectKey: fixture.objectKey,
                    bulbDiameterMeters: Number(event.currentTarget.value)
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
    </InspectorScrollArea>
  );
}

function InspectorScrollArea({ children }: { children: ReactNode }) {
  const contentRef = useRef<HTMLDivElement | null>(null);
  const railRef = useRef<HTMLDivElement | null>(null);
  const dragRef = useRef<{ pointerId: number; startY: number; startScrollTop: number } | null>(null);
  const [metrics, setMetrics] = useState({ top: 0, height: 0, scrollable: false });

  const updateMetrics = useCallback(() => {
    const content = contentRef.current;
    if (content === null) return;
    const scrollable = content.scrollHeight > content.clientHeight + 1;
    const railHeight = Math.max(1, content.clientHeight);
    const height = scrollable ? Math.max(28, (content.clientHeight / content.scrollHeight) * railHeight) : railHeight;
    const maxTop = Math.max(0, railHeight - height);
    const top = scrollable ? (content.scrollTop / Math.max(1, content.scrollHeight - content.clientHeight)) * maxTop : 0;
    setMetrics({ top, height, scrollable });
  }, []);

  useEffect(() => {
    const content = contentRef.current;
    if (content === null) return;
    updateMetrics();
    const resizeObserver = new ResizeObserver(updateMetrics);
    resizeObserver.observe(content);
    const mutationObserver = new MutationObserver(updateMetrics);
    mutationObserver.observe(content, { childList: true, subtree: true, characterData: true });
    content.addEventListener("scroll", updateMetrics, { passive: true });
    return () => {
      resizeObserver.disconnect();
      mutationObserver.disconnect();
      content.removeEventListener("scroll", updateMetrics);
    };
  }, [updateMetrics]);

  const scrollToPointer = useCallback((clientY: number) => {
    const content = contentRef.current;
    const rail = railRef.current;
    if (content === null || rail === null || !metrics.scrollable) return;
    const railRect = rail.getBoundingClientRect();
    const maxTop = Math.max(1, railRect.height - metrics.height);
    const top = clamp(clientY - railRect.top - metrics.height / 2, 0, maxTop);
    content.scrollTop = (top / maxTop) * Math.max(1, content.scrollHeight - content.clientHeight);
  }, [metrics.height, metrics.scrollable]);

  return (
    <aside className="gui-inspector-shell">
      <div ref={contentRef} className="gui-inspector">
        <div onKeyDownCapture={commitInspectorFieldOnEnter}>{children}</div>
      </div>
      <div className="editor-scrollbar" aria-hidden={!metrics.scrollable}>
        <div
          ref={railRef}
          className="editor-scrollbar-rail"
          onPointerDown={(event) => {
            if (!metrics.scrollable) return;
            event.currentTarget.setPointerCapture(event.pointerId);
            scrollToPointer(event.clientY);
          }}
        >
          <div
            className={`editor-scrollbar-thumb ${metrics.scrollable ? "" : "disabled"}`}
            style={{ top: `${metrics.top}px`, height: `${metrics.height}px` }}
            onPointerDown={(event) => {
              if (!metrics.scrollable) return;
              event.stopPropagation();
              event.currentTarget.setPointerCapture(event.pointerId);
              dragRef.current = {
                pointerId: event.pointerId,
                startY: event.clientY,
                startScrollTop: contentRef.current?.scrollTop ?? 0
              };
            }}
            onPointerMove={(event) => {
              const drag = dragRef.current;
              const content = contentRef.current;
              const rail = railRef.current;
              if (drag === null || content === null || rail === null || drag.pointerId !== event.pointerId) return;
              const maxTop = Math.max(1, rail.clientHeight - metrics.height);
              const scrollMax = Math.max(1, content.scrollHeight - content.clientHeight);
              content.scrollTop = drag.startScrollTop + ((event.clientY - drag.startY) / maxTop) * scrollMax;
            }}
            onPointerUp={(event) => {
              if (dragRef.current?.pointerId === event.pointerId) {
                dragRef.current = null;
              }
            }}
            onPointerCancel={(event) => {
              if (dragRef.current?.pointerId === event.pointerId) {
                dragRef.current = null;
              }
            }}
          />
        </div>
      </div>
    </aside>
  );
}

function Readout({ label, value, swatch }: { label: string; value: string | number; swatch?: string }) {
  return (
    <div className="inspector-readout">
      <span>{label}</span>
      <strong>
        {swatch !== undefined && <i style={{ background: swatch }} />}
        {value}
      </strong>
    </div>
  );
}

function commitInspectorFieldOnEnter(event: KeyboardEvent<HTMLDivElement>) {
  if (event.key !== "Enter") return;
  const target = event.target;
  if (!(target instanceof HTMLInputElement || target instanceof HTMLSelectElement || target instanceof HTMLTextAreaElement)) return;
  event.preventDefault();
  event.stopPropagation();
  target.blur();
}

function selectedEffectScriptPath(effect: SequenceEffectDto, scripts: SequenceEffectScriptDto[]) {
  const currentName = effect.script.includes(".") ? effect.script.split(".").pop() ?? effect.script : effect.script;
  return scripts.find((script) => script.name === currentName)?.path ?? "";
}

function openColorPicker(input: HTMLInputElement | null | undefined) {
  if (input === null || input === undefined) return;
  input.showPicker();
}

function EffectParamInput({
  effectId,
  param,
  markCollections
}: {
  effectId: number;
  param: SequenceEffectParamDto;
  markCollections: SequenceMarkCollectionDto[];
}) {
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
    return <Readout label={param.name} value="Unavailable" />;
  }

  switch (param.value.type) {
    case "int":
      return <NumberParam key={`${param.name}:${param.value.value}`} param={param} value={param.value.value} step={1} commit={(value) => commit({ type: "int", value: Math.max(0, Math.round(value)) })} />;
    case "float":
      return <NumberParam key={`${param.name}:${param.value.value}`} param={param} value={param.value.value} step={0.05} commit={(value) => commit({ type: "float", value })} />;
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
      return <ColorField key={`${param.name}:${param.value.value.toLowerCase()}`} label={param.name} value={param.value.value} commit={(value) => commit({ type: "color", value })} />;
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
    case "marks":
      return (
        <label>
          {param.name}
          <select value={param.value.key} onChange={(event) => void commit({ type: "marks", key: event.currentTarget.value })}>
            {markCollections.map((collection) => (
              <option key={collection.key} value={collection.key}>{collection.name}</option>
            ))}
          </select>
        </label>
      );
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

function ColorField({ label, value, commit }: { label: string; value: string; commit: (value: string) => Promise<void> }) {
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
  const displayedColor = isHexColor(draft) ? draft : committedValue;
  return (
    <label>
      {label}
      <div className="effect-param-color">
        <span className="color-swatch" style={{ background: displayedColor }} />
        <input
          type="color"
          value={displayedColor}
          onChange={(event) => {
            setDraft(event.currentTarget.value);
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
  return <FloatCurveParam {...props} />;
}

function ColorCurveParamShell(props: {
  name: string;
  points: EditedColorCurvePoint[];
  commit: (points: EditedColorCurvePoint[]) => Promise<void>;
}) {
  return <ColorCurveParam {...props} />;
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
  const draftsRef = useRef(points);
  const [selectedIndex, setSelectedIndex] = useState(0);
  const [pointsCollapsed, setPointsCollapsed] = useState(false);
  const svgRef = useRef<SVGSVGElement | null>(null);
  const draggingPoint = useRef<number | null>(null);
  const pointsSignature = curvePointsSignature(points);
  const lastCommittedSignature = useRef(pointsSignature);
  const pendingSignature = useRef<string | null>(null);
  useEffect(() => {
    draftsRef.current = drafts;
  }, [drafts]);
  useEffect(() => {
    const draftSignature = curvePointsSignature(draftsRef.current);
    if (pointsSignature === draftSignature) {
      pendingSignature.current = null;
      return;
    }
    if (pendingSignature.current === draftSignature) return;
    setDrafts(points);
    draftsRef.current = points;
    lastCommittedSignature.current = pointsSignature;
    setSelectedIndex((index) => Math.min(index, points.length - 1));
  }, [points, pointsSignature]);
  const update = (next: EditedFloatCurvePoint[]) => {
    if (next.length > 0 && next.every((point) => Number.isFinite(point.time) && Number.isFinite(point.value))) {
      const sorted = sortCurvePoints(next);
      const signature = curvePointsSignature(sorted);
      setDrafts(sorted);
      draftsRef.current = sorted;
      setSelectedIndex((index) => Math.min(index, sorted.length - 1));
      if (signature !== lastCommittedSignature.current) {
        lastCommittedSignature.current = signature;
        pendingSignature.current = signature;
        void commit(sorted).catch(() => {
          pendingSignature.current = null;
        });
      }
    }
  };
  const setPoint = (index: number, point: EditedFloatCurvePoint, commitChange: boolean) => {
    const next = sortCurvePoints(replaceAt(draftsRef.current, index, point));
    const nextIndex = nearestFloatPointIndex(next, point);
    setDrafts(next);
    draftsRef.current = next;
    setSelectedIndex(nextIndex);
    if (commitChange) {
      update(next);
    }
    return nextIndex;
  };
  const deletePoint = (index: number) => {
    if (draftsRef.current.length <= 1) return;
    const next = draftsRef.current.filter((_, pointIndex) => pointIndex !== index);
    update(next);
    setSelectedIndex(Math.min(index, next.length - 1));
  };
  const commitDraftPoint = (index: number) => {
    const point = draftsRef.current[index];
    if (!point) return;
    update(replaceAt(draftsRef.current, index, { time: clamp(point.time, 0, 1), value: point.value }));
  };
  const valueRange = floatCurveValueRange(drafts);
  const path = floatCurveSvgPath(drafts, valueRange);
  const pointFromPointer = (event: PointerEvent<SVGSVGElement>): EditedFloatCurvePoint => {
    const rect = svgRef.current?.getBoundingClientRect();
    if (rect === undefined) return { time: 0, value: 0 };
    const x = clamp((event.clientX - rect.left) / Math.max(1, rect.width), 0, 1);
    const y = clamp((event.clientY - rect.top) / Math.max(1, rect.height), 0, 1);
    return {
      time: roundCurveValue(x),
      value: roundCurveValue(valueRange.max - y * (valueRange.max - valueRange.min))
    };
  };
  return (
    <div className="effect-param-group float-curve-editor">
      <div className="effect-param-name">{name}</div>
      <svg
        ref={svgRef}
        className="float-curve-graph"
        viewBox="0 0 240 120"
        role="img"
        aria-label={`${name} curve`}
        onPointerDown={(event) => {
          if (event.target instanceof SVGCircleElement) return;
          const point = pointFromPointer(event);
          update([...draftsRef.current, point]);
          setSelectedIndex(nearestFloatPointIndex(draftsRef.current, point));
        }}
        onPointerMove={(event) => {
          const index = draggingPoint.current;
          if (index === null) return;
          draggingPoint.current = setPoint(index, pointFromPointer(event), false);
        }}
        onPointerUp={(event) => {
          if (draggingPoint.current === null) return;
          event.currentTarget.releasePointerCapture(event.pointerId);
          draggingPoint.current = null;
          update(draftsRef.current);
        }}
        onPointerCancel={(event) => {
          if (draggingPoint.current === null) return;
          event.currentTarget.releasePointerCapture(event.pointerId);
          draggingPoint.current = null;
          update(draftsRef.current);
        }}
      >
        <rect className="float-curve-graph-bg" x="0" y="0" width="240" height="120" />
        <path className="float-curve-grid-line" d="M0 60H240" />
        <path className="float-curve-grid-line" d="M120 0V120" />
        <path className="float-curve-line" d={path} />
        {drafts.map((point, index) => {
          const x = point.time * 240;
          const y = 120 - ((point.value - valueRange.min) / (valueRange.max - valueRange.min)) * 120;
          return (
            <circle
              key={index}
              className={`float-curve-point ${index === selectedIndex ? "selected" : ""}`}
              cx={x}
              cy={y}
              r={index === selectedIndex ? 5 : 4}
              tabIndex={0}
              onPointerDown={(event) => {
                event.stopPropagation();
                event.currentTarget.ownerSVGElement?.setPointerCapture(event.pointerId);
                draggingPoint.current = index;
                setSelectedIndex(index);
              }}
              onContextMenu={(event) => {
                event.preventDefault();
                event.stopPropagation();
                deletePoint(index);
              }}
              onFocus={() => { setSelectedIndex(index); }}
            />
          );
        })}
      </svg>
      <div className="float-curve-points-panel">
        <button
          type="button"
          className="float-curve-points-toggle"
          onClick={() => { setPointsCollapsed((collapsed) => !collapsed); }}
        >
          {pointsCollapsed ? <ChevronRight size={13} /> : <ChevronRight className="expanded" size={13} />}
          <span>Points</span>
          <strong>{drafts.length}</strong>
        </button>
        {!pointsCollapsed && (
          <div className="float-curve-point-list">
            {drafts.map((point, index) => (
              <div
                key={`${index}:${point.time}:${point.value}`}
                className={`float-curve-point-row ${index === selectedIndex ? "selected" : ""}`}
                onPointerDown={() => { setSelectedIndex(index); }}
              >
                <label>
                  <span>t</span>
                  <input
                    type="number"
                    min={0}
                    max={1}
                    step={0.01}
                    value={point.time}
                    onFocus={() => { setSelectedIndex(index); }}
                    onChange={(event) => {
                      setPoint(index, { ...point, time: Number(event.currentTarget.value) }, false);
                    }}
                    onBlur={() => { commitDraftPoint(index); }}
                    onKeyDown={(event) => {
                      if (event.key === "Enter") {
                        commitDraftPoint(index);
                        event.currentTarget.blur();
                      }
                    }}
                  />
                </label>
                <label>
                  <span>v</span>
                  <input
                    type="number"
                    step={0.05}
                    value={point.value}
                    onFocus={() => { setSelectedIndex(index); }}
                    onChange={(event) => {
                      setPoint(index, { ...point, value: Number(event.currentTarget.value) }, false);
                    }}
                    onBlur={() => { commitDraftPoint(index); }}
                    onKeyDown={(event) => {
                      if (event.key === "Enter") {
                        commitDraftPoint(index);
                        event.currentTarget.blur();
                      }
                    }}
                  />
                </label>
                <button
                  type="button"
                  className="float-curve-point-delete"
                  title="Delete point"
                  disabled={drafts.length <= 1}
                  onClick={() => { deletePoint(index); }}
                >
                  <Minus size={14} />
                </button>
              </div>
            ))}
          </div>
        )}
      </div>
      <button type="button" onClick={() => {
        const nextPoint = { time: 1, value: drafts[drafts.length - 1]?.value ?? 0 };
        update([...drafts, nextPoint]);
        setSelectedIndex(drafts.length);
      }}>Add point</button>
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
  const draftsRef = useRef(points);
  const [selectedIndex, setSelectedIndex] = useState(0);
  const [pointsCollapsed, setPointsCollapsed] = useState(false);
  const gradientRef = useRef<HTMLDivElement | null>(null);
  const colorInputRefs = useRef<Array<HTMLInputElement | null>>([]);
  const draggingPoint = useRef<{ index: number; moved: boolean } | null>(null);
  const lastCommittedValues = useRef(points.map((point) => point.value.toLowerCase()));
  const pointsSignature = curvePointsSignature(points);
  const pendingSignature = useRef<string | null>(null);
  useEffect(() => {
    draftsRef.current = drafts;
  }, [drafts]);
  useEffect(() => {
    const draftSignature = curvePointsSignature(draftsRef.current);
    if (pointsSignature === draftSignature) {
      pendingSignature.current = null;
      return;
    }
    if (pendingSignature.current === draftSignature) return;
    setDrafts(points);
    draftsRef.current = points;
    lastCommittedValues.current = points.map((point) => point.value.toLowerCase());
    setSelectedIndex((index) => Math.min(index, points.length - 1));
  }, [points, pointsSignature]);
  const update = (next: EditedColorCurvePoint[]) => {
    if (next.length > 0 && next.every((point) => Number.isFinite(point.time) && isHexColor(point.value))) {
      const sorted = sortCurvePoints(next).map((point) => ({ ...point, value: point.value.toLowerCase() }));
      const signature = curvePointsSignature(sorted);
      setDrafts(sorted);
      draftsRef.current = sorted;
      lastCommittedValues.current = sorted.map((point) => point.value);
      pendingSignature.current = signature;
      void commit(sorted).catch(() => {
        pendingSignature.current = null;
      });
    }
  };
  const setPoint = (index: number, point: EditedColorCurvePoint, commitChange: boolean) => {
    const next = sortCurvePoints(replaceAt(draftsRef.current, index, point));
    const nextIndex = nearestColorPointIndex(next, point);
    setDrafts(next);
    draftsRef.current = next;
    setSelectedIndex(nextIndex);
    if (commitChange) {
      update(next);
    }
    return nextIndex;
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
  const commitDraftPoint = (index: number) => {
    const point = draftsRef.current[index];
    if (!point) return;
    if (!isHexColor(point.value)) {
      commitDraftValue(index);
      return;
    }
    update(replaceAt(draftsRef.current, index, { time: clamp(point.time, 0, 1), value: point.value.toLowerCase() }));
  };
  const deletePoint = (index: number) => {
    if (draftsRef.current.length <= 1) return;
    const next = draftsRef.current.filter((_, pointIndex) => pointIndex !== index);
    update(next);
    setSelectedIndex(Math.min(index, next.length - 1));
  };
  const pointFromPointer = (event: PointerEvent<HTMLElement>, color: string): EditedColorCurvePoint => {
    const rect = gradientRef.current?.getBoundingClientRect();
    if (rect === undefined) return { time: 0, value: color };
    return {
      time: roundCurveValue(clamp((event.clientX - rect.left) / Math.max(1, rect.width), 0, 1)),
      value: color
    };
  };
  const gradient = colorCurveGradient(drafts);
  return (
    <div className="effect-param-group color-curve-editor">
      <div className="effect-param-name">{name}</div>
      <div
        ref={gradientRef}
        className="color-curve-gradient"
        style={{ background: gradient }}
        onPointerDown={(event) => {
          if (event.target !== event.currentTarget) return;
          const previous = draftsRef.current[draftsRef.current.length - 1]?.value ?? "#ffffff";
          const point = pointFromPointer(event, previous);
          update([...draftsRef.current, point]);
          setSelectedIndex(nearestColorPointIndex(draftsRef.current, point));
        }}
        onPointerMove={(event) => {
          const drag = draggingPoint.current;
          if (drag === null) return;
          const point = draftsRef.current[drag.index];
          if (point === undefined) return;
          const nextIndex = setPoint(drag.index, pointFromPointer(event, point.value), false);
          draggingPoint.current = { index: nextIndex, moved: true };
        }}
        onPointerUp={(event) => {
          if (draggingPoint.current === null) return;
          event.currentTarget.releasePointerCapture(event.pointerId);
          const moved = draggingPoint.current.moved;
          draggingPoint.current = null;
          if (moved) {
            update(draftsRef.current);
          }
        }}
        onPointerCancel={(event) => {
          if (draggingPoint.current === null) return;
          event.currentTarget.releasePointerCapture(event.pointerId);
          const moved = draggingPoint.current.moved;
          draggingPoint.current = null;
          if (moved) {
            update(draftsRef.current);
          }
        }}
      >
        {drafts.map((point, index) => {
          const displayedColor = isHexColor(point.value) ? point.value : (points[index]?.value ?? "#ffffff");
          return (
            <span
              key={index}
              className={`color-curve-stop ${index === selectedIndex ? "selected" : ""}`}
              style={{ left: `${point.time * 100}%` }}
              onPointerDown={(event) => {
                event.stopPropagation();
                event.currentTarget.parentElement?.setPointerCapture(event.pointerId);
                draggingPoint.current = { index, moved: false };
                setSelectedIndex(index);
              }}
              onDoubleClick={(event) => {
                event.preventDefault();
                event.stopPropagation();
                openColorPicker(colorInputRefs.current[index]);
              }}
              onContextMenu={(event) => {
                event.preventDefault();
                event.stopPropagation();
                deletePoint(index);
              }}
              onFocus={() => { setSelectedIndex(index); }}
            >
              <span className="color-curve-stop-line" />
              <label className="color-curve-stop-picker" title={`Gradient stop ${index + 1}`}>
                <input
                  ref={(input) => {
                    colorInputRefs.current[index] = input;
                  }}
                  type="color"
                  value={displayedColor}
                  onChange={(event) => {
                    setPoint(index, { ...point, value: event.currentTarget.value }, false);
                  }}
                  onBlur={() => { commitDraftValue(index); }}
                />
              </label>
            </span>
          );
        })}
      </div>
      <div className="float-curve-points-panel">
        <button
          type="button"
          className="float-curve-points-toggle"
          onClick={() => { setPointsCollapsed((collapsed) => !collapsed); }}
        >
          {pointsCollapsed ? <ChevronRight size={13} /> : <ChevronRight className="expanded" size={13} />}
          <span>Stops</span>
          <strong>{drafts.length}</strong>
        </button>
        {!pointsCollapsed && (
          <div className="float-curve-point-list">
            {drafts.map((point, index) => {
              const displayedColor = isHexColor(point.value) ? point.value : (points[index]?.value ?? "#ffffff");
              return (
                <div
                  key={index}
                  className={`color-curve-point-row-compact ${index === selectedIndex ? "selected" : ""}`}
                  onPointerDown={() => { setSelectedIndex(index); }}
                >
                  <label>
                    <span>t</span>
                    <input
                      type="number"
                      min={0}
                      max={1}
                      step={0.01}
                      value={point.time}
                      onFocus={() => { setSelectedIndex(index); }}
                      onChange={(event) => {
                        setPoint(index, { ...point, time: Number(event.currentTarget.value) }, false);
                      }}
                      onBlur={() => { commitDraftPoint(index); }}
                      onKeyDown={(event) => {
                        if (event.key === "Enter") {
                          commitDraftPoint(index);
                          event.currentTarget.blur();
                        }
                      }}
                    />
                  </label>
                  <label className="color-swatch-picker">
                    <span className="color-swatch" style={{ background: displayedColor }} />
                    <input
                      type="color"
                      value={displayedColor}
                      onFocus={() => { setSelectedIndex(index); }}
                      onChange={(event) => {
                        setPoint(index, { ...point, value: event.currentTarget.value }, false);
                      }}
                      onBlur={() => { commitDraftValue(index); }}
                    />
                  </label>
                  <input
                    value={point.value}
                    onFocus={() => { setSelectedIndex(index); }}
                    onChange={(event) => { setPoint(index, { ...point, value: event.currentTarget.value }, false); }}
                    onBlur={() => { commitDraftValue(index); }}
                    onKeyDown={(event) => {
                      if (event.key === "Enter") {
                        commitDraftValue(index);
                        event.currentTarget.blur();
                      }
                    }}
                  />
                  <button type="button" className="float-curve-point-delete" disabled={drafts.length <= 1} onClick={() => { deletePoint(index); }}>
                    <Minus size={14} />
                  </button>
                </div>
              );
            })}
          </div>
        )}
      </div>
      <button type="button" onClick={() => {
        const nextPoint = { time: 1, value: drafts[drafts.length - 1]?.value ?? "#ffffff" };
        update([...drafts, nextPoint]);
        setSelectedIndex(drafts.length);
      }}>Add stop</button>
    </div>
  );
}

type SequenceViewport = {
  pxPerSecond: number;
  laneHeight: number;
  scrollXSeconds: number;
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

type SequenceMarkHit = {
  collectionKey: string;
  index: number;
  timeSeconds: number;
};

type SequencePreviewImage = {
  signature: string;
} & ({ status: "ready"; canvas: HTMLCanvasElement } | { status: "unavailable" });

type WaveformAudio = { durationSeconds: number; sampleRate: number; levels: WaveformLevel[] };
type WaveformLevel = { samplesPerPeak: number; mins: Float32Array; maxes: Float32Array };
type WaveformState = { key: string | null; audio: WaveformAudio | null };

const waveformCache = new Map<string, Promise<WaveformAudio | null>>();

function useSequenceWaveform(audio: SequenceAudioDto | null): WaveformState {
  const key = audio?.exists === true ? audio.resolvedPath : null;
  const [state, setState] = useState<WaveformState>({ key, audio: null });

  useEffect(() => {
    if (key === null) return;
    let cancelled = false;
    let request = waveformCache.get(key);
    if (request === undefined) {
      request = decodeWaveformPeaks(key);
      waveformCache.set(key, request);
    }
    void request.then((waveformAudio) => {
      if (!cancelled) {
        setState({ key, audio: waveformAudio });
      }
    });
    return () => {
      cancelled = true;
    };
  }, [key]);

  return state.key === key ? state : { key, audio: null };
}

async function decodeWaveformPeaks(path: string): Promise<WaveformAudio | null> {
  try {
    const response = await fetch(convertFileSrc(path));
    if (!response.ok) return null;
    const arrayBuffer = await response.arrayBuffer();
    const context = new AudioContext();
    try {
      const audioBuffer = await context.decodeAudioData(arrayBuffer);
      const waveform = buildWaveformAudio(audioBuffer);
      await context.close();
      return waveform;
    } catch {
      await context.close();
      return null;
    }
  } catch {
    return null;
  }
}

function buildWaveformAudio(buffer: AudioBuffer): WaveformAudio {
  const baseSamplesPerPeak = 32;
  const channels = Array.from({ length: buffer.numberOfChannels }, (_, index) => buffer.getChannelData(index));
  const bucketCount = Math.max(1, Math.ceil(buffer.length / baseSamplesPerPeak));
  const mins = new Float32Array(bucketCount);
  const maxes = new Float32Array(bucketCount);
  for (let bucket = 0; bucket < bucketCount; bucket += 1) {
    const start = bucket * baseSamplesPerPeak;
    const end = Math.min(buffer.length, start + baseSamplesPerPeak);
    let min = 0;
    let max = 0;
    for (const channel of channels) {
      for (let index = start; index < end; index += 1) {
        const sample = channel[index] ?? 0;
        min = Math.min(min, sample);
        max = Math.max(max, sample);
      }
    }
    mins[bucket] = min;
    maxes[bucket] = max;
  }

  const levels: WaveformLevel[] = [{ samplesPerPeak: baseSamplesPerPeak, mins, maxes }];
  while ((levels[levels.length - 1]?.mins.length ?? 0) > 1) {
    const previous = levels[levels.length - 1];
    if (previous === undefined) break;
    levels.push(coarsenWaveformLevel(previous));
  }
  return { durationSeconds: buffer.duration, sampleRate: buffer.sampleRate, levels };
}

function coarsenWaveformLevel(level: WaveformLevel): WaveformLevel {
  const bucketCount = Math.ceil(level.mins.length / 2);
  const mins = new Float32Array(bucketCount);
  const maxes = new Float32Array(bucketCount);
  for (let bucket = 0; bucket < bucketCount; bucket += 1) {
    const left = bucket * 2;
    const right = left + 1;
    mins[bucket] = Math.min(level.mins[left] ?? 0, level.mins[right] ?? level.mins[left] ?? 0);
    maxes[bucket] = Math.max(level.maxes[left] ?? 0, level.maxes[right] ?? level.maxes[left] ?? 0);
  }
  return {
    samplesPerPeak: level.samplesPerPeak * 2,
    mins,
    maxes
  };
}

function buildSequenceClipLayout(
  document: SequenceDocumentDto,
  previews: SequencePreview[],
  viewport: SequenceViewport,
  left: number,
  top: number
): SequenceClipLayout[] {
  const clips = document.effects.map((effect): SequenceClip => {
    const activePreview = previews.find((preview) => preview.id === effect.id) ?? null;
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
        startSeconds: activePreview.startSeconds,
        durationSeconds: activePreview.durationSeconds,
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
        const startSeconds = clip.effect.startSeconds;
        const endSeconds = startSeconds + clip.effect.durationSeconds;
        const x = left + (startSeconds - viewport.scrollXSeconds) * viewport.pxPerSecond;
        const width = Math.max(12, (endSeconds - startSeconds) * viewport.pxPerSecond);
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
    const start = clip.effect.startSeconds;
    const end = clip.effect.startSeconds + clip.effect.durationSeconds;
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
    const start = clip.effect.startSeconds;
    const end = clip.effect.startSeconds + clip.effect.durationSeconds;
    let slot = slotEnds.findIndex((slotEnd) => slotEnd <= start);
    if (slot === -1) slot = slotEnds.length;
    slotEnds[slot] = end;
    return { ...clip, slot };
  });
}

function compareClipsByTime(left: { effect: SequenceEffectDto }, right: { effect: SequenceEffectDto }) {
  return (
    left.effect.startSeconds - right.effect.startSeconds ||
    left.effect.startSeconds + left.effect.durationSeconds - (right.effect.startSeconds + right.effect.durationSeconds) ||
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

function hitSequenceMark(
  collections: SequenceMarkCollectionDto[],
  mode: MarkDisplayMode,
  x: number,
  y: number,
  left: number,
  audioStripTop: number,
  audioStripHeight: number,
  canvasHeight: number,
  viewport: SequenceViewport
): SequenceMarkHit | null {
  if (mode === "hidden" || x < left) return null;
  if (mode === "strip" && (y < audioStripTop || y > audioStripTop + audioStripHeight)) return null;
  if (mode === "overlay" && (y < audioStripTop || y > canvasHeight)) return null;
  for (const collection of [...collections].reverse()) {
    for (let index = collection.marksSeconds.length - 1; index >= 0; index -= 1) {
      const timeSeconds = collection.marksSeconds[index] ?? 0;
      const markX = left + (timeSeconds - viewport.scrollXSeconds) * viewport.pxPerSecond;
      if (Math.abs(x - markX) <= 5) {
        return { collectionKey: collection.key, index, timeSeconds };
      }
    }
  }
  return null;
}

function sequenceHoverEqual(left: SequenceHover, right: SequenceHover) {
  if (left === right) return true;
  if (left === null || right === null || left.kind !== right.kind) return false;
  if (left.kind === "effect" && right.kind === "effect") {
    return left.effectId === right.effectId && left.resize === right.resize;
  }
  if (left.kind !== "mark" || right.kind !== "mark") return false;
  return left.collectionKey === right.collectionKey && left.index === right.index;
}

function drawSequenceMarks(
  ctx: CanvasRenderingContext2D,
  collections: SequenceMarkCollectionDto[],
  selected: string | null,
  selectedMarkKeys: Set<string>,
  mode: MarkDisplayMode,
  left: number,
  audioStripTop: number,
  audioStripHeight: number,
  width: number,
  height: number,
  pxPerSecond: number,
  scrollXSeconds: number,
  previews: MarkPreviewLookup
) {
  if (mode === "hidden") return;
  const y1 = audioStripTop;
  const y2 = mode === "strip" ? audioStripTop + audioStripHeight : height;
  ctx.save();
  ctx.beginPath();
  ctx.rect(left, y1, width, y2 - y1);
  ctx.clip();
  for (const collection of collections) {
    for (const [index, timeSeconds] of collection.marksSeconds.entries()) {
      const preview = previews.get(markKey({ collectionKey: collection.key, index }));
      const drawnTimeSeconds = preview?.timeSeconds ?? timeSeconds;
      const x = left + (drawnTimeSeconds - scrollXSeconds) * pxPerSecond;
      if (x < left - 6 || x > left + width + 6) continue;
      const isSelected = selected === `mark:${collection.key}:${index}` || selectedMarkKeys.has(markKey({ collectionKey: collection.key, index }));
      ctx.strokeStyle = collection.color;
      ctx.lineWidth = isSelected ? 2 : 1;
      ctx.globalAlpha = mode === "strip" ? 0.95 : 0.75;
      ctx.beginPath();
      ctx.moveTo(x + 0.5, y1);
      ctx.lineTo(x + 0.5, y2);
      ctx.stroke();
      if (isSelected) {
        ctx.globalAlpha = 1;
        ctx.strokeStyle = "#fffaf0";
        ctx.lineWidth = 1;
        ctx.beginPath();
        ctx.moveTo(x - 4, y1 + 0.5);
        ctx.lineTo(x + 4, y1 + 0.5);
        ctx.stroke();
      }
    }
  }
  ctx.restore();
}

function committedMarkPreviews(collections: SequenceMarkCollectionDto[], previews: MarkPreviewLookup) {
  const next = new Map<string, MarkPreview>();
  for (const [key, preview] of previews) {
    if (preview.committedIndex === undefined) {
      next.set(key, preview);
      continue;
    }
    const collection = collections.find((candidate) => candidate.key === preview.collectionKey);
    if (collection?.marksSeconds[preview.committedIndex] !== preview.timeSeconds) {
      next.set(key, preview);
    }
  }
  return next;
}

function parseSelectedMark(selected: string | null): { collectionKey: string; index: number } | null {
  if (selected === null || !selected.startsWith("mark:")) return null;
  const [, collectionKey, rawIndex] = selected.split(":");
  const index = Number(rawIndex);
  if (collectionKey === undefined || collectionKey.length === 0 || !Number.isInteger(index) || index < 0) return null;
  return { collectionKey, index };
}

function parseSelectedEffectId(selected: string | null): number | null {
  if (selected === null || !selected.startsWith("effect:")) return null;
  const id = Number(selected.split(":")[1]);
  return Number.isInteger(id) ? id : null;
}

function markKey(mark: SequenceMarkRefDto) {
  return `${mark.collectionKey}:${mark.index}`;
}

function selectionFromSingle(selected: string | null): SequenceSelection {
  const effectId = parseSelectedEffectId(selected);
  if (effectId !== null) return { type: "effects", ids: [effectId] };
  const mark = parseSelectedMark(selected);
  if (mark !== null) return { type: "marks", marks: [mark] };
  return null;
}

function singleSelectionString(selection: SequenceSelection): string | null {
  if (selection?.type === "effects" && selection.ids.length === 1) return `effect:${selection.ids[0]}`;
  if (selection?.type === "marks" && selection.marks.length === 1) {
    const mark = selection.marks[0];
    return mark === undefined ? null : `mark:${mark.collectionKey}:${mark.index}`;
  }
  return null;
}

function selectionCount(selection: SequenceSelectionDto) {
  return selection.type === "effects" ? selection.ids.length : selection.marks.length;
}

function selectionCompatibleWithFocusedItem(selection: SequenceSelectionDto, selected: string | null) {
  const effectId = parseSelectedEffectId(selected);
  if (effectId !== null) return selection.type === "effects" && selection.ids.includes(effectId);
  const mark = parseSelectedMark(selected);
  if (mark !== null) return selection.type === "marks" && selection.marks.some((candidate) => markKey(candidate) === markKey(mark));
  return true;
}

function nextEffectSelection(current: SequenceSelection, id: number, shift: boolean, ctrl: boolean): SequenceSelectionDto {
  if (current?.type !== "effects" || (!shift && !ctrl)) return { type: "effects", ids: [id] };
  const ids = new Set(current.ids);
  if (ctrl && ids.has(id)) ids.delete(id);
  else ids.add(id);
  return { type: "effects", ids: [...ids] };
}

function nextMarkSelection(current: SequenceSelection, mark: SequenceMarkRefDto, shift: boolean, ctrl: boolean): SequenceSelectionDto {
  if (current?.type !== "marks" || (!shift && !ctrl)) return { type: "marks", marks: [mark] };
  const byKey = new Map(current.marks.map((candidate) => [markKey(candidate), candidate]));
  const key = markKey(mark);
  if (ctrl && byKey.has(key)) byKey.delete(key);
  else byKey.set(key, mark);
  return { type: "marks", marks: [...byKey.values()] };
}

function mergeSequenceSelection(current: SequenceSelection, next: SequenceSelectionDto, shift: boolean, ctrl: boolean): SequenceSelection {
  if ((!shift && !ctrl) || current?.type !== next.type) return next;
  if (next.type === "effects") {
    const ids = new Set(current.type === "effects" ? current.ids : []);
    for (const id of next.ids) {
      if (ctrl && ids.has(id)) ids.delete(id);
      else ids.add(id);
    }
    return { type: "effects", ids: [...ids] };
  }
  const marks = new Map(current.type === "marks" ? current.marks.map((mark) => [markKey(mark), mark]) : []);
  for (const mark of next.marks) {
    const key = markKey(mark);
    if (ctrl && marks.has(key)) marks.delete(key);
    else marks.set(key, mark);
  }
  return { type: "marks", marks: [...marks.values()] };
}

function normalizedRect(startX: number, startY: number, x: number, y: number) {
  const left = Math.min(startX, x);
  const top = Math.min(startY, y);
  return { x: left, y: top, width: Math.abs(x - startX), height: Math.abs(y - startY) };
}

function rectsIntersect(left: { x: number; y: number; width: number; height: number }, right: { x: number; y: number; width: number; height: number }) {
  return left.x <= right.x + right.width && left.x + left.width >= right.x && left.y <= right.y + right.height && left.y + left.height >= right.y;
}

function selectionFromMarqueeEffects(clips: SequenceClipLayout[], marquee: SequenceMarquee): SequenceSelectionDto {
  const box = normalizedRect(marquee.startX, marquee.startY, marquee.x, marquee.y);
  return { type: "effects", ids: clips.filter((clip) => rectsIntersect(box, clip.rect)).map((clip) => clip.effect.id) };
}

function selectionFromMarqueeMarks(
  collections: SequenceMarkCollectionDto[],
  mode: MarkDisplayMode,
  marquee: SequenceMarquee,
  left: number,
  audioStripTop: number,
  audioStripHeight: number,
  canvasHeight: number,
  viewport: SequenceViewport
): SequenceSelectionDto {
  const box = normalizedRect(marquee.startX, marquee.startY, marquee.x, marquee.y);
  const y1 = mode === "strip" ? audioStripTop : audioStripTop;
  const y2 = mode === "strip" ? audioStripTop + audioStripHeight : canvasHeight;
  const marks: SequenceMarkRefDto[] = [];
  if (mode === "hidden") return { type: "marks", marks };
  for (const collection of collections) {
    collection.marksSeconds.forEach((timeSeconds, index) => {
      const x = left + (timeSeconds - viewport.scrollXSeconds) * viewport.pxPerSecond;
      if (rectsIntersect(box, { x: x - 5, y: y1, width: 10, height: y2 - y1 })) {
        marks.push({ collectionKey: collection.key, index });
      }
    });
  }
  return { type: "marks", marks };
}

function constrainEffectMoveDelta(document: SequenceDocumentDto, ids: number[], deltaSeconds: number) {
  let minDelta = -Infinity;
  let maxDelta = Infinity;
  for (const effect of document.effects.filter((candidate) => ids.includes(candidate.id))) {
    minDelta = Math.max(minDelta, -effect.startSeconds);
    maxDelta = Math.min(maxDelta, document.durationSeconds - effect.durationSeconds - effect.startSeconds);
  }
  return clamp(deltaSeconds, minDelta, maxDelta);
}

function constrainEffectLaneDelta(document: SequenceDocumentDto, ids: number[], laneDelta: number) {
  let minDelta = -Infinity;
  let maxDelta = Infinity;
  for (const effect of document.effects.filter((candidate) => ids.includes(candidate.id))) {
    const laneIndex = document.lanes.findIndex((lane) => targetsEqual(lane.target, effect.target));
    if (laneIndex < 0) continue;
    minDelta = Math.max(minDelta, -laneIndex);
    maxDelta = Math.min(maxDelta, document.lanes.length - 1 - laneIndex);
  }
  return Math.trunc(clamp(laneDelta, minDelta, maxDelta));
}

function effectMovePreviews(document: SequenceDocumentDto, ids: number[], deltaSeconds: number, laneDelta: number): SequencePreview[] {
  return document.effects
    .filter((effect) => ids.includes(effect.id))
    .map((effect) => {
      const laneIndex = document.lanes.findIndex((lane) => targetsEqual(lane.target, effect.target));
      return {
        id: effect.id,
        startSeconds: clamp(effect.startSeconds + deltaSeconds, 0, Math.max(0, document.durationSeconds - effect.durationSeconds)),
        durationSeconds: effect.durationSeconds,
        laneIndex: clamp(laneIndex + laneDelta, 0, Math.max(0, document.lanes.length - 1))
      };
    });
}

function effectResizePreviews(document: SequenceDocumentDto, ids: number[], edge: "left" | "right", deltaSeconds: number): SequencePreview[] {
  return document.effects
    .filter((effect) => ids.includes(effect.id))
    .map((effect) => {
      const laneIndex = Math.max(0, document.lanes.findIndex((lane) => targetsEqual(lane.target, effect.target)));
      if (edge === "left") {
        const endSeconds = effect.startSeconds + effect.durationSeconds;
        const startSeconds = clamp(effect.startSeconds + deltaSeconds, 0, endSeconds - MIN_EFFECT_DURATION_SECONDS);
        return { id: effect.id, startSeconds, durationSeconds: endSeconds - startSeconds, laneIndex };
      }
      return {
        id: effect.id,
        startSeconds: effect.startSeconds,
        durationSeconds: clamp(effect.durationSeconds + deltaSeconds, MIN_EFFECT_DURATION_SECONDS, document.durationSeconds - effect.startSeconds),
        laneIndex
      };
    });
}

function constrainEffectResizeDelta(document: SequenceDocumentDto, ids: number[], edge: "left" | "right", deltaSeconds: number) {
  let minDelta = -Infinity;
  let maxDelta = Infinity;
  for (const effect of document.effects.filter((candidate) => ids.includes(candidate.id))) {
    if (edge === "left") {
      minDelta = Math.max(minDelta, -effect.startSeconds);
      maxDelta = Math.min(maxDelta, effect.durationSeconds - MIN_EFFECT_DURATION_SECONDS);
    } else {
      minDelta = Math.max(minDelta, MIN_EFFECT_DURATION_SECONDS - effect.durationSeconds);
      maxDelta = Math.min(maxDelta, document.durationSeconds - effect.startSeconds - effect.durationSeconds);
    }
  }
  return clamp(deltaSeconds, minDelta, maxDelta);
}

function constrainMarkDelta(document: SequenceDocumentDto, marks: SequenceMarkRefDto[], deltaSeconds: number) {
  let minDelta = -Infinity;
  let maxDelta = Infinity;
  for (const mark of marks) {
    const collection = document.markCollections.find((candidate) => candidate.key === mark.collectionKey);
    const timeSeconds = collection?.marksSeconds[mark.index];
    if (timeSeconds === undefined) continue;
    minDelta = Math.max(minDelta, -timeSeconds);
    maxDelta = Math.min(maxDelta, document.durationSeconds - timeSeconds);
  }
  return clamp(deltaSeconds, minDelta, maxDelta);
}

function markMovePreviews(document: SequenceDocumentDto, marks: SequenceMarkRefDto[], deltaSeconds: number): MarkPreviewLookup {
  const previews: MarkPreviewLookup = new Map();
  for (const mark of marks) {
    const collection = document.markCollections.find((candidate) => candidate.key === mark.collectionKey);
    const timeSeconds = collection?.marksSeconds[mark.index];
    if (collection === undefined || timeSeconds === undefined) continue;
    const nextTimeSeconds = clamp(timeSeconds + deltaSeconds, 0, document.durationSeconds);
    previews.set(markKey(mark), {
      collectionKey: mark.collectionKey,
      index: mark.index,
      timeSeconds: nextTimeSeconds,
      committedIndex: markIndexAfterMove(collection, mark.index, nextTimeSeconds)
    });
  }
  return previews;
}

function markSelectionConsumesKey(selected: string | null, key: string) {
  return parseSelectedMark(selected) !== null && (key === "ArrowLeft" || key === "ArrowRight");
}

function markIndexAfterMove(collection: SequenceMarkCollectionDto, index: number, timeSeconds: number) {
  const sorted = collection.marksSeconds
    .map((markTimeSeconds, markIndex) => ({
      markIndex,
      timeSeconds: markIndex === index ? timeSeconds : markTimeSeconds
    }))
    .sort((left, right) => left.timeSeconds - right.timeSeconds || left.markIndex - right.markIndex);
  return Math.max(0, sorted.findIndex((mark) => mark.markIndex === index));
}

function nextCollectionKey(name: string, collections: SequenceMarkCollectionDto[]) {
  const used = new Set(collections.map((collection) => collection.key));
  const base = snakeCaseKey(name);
  if (!used.has(base)) return base;
  for (let suffix = 2; ; suffix += 1) {
    const key = `${base}_${suffix}`;
    if (!used.has(key)) return key;
  }
}

function defaultMarkColor(index: number) {
  return DEFAULT_MARK_COLORS[index % DEFAULT_MARK_COLORS.length] ?? "#38bdf8";
}

function snakeCaseKey(value: string) {
  const key = value
    .trim()
    .toLowerCase()
    .replace(/[^a-z0-9_]+/g, "_")
    .replace(/_+/g, "_")
    .replace(/^_+|_+$/g, "");
  return /^[a-z]/.test(key) ? key : key.length > 0 ? `marks_${key}` : "marks";
}

function isTextEntryElement(target: EventTarget | null) {
  return target instanceof HTMLInputElement || target instanceof HTMLTextAreaElement || target instanceof HTMLSelectElement;
}

function drawWaveformStrip(
  ctx: CanvasRenderingContext2D,
  audio: WaveformAudio | null,
  left: number,
  top: number,
  width: number,
  height: number,
  durationSeconds: number,
  pxPerSecond: number,
  scrollXSeconds: number
) {
  ctx.save();
  ctx.beginPath();
  ctx.rect(left, top, width, height);
  ctx.clip();
  ctx.strokeStyle = "#24272c";
  ctx.beginPath();
  ctx.moveTo(left, top + height / 2 + 0.5);
  ctx.lineTo(left + width, top + height / 2 + 0.5);
  ctx.stroke();
  if (audio !== null && audio.durationSeconds > 0 && audio.levels.length > 0) {
    const samplesPerSecond = audio.sampleRate;
    const samplesPerPixel = samplesPerSecond / pxPerSecond;
    const level = waveformLevelForZoom(audio.levels, samplesPerPixel);
    const xPerPeak = (level.samplesPerPeak / samplesPerSecond) * pxPerSecond;
    const clipEndSeconds = Math.min(durationSeconds, audio.durationSeconds);
    const visibleStartSeconds = Math.max(0, scrollXSeconds);
    const visibleEndSeconds = Math.min(clipEndSeconds, scrollXSeconds + width / pxPerSecond);
    const firstIndex = Math.max(0, Math.floor((visibleStartSeconds * samplesPerSecond) / level.samplesPerPeak));
    const lastIndex = Math.min(
      level.mins.length - 1,
      Math.ceil((visibleEndSeconds * samplesPerSecond) / level.samplesPerPeak)
    );
    const centerY = top + height / 2;
    const maxAmplitude = Math.max(1, height / 2 - 4);
    ctx.fillStyle = "#6abf8a";
    for (let index = firstIndex; index <= lastIndex; index += 1) {
      const timeSeconds = (index * level.samplesPerPeak) / samplesPerSecond;
      if (timeSeconds > clipEndSeconds) break;
      const x = left + (timeSeconds - scrollXSeconds) * pxPerSecond;
      if (x > left + width) break;
      if (x + xPerPeak < left) continue;
      const min = level.mins[index] ?? 0;
      const max = level.maxes[index] ?? 0;
      const y1 = centerY - max * maxAmplitude;
      const y2 = centerY - min * maxAmplitude;
      ctx.fillRect(x, y1, Math.max(1, xPerPeak), Math.max(1, y2 - y1));
    }
  }
  ctx.restore();
}

function waveformLevelForZoom(levels: WaveformLevel[], samplesPerPixel: number): WaveformLevel {
  return levels.find((level) => level.samplesPerPeak >= samplesPerPixel) ?? levels[levels.length - 1] ?? {
    samplesPerPeak: 1,
    mins: new Float32Array([0]),
    maxes: new Float32Array([0])
  };
}

function drawTimelineGrid(
  ctx: CanvasRenderingContext2D,
  left: number,
  top: number,
  width: number,
  height: number,
  pxPerSecond: number,
  scrollXSeconds: number,
  frameRate: number
) {
  const tick = chooseTimelineTick(pxPerSecond, frameRate);
  const firstMinor = Math.floor(scrollXSeconds / tick.minorSeconds) * tick.minorSeconds;
  ctx.lineWidth = 1;
  for (let time = firstMinor; ; time += tick.minorSeconds) {
    const x = left + (time - scrollXSeconds) * pxPerSecond;
    if (x > width) break;
    if (x < left) continue;
    const labeled = isMultipleOf(time, tick.labelSeconds);
    ctx.strokeStyle = labeled ? "#343941" : "#1f2227";
    ctx.beginPath();
    ctx.moveTo(x + 0.5, labeled ? 0 : top);
    ctx.lineTo(x + 0.5, height);
    ctx.stroke();
    if (labeled) {
      ctx.fillStyle = "#a8a29a";
      ctx.fillText(formatTimelineSeconds(time, tick.labelSeconds), x + 5, 18);
    }
  }
}

function chooseTimelineTick(pxPerSecond: number, frameRate: number) {
  const frameSeconds = 1 / Math.max(1, frameRate);
  const minorCandidates = Array.from(new Set([
    frameSeconds,
    frameSeconds * 2,
    frameSeconds * 5,
    frameSeconds * 10,
    0.05,
    0.1,
    0.25,
    0.5,
    1,
    2.5,
    5,
    10,
    30,
    60
  ])).sort((left, right) => left - right);
  const minorSeconds = minorCandidates.find((candidate) => candidate * pxPerSecond >= 24) ?? 60;
  const labelSeconds = minorCandidates.find((candidate) => candidate >= minorSeconds && candidate * pxPerSecond >= 110) ?? minorSeconds * 5;
  return { minorSeconds, labelSeconds };
}

function isMultipleOf(value: number, interval: number) {
  return Math.abs(value / interval - Math.round(value / interval)) < 0.0001;
}

function formatSeconds(value: number) {
  const totalSeconds = Math.max(0, Math.floor(value));
  const minutes = Math.floor(totalSeconds / 60);
  const seconds = totalSeconds % 60;
  return `${minutes}:${String(seconds).padStart(2, "0")}`;
}

function formatMs(value: number) {
  return `${value.toFixed(1)}ms`;
}

function formatSignedMs(value: number) {
  const sign = value > 0 ? "+" : "";
  return `${sign}${formatMs(value)}`;
}

function formatTimelineSeconds(value: number, intervalSeconds: number) {
  if (intervalSeconds < 1) {
    const totalMilliseconds = Math.max(0, Math.round(value * 1000));
    const minutes = Math.floor(totalMilliseconds / 60000);
    const seconds = Math.floor((totalMilliseconds % 60000) / 1000);
    const milliseconds = totalMilliseconds % 1000;
    return `${minutes}:${String(seconds).padStart(2, "0")}.${String(milliseconds).padStart(3, "0")}`;
  }
  return formatSeconds(value);
}

function isEditableShortcutTarget(target: EventTarget | null) {
  if (!(target instanceof HTMLElement)) return false;
  if (target.isContentEditable) return true;
  if (target.closest(".cm-editor")) return true;
  return target.closest("input, textarea, select") !== null;
}

function handleSequencePlaybackShortcut(
  event: KeyboardEvent<HTMLElement>,
  document: SequenceDocumentDto,
  preview: AppSnapshotDto["preview"],
  unsupported: boolean
) {
  if (unsupported || isEditableShortcutTarget(event.target)) return;
  if (event.key === " ") {
    event.preventDefault();
    event.stopPropagation();
    void runSnapshotCommand(preview.audioPlaybackStatus === "loading_to_play" ? commands.previewPause : preview.isPlaying ? commands.previewStop : commands.previewPlay);
  } else if (event.key.toLowerCase() === "s") {
    event.preventDefault();
    event.stopPropagation();
    void runSnapshotCommand(commands.previewStop);
  } else if (event.key === "Home") {
    event.preventDefault();
    event.stopPropagation();
    void runSnapshotCommand(commands.previewRewindToZero);
  } else if (event.key === "ArrowLeft") {
    event.preventDefault();
    event.stopPropagation();
    stepSequenceFrame(document, preview.positionSeconds, preview.durationSeconds, -1);
  } else if (event.key === "ArrowRight") {
    event.preventDefault();
    event.stopPropagation();
    stepSequenceFrame(document, preview.positionSeconds, preview.durationSeconds, 1);
  }
}

function stepSequenceFrame(document: SequenceDocumentDto, positionSeconds: number, previewDurationSeconds: number, direction: -1 | 1) {
  const frameSeconds = 1 / Math.max(1, document.frameRate);
  const nextPositionSeconds = clamp(positionSeconds + direction * frameSeconds, 0, previewDurationSeconds || document.durationSeconds);
  void runSnapshotCommand(() => commands.previewSeek(nextPositionSeconds));
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
        durationSeconds: effect.durationSeconds,
        target: effect.target,
        scope: effect.scope,
        script: effect.script,
        params: effect.params,
        markCollections: relevantMarkCollections(effect, document.markCollections)
      })
    ])
  );
}

function relevantMarkCollections(effect: SequenceEffectDto, markCollections: SequenceMarkCollectionDto[]) {
  const keys = effect.params
    .flatMap((param) => (param.value.type === "marks" ? [param.value.key] : []));
  if (keys.length === 0) return [];
  return {
    effectStartSeconds: effect.startSeconds,
    collections: markCollections
      .filter((collection) => keys.includes(collection.key))
      .map((collection) => ({ key: collection.key, marksSeconds: collection.marksSeconds }))
  };
}

function replaceAt<T>(items: T[], index: number, value: T) {
  return items.map((item, itemIndex) => (itemIndex === index ? value : item));
}

function sortCurvePoints<T extends { time: number }>(points: T[]) {
  return [...points].sort((left, right) => left.time - right.time);
}

function floatCurveValueRange(points: EditedFloatCurvePoint[]) {
  const values = points.map((point) => point.value).filter(Number.isFinite);
  const min = Math.min(0, ...values);
  const max = Math.max(1, ...values);
  if (Math.abs(max - min) < 0.0001) return { min: min - 0.5, max: max + 0.5 };
  return { min, max };
}

function floatCurveSvgPath(points: EditedFloatCurvePoint[], range: { min: number; max: number }) {
  const sorted = sortCurvePoints(points);
  if (sorted.length === 0) return "";
  return sorted
    .map((point, index) => {
      const x = point.time * 240;
      const y = 120 - ((point.value - range.min) / (range.max - range.min)) * 120;
      return `${index === 0 ? "M" : "L"}${x.toFixed(2)} ${y.toFixed(2)}`;
    })
    .join(" ");
}

function nearestFloatPointIndex(points: EditedFloatCurvePoint[], point: EditedFloatCurvePoint) {
  let bestIndex = 0;
  let bestDistance = Infinity;
  points.forEach((candidate, index) => {
    const distance = Math.abs(candidate.time - point.time) + Math.abs(candidate.value - point.value);
    if (distance < bestDistance) {
      bestDistance = distance;
      bestIndex = index;
    }
  });
  return bestIndex;
}

function nearestColorPointIndex(points: EditedColorCurvePoint[], point: EditedColorCurvePoint) {
  let bestIndex = 0;
  let bestDistance = Infinity;
  points.forEach((candidate, index) => {
    const distance = Math.abs(candidate.time - point.time) + (candidate.value === point.value ? 0 : 0.001);
    if (distance < bestDistance) {
      bestDistance = distance;
      bestIndex = index;
    }
  });
  return bestIndex;
}

function colorCurveGradient(points: EditedColorCurvePoint[]) {
  const stops = sortCurvePoints(points)
    .filter((point) => isHexColor(point.value))
    .map((point) => `${point.value} ${clamp(point.time, 0, 1) * 100}%`);
  if (stops.length === 0) return "#17181b";
  if (stops.length === 1) return stops[0]?.split(" ")[0] ?? "#17181b";
  return `linear-gradient(90deg, ${stops.join(", ")})`;
}

function roundCurveValue(value: number) {
  return Math.round(value * 1000) / 1000;
}

function curvePointsSignature(points: Array<{ time: number; value: number | string }>) {
  return JSON.stringify(points);
}

function normalizeFloatCurvePoints(points: FloatCurvePointDto[]): EditedFloatCurvePoint[] {
  const normalized = points
    .filter((point) => Number.isFinite(point.time) && Number.isFinite(point.value))
    .map((point) => ({ time: clamp(point.time, 0, 1), value: point.value }));
  return normalized.length > 0 ? normalized : [{ time: 0, value: 0 }];
}

function normalizeColorCurvePoints(points: ColorCurvePointDto[]): EditedColorCurvePoint[] {
  const normalized = points
    .filter((point) => isHexColor(point.value))
    .filter((point) => Number.isFinite(point.time))
    .map((point) => ({ time: clamp(point.time, 0, 1), value: point.value.toLowerCase() }));
  return normalized.length > 0 ? normalized : [{ time: 0, value: "#ffffff" }];
}

function isHexColor(value: string): boolean {
  return /^#[0-9a-fA-F]{6}$/.test(value);
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

function normalizePoint(point: Point3MetersDto | GeometryRenderPointDto): Point3 {
  return {
    x: point.xMeters,
    y: point.yMeters,
    z: point.zMeters
  };
}

function normalizeTransform(transform: TransformDto): Transform {
  return {
    position: normalizePoint(transform.position),
    rotation: {
      x: transform.rotation.xDegrees,
      y: transform.rotation.yDegrees,
      z: transform.rotation.zDegrees
    },
    scale: {
      x: transform.scale.x,
      y: transform.scale.y,
      z: transform.scale.z
    }
  };
}

function denormalizePoint(point: Point3): Point3MetersDto {
  return {
    xMeters: point.x,
    yMeters: point.y,
    zMeters: point.z
  };
}

function denormalizeTransform(transform: Transform): TransformDto {
  return {
    position: denormalizePoint(transform.position),
    rotation: {
      xDegrees: transform.rotation.x,
      yDegrees: transform.rotation.y,
      zDegrees: transform.rotation.z
    },
    scale: transform.scale
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
    minX: bounds.minXMeters,
    minY: bounds.minYMeters,
    maxX: bounds.maxXMeters,
    maxY: bounds.maxYMeters
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

function round6(value: number) {
  return Math.round(value * 1_000_000) / 1_000_000;
}

function clamp(value: number, min: number, max: number) {
  return Math.max(min, Math.min(max, value));
}

function roundToNanosecond(value: number) {
  return Math.round(value * 1_000_000_000) / 1_000_000_000;
}
