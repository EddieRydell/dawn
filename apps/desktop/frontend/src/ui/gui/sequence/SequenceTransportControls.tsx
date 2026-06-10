import { listen } from "@tauri-apps/api/event";

import { ChevronLeft, ChevronRight, Music, Pause, Play, RadioTower, SkipBack, Square, X } from "lucide-react";

import { useEffect, useMemo, useRef, useState, type KeyboardEvent } from "react";

import { commands } from "../../../api";

import type { AppSnapshotDto, SequenceTransportState, SequenceEditorDocumentDto } from "../../../types";

import { runSnapshotCommand } from "../../../store";

import { clamp, formatMs, formatSeconds, formatSignedMs, type SequenceRenderEvent, type SequenceRenderTiming, type SequenceTransportSnapshot } from "../shared";
import { setGlobalMarkDisplayMode, useMarkDisplayMode, type MarkDisplayMode } from "./marks";

export function SequenceTransportControls({
  document,
  transport,
  liveOutput
}: {
  document: SequenceEditorDocumentDto;
  transport: AppSnapshotDto["sequenceTransport"];
  liveOutput: AppSnapshotDto["liveOutput"];
}) {
  const liveTransport = useSequenceTransport(transport);
  const unsupported = document.durationSeconds <= 0;
  const audioStatus = useSequenceAudioStatus(liveTransport);
  const timingSummary = renderTimingSummary(liveTransport.timing);
  const activePlayback = isActiveSequenceTransportPlayback(liveTransport.transportState);
  const playCommand = activePlayback ? commands.sequenceTransportPause : commands.sequenceTransportPlay;
  const [mode, setMode] = useMarkDisplayMode();
  const stepFrame = (direction: -1 | 1) => {
    stepSequenceFrame(document, liveTransport.positionSeconds, liveTransport.durationSeconds, direction);
  };
  const setMarkMode = (nextMode: MarkDisplayMode) => {
    setGlobalMarkDisplayMode(nextMode);
    setMode(nextMode);
  };
  return (
    <div
      className="sequence-toolbar"
      aria-label="Sequence transport"
      onKeyDownCapture={(event) => {
        handleSequencePlaybackShortcut(event, document, liveTransport, unsupported);
      }}
    >
      <button
        type="button"
        title={activePlayback ? "Pause" : "Play"}
        disabled={unsupported}
        onClick={() => void runSnapshotCommand(playCommand)}
      >
        {activePlayback ? <Pause size={15} /> : <Play size={15} />}
      </button>
      <button type="button" title="Stop" disabled={unsupported} onClick={() => void runSnapshotCommand(commands.sequenceTransportStop)}>
        <Square size={14} />
      </button>
      <button type="button" title="Rewind to zero" disabled={unsupported} onClick={() => void runSnapshotCommand(commands.sequenceTransportRewindToZero)}>
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
        {formatSeconds(liveTransport.positionSeconds)} / {formatSeconds(liveTransport.durationSeconds || document.durationSeconds)} | Home {formatSeconds(liveTransport.homeSeconds)}
        {document.audio ? ` | ${document.audio.exists ? document.audio.fileName : "Missing audio"}` : ""}
        {liveOutput.enabled ? ` | Live ${liveOutput.status} (${liveOutput.activeUniverseCount})` : ""}
        {audioStatus !== null && <span className={`sequence-audio-status sequence-audio-status-${audioStatus.tone}`}>{audioStatus.label}</span>}
        {timingSummary !== null && <span className="sequence-timing-status">{timingSummary}</span>}
      </span>
    </div>
  );
}

export function useSequenceTransport(transport: AppSnapshotDto["sequenceTransport"]): SequenceTransportSnapshot {
  const [renderEvent, setRenderEvent] = useState<SequenceRenderEvent | null>(null);
  const [animatedPositionSeconds, setAnimatedPositionSeconds] = useState(transport.positionSeconds);
  const snapshotRef = useRef(transport);
  const anchor = useRef({
    transport,
    positionSeconds: transport.positionSeconds,
    anchoredAt: 0
  });

  useEffect(() => {
    snapshotRef.current = transport;
  }, [transport]);

  useEffect(() => {
    let dispose: (() => void) | undefined;
    let disposed = false;
    void (async () => {
      dispose = await listen<SequenceRenderEvent>("sequence_render_state_changed", (event) => {
        if (!disposed) {
          setRenderEvent(renderEventMatchesSnapshot(event.payload, snapshotRef.current) ? event.payload : null);
        }
      });
    })();
    return () => {
      disposed = true;
      dispose?.();
    };
  }, []);

  const liveTransport: SequenceTransportSnapshot = useMemo(() => {
    const matchingRenderTiming = renderEvent !== null && renderEventMatchesSnapshot(renderEvent, transport) ? renderEvent.timing : null;
    return matchingRenderTiming === null
      ? transport
      : {
          ...transport,
          timing: matchingRenderTiming
        };
  }, [transport, renderEvent]);
  const liveTransportRef = useRef(liveTransport);

  useEffect(() => {
    liveTransportRef.current = liveTransport;
  }, [liveTransport]);

  useEffect(() => {
    anchor.current = {
      transport,
      positionSeconds: transport.positionSeconds,
      anchoredAt: performance.now()
    };
  }, [transport]);

  useEffect(() => {
    let frame = 0;
    const tick = () => {
      const latest = liveTransportRef.current;
      const current = anchor.current;
      if (!shouldAnimateTransportPosition(latest) || !shouldAnimateTransportPosition(current.transport)) {
        setAnimatedPositionSeconds(latest.positionSeconds);
        return;
      }
      const elapsedSeconds = transportExtrapolationSeconds(current.anchoredAt);
      setAnimatedPositionSeconds(clamp(current.positionSeconds + elapsedSeconds, 0, current.transport.durationSeconds));
      frame = window.requestAnimationFrame(tick);
    };
    frame = window.requestAnimationFrame(tick);
    return () => {
      window.cancelAnimationFrame(frame);
    };
  }, [liveTransport.transportState, liveTransport.positionSeconds]);

  return shouldAnimateTransportPosition(liveTransport)
    ? {
        ...liveTransport,
        positionSeconds: animatedPositionSeconds
      }
    : liveTransport;
}

function useSequenceAudioStatus(transport: AppSnapshotDto["sequenceTransport"]) {
  switch (transport.audioPlaybackStatus) {
    case "playing":
      return { label: "Audio playing", tone: "ready" };
    case "ready":
      return transport.audio !== null ? { label: "Audio ready", tone: "ready" } : null;
    case "missing":
      return { label: "Audio missing", tone: "error" };
    case "error":
      return { label: "Audio error", tone: "error" };
    case "ended":
    case "none":
      return null;
  }
}

function renderTimingSummary(timing: SequenceRenderTiming | undefined) {
  if (timing === undefined || timing.backendSeconds === 0) return null;
  const frameAudio = timing.renderBufferMinusAudioMs;
  const snapshotAudio = timing.snapshotMinusAudioMs;
  const parts = [
    `fps ${timing.activeFps}/${timing.targetFps}`,
    `target ${formatMs(timing.targetFrameMs)}`,
    `total ${formatMs(timing.loopTotalMs)}`,
    `work ${formatMs(timing.loopElapsedMs)}`,
    `unaccounted ${formatMs(timing.loopUnaccountedMs)}`,
    `sleep ${formatMs(timing.sleepActualMs)}`,
    frameAudio === null ? null : `frame-audio ${formatSignedMs(frameAudio)}`,
    snapshotAudio === null ? null : `playhead-audio ${formatSignedMs(snapshotAudio)}`,
    `interval ${formatMs(timing.loopIntervalMs)}`,
    `model ${formatMs(timing.modelUpdateMs)}`,
    `model-lock ${formatMs(timing.modelLockWaitMs)}`,
    `project-snapshot ${formatMs(timing.projectSnapshotMs)}`,
    `audio-poll ${formatMs(timing.audioPollMs)}`,
    `apply ${formatMs(timing.audioApplyMs)}`,
    `render-wall ${formatMs(timing.renderWallMs)}`,
    `render-eval ${formatMs(timing.renderMs)}`,
    `render-overhead ${formatMs(timing.renderOverheadMs)}`,
    `invalidation ${formatMs(timing.renderInvalidationMs)}`,
    `cache ${formatMs(timing.renderCacheMs)}`,
    `result ${formatMs(timing.renderResultMs)}`,
    `build ${formatMs(timing.rendererBuildMs)}`,
    `effects ${formatMs(timing.frameEffectLoopMs)}`,
    `live-output ${formatMs(timing.liveOutputMs)}`
  ].filter((part): part is string => part !== null);
  return ` | ${parts.join(" | ")}`;
}

function isEditableShortcutTarget(target: EventTarget | null) {
  if (!(target instanceof HTMLElement)) return false;
  if (target.isContentEditable) return true;
  if (target.closest(".cm-editor")) return true;
  return target.closest("input, textarea, select") !== null;
}

export function handleSequencePlaybackShortcut(
  event: KeyboardEvent<HTMLElement>,
  document: SequenceEditorDocumentDto,
  transport: AppSnapshotDto["sequenceTransport"],
  unsupported: boolean
) {
  if (unsupported || isEditableShortcutTarget(event.target)) return;
  if (event.key === " ") {
    event.preventDefault();
    event.stopPropagation();
    if (event.repeat) return;
    void runSnapshotCommand(isActiveSequenceTransportPlayback(transport.transportState) ? commands.sequenceTransportStop : commands.sequenceTransportPlay);
  } else if (event.key.toLowerCase() === "s") {
    event.preventDefault();
    event.stopPropagation();
    void runSnapshotCommand(commands.sequenceTransportStop);
  } else if (event.key === "Home") {
    event.preventDefault();
    event.stopPropagation();
    void runSnapshotCommand(commands.sequenceTransportRewindToZero);
  } else if (event.key === "ArrowLeft") {
    event.preventDefault();
    event.stopPropagation();
    stepSequenceFrame(document, transport.positionSeconds, transport.durationSeconds, -1);
  } else if (event.key === "ArrowRight") {
    event.preventDefault();
    event.stopPropagation();
    stepSequenceFrame(document, transport.positionSeconds, transport.durationSeconds, 1);
  }
}

function isActiveSequenceTransportPlayback(state: SequenceTransportState) {
  return state === "playing";
}

function shouldAnimateTransportPosition(transport: SequenceTransportSnapshot) {
  return transport.transportState === "playing";
}

function transportExtrapolationSeconds(anchoredAt: number) {
  return anchoredAt > 0 ? (performance.now() - anchoredAt) / 1000 : 0;
}

function renderEventMatchesSnapshot(event: SequenceRenderEvent, snapshot: AppSnapshotDto["sequenceTransport"]) {
  return (
    event.sourceLabel === snapshot.sourceLabel &&
    event.renderGeneration === snapshot.renderGeneration &&
    event.renderDirtyRevision === snapshot.renderDirtyRevision &&
    event.geometryIdentity === snapshot.geometryIdentity &&
    sequenceKeyMatches(event.sourceKey, snapshot.sourceKey)
  );
}

function sequenceKeyMatches(left: AppSnapshotDto["sequenceTransport"]["sourceKey"], right: AppSnapshotDto["sequenceTransport"]["sourceKey"]) {
  if (left === null || right === null) return left === right;
  return left.path === right.path && left.objectKey === right.objectKey;
}

function stepSequenceFrame(document: SequenceEditorDocumentDto, positionSeconds: number, transportDurationSeconds: number, direction: -1 | 1) {
  const frameSeconds = 1 / Math.max(1, document.frameRate);
  const nextPositionSeconds = clamp(positionSeconds + direction * frameSeconds, 0, transportDurationSeconds || document.durationSeconds);
  void runSnapshotCommand(() => commands.sequenceTransportSeek(nextPositionSeconds));
}
