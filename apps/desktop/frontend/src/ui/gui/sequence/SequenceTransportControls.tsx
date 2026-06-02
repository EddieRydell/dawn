import { listen } from "@tauri-apps/api/event";

import { ChevronLeft, ChevronRight, Crosshair, LoaderCircle, Music, Pause, Play, RadioTower, SkipBack, Square, X } from "lucide-react";

import { useEffect, useRef, useState, type KeyboardEvent } from "react";

import { commands } from "../../../api";

import type { AppSnapshotDto, AudioPlaybackStatus, SequenceDocumentDto } from "../../../bindings";

import { runSnapshotCommand } from "../../../store";

import { clamp, formatMs, formatSeconds, formatSignedMs, type LivePreview, type PreviewStateEvent, type PreviewTiming } from "../shared";
import { setGlobalMarkDisplayMode, useMarkDisplayMode, type MarkDisplayMode } from "./marks";

export function SequenceTransportControls({
  document,
  preview,
  liveOutput,
  effectPreviewEnabled,
  selectedEffectIds
}: {
  document: SequenceDocumentDto;
  preview: AppSnapshotDto["preview"];
  liveOutput: AppSnapshotDto["liveOutput"];
  effectPreviewEnabled: boolean;
  selectedEffectIds: number[];
}) {
  const livePreview = useSequencePreview(preview);
  const unsupported = document.durationSeconds <= 0;
  const audioStatus = useSequenceAudioStatus(livePreview);
  const timingSummary = previewTimingSummary(livePreview.timing);
  const audioLoading = isAudioLoadingStatus(livePreview.audioPlaybackStatus);
  const audioQueued = livePreview.audioPlaybackStatus === "loading_to_play";
  const playCommand = audioQueued || livePreview.isPlaying ? commands.previewPause : commands.previewPlay;
  const [mode, setMode] = useMarkDisplayMode();
  const selectedEffectIdsSignature = selectedEffectIds.join(",");
  const stepFrame = (direction: -1 | 1) => {
    stepSequenceFrame(document, livePreview.positionSeconds, livePreview.durationSeconds, direction);
  };
  const setMarkMode = (nextMode: MarkDisplayMode) => {
    setGlobalMarkDisplayMode(nextMode);
    setMode(nextMode);
  };
  useEffect(() => {
    if (!effectPreviewEnabled) return;
    void runSnapshotCommand(() => commands.setEffectPreviewEffects(selectedEffectIds));
  }, [effectPreviewEnabled, selectedEffectIds, selectedEffectIdsSignature]);
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
        className={effectPreviewEnabled ? "active" : ""}
        title={effectPreviewEnabled ? "Stop previewing selected effect" : "Preview selected effect"}
        disabled={!effectPreviewEnabled && selectedEffectIds.length === 0}
        onClick={() => {
          const enabled = !effectPreviewEnabled;
          void runSnapshotCommand(() => commands.setEffectPreviewEnabled(enabled)).then(() => {
            if (enabled) void runSnapshotCommand(() => commands.setEffectPreviewEffects(selectedEffectIds));
          });
        }}
      >
        <Crosshair size={15} />
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
        {livePreview.previewUpdating ? <span className="sequence-preview-status">Updating preview</span> : null}
        {audioStatus !== null && <span className={`sequence-audio-status sequence-audio-status-${audioStatus.tone}`}>{audioStatus.label}</span>}
        {timingSummary !== null && <span className="sequence-timing-status">{timingSummary}</span>}
      </span>
    </div>
  );
}

export function useSequencePreview(preview: AppSnapshotDto["preview"]): LivePreview {
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
    case "ended":
    case "none":
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
    `model-lock ${formatMs(timing.modelLockWaitMs)}`,
    `analysis-clone ${formatMs(timing.analysisCloneMs)}`,
    `snapshot ${formatMs(timing.previewSnapshotMs)}`,
    `audio ${formatMs(timing.audioPollMs)}`,
    `apply ${formatMs(timing.audioApplyMs)}`,
    `render ${formatMs(timing.renderMs)}`,
    `build ${formatMs(timing.rendererBuildMs)}`,
    `eval ${formatMs(timing.frameEvaluateMs)}`,
    `frame-clone ${formatMs(timing.frameFixtureCloneMs)}`,
    `effects ${formatMs(timing.frameEffectLoopMs)}`,
    `publish ${formatMs(timing.publishMs)}`,
    `event-emit ${formatMs(timing.eventEmitMs)}`,
    `event ${formatMs(timing.eventIntervalMs)}`
  ].filter((part): part is string => part !== null);
  return ` | ${parts.join(" | ")}`;
}

function isAudioLoadingStatus(status: AudioPlaybackStatus) {
  return status === "loading" || status === "loading_to_play";
}

function isEditableShortcutTarget(target: EventTarget | null) {
  if (!(target instanceof HTMLElement)) return false;
  if (target.isContentEditable) return true;
  if (target.closest(".cm-editor")) return true;
  return target.closest("input, textarea, select") !== null;
}

export function handleSequencePlaybackShortcut(
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
