import { ChevronLeft, ChevronRight, Music, Pause, Play, RadioTower, SkipBack, Square, X } from "lucide-react";

import { useEffect, useRef, useState, type KeyboardEvent } from "react";

import { commands } from "../../../api";

import type { AppSnapshot, AudioTransportState, SequenceEditorDocument } from "../../../types";

import { runSnapshotCommand } from "../../../store";

import { clamp, formatSeconds, type AudioTransportViewSnapshot } from "../shared";
import { setGlobalMarkDisplayMode, useMarkDisplayMode, type MarkDisplayMode } from "./marks";

export function SequenceTransportControls({
  document,
  transport,
  liveOutput
}: {
  document: SequenceEditorDocument;
  transport: AppSnapshot["audioTransport"];
  liveOutput: AppSnapshot["liveOutput"];
}) {
  const unsupported = document.durationSeconds <= 0 || transport.state === "unloaded" || transport.state === "error";
  const activePlayback = isActiveAudioPlayback(transport.state);
  const [mode, setMode] = useMarkDisplayMode();
  const stepFrame = (direction: -1 | 1) => {
    stepSequenceFrame(document, transport.positionSeconds, transport.durationSeconds, direction);
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
        handleSequencePlaybackShortcut(event, document, transport, unsupported);
      }}
    >
      <button
        type="button"
        title="Play"
        disabled={unsupported || activePlayback}
        onClick={() => void runSnapshotCommand(commands.audioPlay)}
      >
        <Play size={15} />
      </button>
      <button
        type="button"
        title="Pause"
        disabled={unsupported || !activePlayback}
        onClick={() => void runSnapshotCommand(commands.audioPause)}
      >
        <Pause size={15} />
      </button>
      <button type="button" title="Stop" disabled={unsupported} onClick={() => void runSnapshotCommand(commands.audioStop)}>
        <Square size={14} />
      </button>
      <button type="button" title="Rewind to zero" disabled={unsupported} onClick={() => void runSnapshotCommand(commands.audioRewindToZero)}>
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
        {formatSeconds(transport.positionSeconds)} / {formatSeconds(transport.durationSeconds || document.durationSeconds)} | Home {formatSeconds(transport.homeSeconds)}
        {liveOutput.enabled ? ` | Live ${liveOutput.status} (${liveOutput.activeUniverseCount})` : ""}
      </span>
    </div>
  );
}

export function useSequenceTransport(transport: AppSnapshot["audioTransport"]): AudioTransportViewSnapshot {
  const [animatedPositionSeconds, setAnimatedPositionSeconds] = useState(transport.positionSeconds);
  const transportRef = useRef(transport);
  const anchor = useRef({
    transport,
    positionSeconds: transport.positionSeconds,
    anchoredAt: 0
  });

  useEffect(() => {
    transportRef.current = transport;
    anchor.current = {
      transport,
      positionSeconds: transport.positionSeconds,
      anchoredAt: performance.now()
    };
  }, [transport]);

  useEffect(() => {
    let frame = 0;
    const tick = () => {
      const latest = transportRef.current;
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
  }, [transport.state, transport.positionSeconds]);

  return shouldAnimateTransportPosition(transport)
    ? {
        ...transport,
        positionSeconds: animatedPositionSeconds
      }
    : transport;
}

function isEditableShortcutTarget(target: EventTarget | null) {
  if (!(target instanceof HTMLElement)) return false;
  if (target.isContentEditable) return true;
  if (target.closest(".cm-editor")) return true;
  return target.closest("input, textarea, select") !== null;
}

export function handleSequencePlaybackShortcut(
  event: KeyboardEvent<HTMLElement>,
  document: SequenceEditorDocument,
  transport: AppSnapshot["audioTransport"],
  unsupported: boolean
) {
  if (unsupported || isEditableShortcutTarget(event.target)) return;
  if (event.key === " ") {
    event.preventDefault();
    event.stopPropagation();
    if (event.repeat) return;
    void runSnapshotCommand(isActiveAudioPlayback(transport.state) ? commands.audioStop : commands.audioPlay);
  } else if (event.key.toLowerCase() === "s") {
    event.preventDefault();
    event.stopPropagation();
    void runSnapshotCommand(commands.audioStop);
  } else if (event.key === "Home") {
    event.preventDefault();
    event.stopPropagation();
    void runSnapshotCommand(commands.audioRewindToZero);
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

function isActiveAudioPlayback(state: AudioTransportState) {
  return state === "playing";
}

function shouldAnimateTransportPosition(transport: AudioTransportViewSnapshot) {
  return transport.state === "playing";
}

function transportExtrapolationSeconds(anchoredAt: number) {
  return anchoredAt > 0 ? (performance.now() - anchoredAt) / 1000 : 0;
}

function stepSequenceFrame(document: SequenceEditorDocument, positionSeconds: number, transportDurationSeconds: number, direction: -1 | 1) {
  const frameSeconds = 1 / Math.max(1, document.frameRate);
  const nextPositionSeconds = clamp(positionSeconds + direction * frameSeconds, 0, transportDurationSeconds || document.durationSeconds);
  void runSnapshotCommand(() => commands.audioSeek(nextPositionSeconds));
}
