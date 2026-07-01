import * as ContextMenu from "@radix-ui/react-context-menu";
import { convertFileSrc } from "@tauri-apps/api/core";

import { useCallback, useEffect, useMemo, useRef, useState, type MouseEvent } from "react";

import { Trash2 } from "lucide-react";

import { commands } from "../../../api";

import type { AppSettings, LayoutTarget, PersistedSequenceViewportState, SequenceAudio, SequenceClipRaster, SequenceEditorDocument, SequenceEffectScope, SequenceEffectScript } from "../../../types";

import { runGuiEditCommand, runSnapshotCommand, useAppStore } from "../../../store";

import { clamp, formatSeconds, roundToNanosecond, type GuiFocus, type SequenceSelection } from "../shared";

import { defaultMarkColor, drawSequenceMarks, committedMarkDrafts, markIndexAfterMove, nextCollectionKey, useMarkDisplayMode } from "./marks";

import { targetsEqual } from "./sequenceTargets";

import { buildSequenceClipLayout, constrainEffectLaneDelta, constrainEffectMoveDelta, constrainEffectResizeDelta, constrainMarkDelta, effectMoveDrafts, effectResizeDrafts, hitSequence, hitSequenceMark, markMoveDrafts, markRefLookup, mergeSequenceSelection, MIN_EFFECT_DURATION_SECONDS, nextEffectSelection, nextMarkSelection, normalizedRect, selectedEffectId, selectionCount, selectionFromMarqueeEffects, selectionFromMarqueeMarks, sequenceHoverEqual, setMarkDraft, singleEffectSelectionFocus, singleSelectionFocus, selectionFromSingle, type MarkDraftLookup, type SequenceClipLayout, type SequenceContextMenu, type SequenceHover, type SequenceMarquee, type SequenceDraft, type SequenceViewport } from "./sequenceSelection";

const SEQUENCE_CANVAS = {
  leftGutterPx: 128,
  topPx: 66,
  audioStripTopPx: 28,
  initialPxPerSecond: 80,
  initialLaneHeightPx: 42,
  minPxPerSecond: 0.01,
  maxPxPerSecond: 600,
  maxZoomPxPerSecond: 12000,
  minLaneHeightPx: 24,
  maxLaneHeightPx: 600,
  wheelZoomScale: 0.002,
  scrubStepSeconds: 0.01,
  nudgeSeconds: 0.001,
  shiftedNudgeSeconds: 0.01
} as const;

const SEQUENCE_COLORS = {
  page: "#111214",
  panel: "#17181b",
  laneAlt: "#15171a",
  laneSelected: "rgb(106 191 138 / 12%)",
  grid: "#24272c",
  border: "#373b42",
  gridFaint: "#2c3036",
  timelineMajor: "#343941",
  timelineMinor: "#1f2227",
  timelineLabel: "#a8a29a",
  textMuted: "#c7c0b6",
  textFaint: "#696b70",
  overlay: "rgb(255 250 240 / 10%)",
  clipSelected: "#f0f0f0",
  clipHover: "#d8d2c9",
  clipBorder: "#8a8d93",
  accent: "#6abf8a",
  warning: "#f0c46b",
  markMarquee: "#8ecae6",
  markMarqueeFill: "rgb(142 202 230 / 12%)",
  effectMarqueeFill: "rgb(240 196 107 / 12%)",
  playhead: "#d6a35a"
} as const;

const SEQUENCE_DRAG_THRESHOLD_PX = 4;

type SequenceDragState =
  | null
  | { kind: "sequence"; id: number; startX: number; startY: number; active: boolean; originalStartSeconds: number; laneIndex: number; resize: "none" | "left" | "right" }
  | { kind: "mark"; collectionKey: string; index: number; startX: number; startY: number; active: boolean; originalTimeSeconds: number }
  | { kind: "marquee"; state: SequenceMarquee }
  | { kind: "sequenceScrub" };

let sequenceViewportStateTimer: number | undefined;

export function SequenceCanvas({
  document,
  playheadSeconds,
  homeSeconds,
  selected,
  setSelected,
  sequenceSelection,
  setSequenceSelection,
  activeMarkCollectionKey,
  setActiveMarkCollectionKey,
  visibleMarkCollectionKeys,
  setVisibleMarkCollectionKeys
}: {
  document: SequenceEditorDocument;
  playheadSeconds: number;
  homeSeconds: number;
  selected: GuiFocus;
  setSelected: (id: GuiFocus) => void;
  sequenceSelection: SequenceSelection;
  setSequenceSelection: (selection: SequenceSelection) => void;
  activeMarkCollectionKey: string | null;
  setActiveMarkCollectionKey: (key: string | null) => void;
  visibleMarkCollectionKeys: Set<string>;
  setVisibleMarkCollectionKeys: (keys: Set<string>) => void;
}) {
  const canvas = useRef<HTMLCanvasElement | null>(null);
  const drag = useRef<SequenceDragState>(null);
  const sequenceSelectionRef = useRef<SequenceSelection>(sequenceSelection);
  const [draft, setDraft] = useState<SequenceDraft | null>(null);
  const [groupDraft, setGroupDraft] = useState<SequenceDraft[]>([]);
  const [markDrafts, setMarkDrafts] = useState<MarkDraftLookup>(() => new Map());
  const [sequenceContextMenu, setSequenceContextMenu] = useState<SequenceContextMenu | null>(null);
  const [hover, setHover] = useState<SequenceHover>(null);
  const [dragCursor, setDragCursor] = useState<"grabbing" | null>(null);
  const [selectedLaneIndex, setSelectedLaneIndex] = useState<number | null>(null);
  const [selectedTimeSeconds, setSelectedTimeSeconds] = useState<number | null>(null);
  const [marquee, setMarquee] = useState<SequenceMarquee | null>(null);
  const [canvasSize, setCanvasSize] = useState({ width: 0, height: 0 });
  const restoreState = useAppStore((store) => store.restoreState);
  const settings = useAppStore((store) => store.snapshot?.settings ?? null);
  const restoreKey = `${document.path}::${document.objectKey}`;
  const restoredViewport = restoreState?.sequenceViewports[restoreKey];
  const [viewport, setViewport] = useState<SequenceViewport>(() => sequenceViewportFromPersisted(restoredViewport, settings));
  const initializedViewportKey = useRef<string | null>(null);
  const restoredViewportKey = useRef<string | null>(restoredViewport === undefined ? null : restoreKey);
  const left = SEQUENCE_CANVAS.leftGutterPx;
  const top = SEQUENCE_CANVAS.topPx;
  const audioStripTop = SEQUENCE_CANVAS.audioStripTopPx;
  const audioStripHeight = top - audioStripTop;
  const waveform = useSequenceWaveform(document.audio);
  const [mode] = useMarkDisplayMode();
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
    const target = canvas.current;
    if (!target) return;
    const updateSize = () => {
      const rect = target.getBoundingClientRect();
      setCanvasSize({ width: rect.width, height: rect.height });
      const timelineWidth = Math.max(1, rect.width - left);
      const key = `${document.durationSeconds}:${document.lanes.length}`;
      if (rect.width > 0 && initializedViewportKey.current !== key) {
        initializedViewportKey.current = key;
        if (restoredViewport === undefined) {
          setViewport({
            pxPerSecond: initialSequencePxPerSecond(settings, timelineWidth, document.durationSeconds),
            laneHeight: initialSequenceLaneHeight(settings),
            scrollXSeconds: 0,
            scrollY: 0
          });
        }
      }
      setViewport((current) => {
        const minPxPerSecond = minSequencePxPerSecond(timelineWidth, document.durationSeconds);
        const pxPerSecond = Math.max(current.pxPerSecond, minPxPerSecond);
        const scrollXSeconds = clamp(current.scrollXSeconds, 0, Math.max(0, document.durationSeconds - timelineWidth / pxPerSecond));
        if (pxPerSecond === current.pxPerSecond && scrollXSeconds === current.scrollXSeconds) return current;
        return {
          ...current,
          pxPerSecond,
          scrollXSeconds
        };
      });
    };
    const frame = window.requestAnimationFrame(updateSize);
    const observer = new ResizeObserver(updateSize);
    observer.observe(target);
    return () => {
      window.cancelAnimationFrame(frame);
      observer.disconnect();
    };
  }, [document.durationSeconds, document.lanes.length, left, restoredViewport, settings]);

  useEffect(() => {
    if (restoredViewport === undefined || restoredViewportKey.current === restoreKey) return;
    restoredViewportKey.current = restoreKey;
    setViewport(sequenceViewportFromPersisted(restoredViewport, settings));
  }, [restoreKey, restoredViewport, settings]);

  useEffect(() => {
    const state: PersistedSequenceViewportState = {
      pxPerSecond: viewport.pxPerSecond,
      laneHeight: viewport.laneHeight,
      scrollXSeconds: viewport.scrollXSeconds,
      scrollY: viewport.scrollY,
      activeMarkCollectionKey,
      visibleMarkCollectionKeys: [...visibleMarkCollectionKeys]
    };
    scheduleSequenceViewportStateSave(document.path, document.objectKey, state);
  }, [activeMarkCollectionKey, document.objectKey, document.path, viewport, visibleMarkCollectionKeys]);

  const visibleClips = useMemo(
    () => buildSequenceClipLayout(document, groupDraft.length > 0 ? groupDraft : draft === null ? [] : [draft], viewport, left, top),
    [document, groupDraft, left, draft, top, viewport]
  );
  const visibleRasterClips = useMemo(() => {
    return visibleClips
      .filter((clip) => clip.rect.x + clip.rect.width >= left && clip.rect.x <= canvasSize.width && clip.rect.y + clip.rect.height >= top && clip.rect.y <= canvasSize.height);
  }, [canvasSize.height, canvasSize.width, left, top, visibleClips]);
  const clipRasters = useSequenceClipRasters(document, visibleRasterClips, viewport.laneHeight, settings);
  const selectedEffectIds = useMemo(() => new Set<number>(sequenceSelection?.type === "effects" ? sequenceSelection.ids : []), [sequenceSelection]);
  const selectedMarks = useMemo(
    () => markRefLookup(sequenceSelection?.type === "marks" ? sequenceSelection.marks : []),
    [sequenceSelection]
  );

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
    ctx.fillStyle = SEQUENCE_COLORS.page;
    ctx.fillRect(0, 0, rect.width, rect.height);
    ctx.font = "12px Inter, sans-serif";

    const timelineWidth = Math.max(1, rect.width - left);
    const laneCount = document.lanes.length;
    const totalLaneHeight = laneCount * viewport.laneHeight;
    const maxScrollXSeconds = Math.max(0, document.durationSeconds - timelineWidth / viewport.pxPerSecond);
    const maxScrollY = Math.max(0, totalLaneHeight - Math.max(1, rect.height - top));
    const scrollXSeconds = clamp(viewport.scrollXSeconds, 0, maxScrollXSeconds);
    const scrollY = clamp(viewport.scrollY, 0, maxScrollY);

    ctx.fillStyle = SEQUENCE_COLORS.panel;
    ctx.fillRect(0, 0, left, rect.height);
    ctx.fillStyle = SEQUENCE_COLORS.page;
    ctx.fillRect(left, top, timelineWidth, rect.height - top);

    ctx.save();
    ctx.beginPath();
    ctx.rect(0, top, rect.width, rect.height - top);
    ctx.clip();
      document.lanes.forEach((lane, index) => {
      const y = top + index * viewport.laneHeight - scrollY;
      if (y > rect.height || y + viewport.laneHeight < top) return;
      ctx.fillStyle = index % 2 === 0 ? SEQUENCE_COLORS.page : SEQUENCE_COLORS.laneAlt;
      ctx.fillRect(left, y, timelineWidth, viewport.laneHeight);
      if (selectedLaneIndex === index) {
        ctx.fillStyle = SEQUENCE_COLORS.laneSelected;
        ctx.fillRect(left, y, timelineWidth, viewport.laneHeight);
      }
      ctx.strokeStyle = SEQUENCE_COLORS.grid;
      ctx.beginPath();
      ctx.moveTo(left, y + viewport.laneHeight + 0.5);
      ctx.lineTo(rect.width, y + viewport.laneHeight + 0.5);
      ctx.stroke();
      ctx.fillStyle = SEQUENCE_COLORS.panel;
      ctx.fillRect(0, y, left, viewport.laneHeight);
      ctx.fillStyle = SEQUENCE_COLORS.textMuted;
      ctx.fillText(lane.label, 12, y + viewport.laneHeight / 2 + 4);
    });
    ctx.restore();

    ctx.strokeStyle = SEQUENCE_COLORS.border;
    ctx.beginPath();
    ctx.moveTo(left, 0);
    ctx.lineTo(left, rect.height);
    ctx.stroke();

    ctx.fillStyle = SEQUENCE_COLORS.panel;
    ctx.fillRect(0, 0, rect.width, top);
    ctx.fillStyle = SEQUENCE_COLORS.page;
    ctx.fillRect(left, audioStripTop, timelineWidth, audioStripHeight);
    ctx.strokeStyle = SEQUENCE_COLORS.gridFaint;
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
      selectedMarks,
      mode,
      left,
      audioStripTop,
      audioStripHeight,
      timelineWidth,
      rect.height,
      viewport.pxPerSecond,
      scrollXSeconds,
      committedMarkDrafts(visibleMarkCollections, markDrafts)
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
      ctx.fillStyle = SEQUENCE_COLORS.textFaint;
      ctx.fillRect(clip.rect.x, clip.rect.y, clip.rect.width, clip.rect.height);
      const expectedRasterKey = clipRasters.expectedRasterKeys.get(clip.effect.id) ?? null;
      const raster = expectedRasterKey === null ? null : clipRasters.rasters.get(expectedRasterKey) ?? null;
      const rasterError = clipRasters.errors.has(clip.effect.id);
      if (raster !== null) {
        drawClipRaster(ctx, raster, clip.rect);
      }
      if (rasterError) {
        drawClipRasterWarning(ctx, clip.rect);
      }
      if (hoverResize !== null) {
        ctx.fillStyle = SEQUENCE_COLORS.overlay;
        ctx.fillRect(clip.rect.x, clip.rect.y, clip.rect.width, clip.rect.height);
      }
      const clipSelected = selectedEffectIds.has(clip.effect.id) || (selected?.type === "effect" && selected.id === clip.effect.id);
      ctx.strokeStyle = clipSelected ? SEQUENCE_COLORS.clipSelected : hoverResize !== null ? SEQUENCE_COLORS.clipHover : SEQUENCE_COLORS.clipBorder;
      ctx.lineWidth = clipSelected || hoverResize !== null ? 2 : 1;
      ctx.strokeRect(clip.rect.x + 0.5, clip.rect.y + 0.5, Math.max(0, clip.rect.width - 1), Math.max(0, clip.rect.height - 1));
      if (hoverResize === "left" || hoverResize === "right") {
        const handleX = hoverResize === "left" ? clip.rect.x : clip.rect.x + clip.rect.width;
        ctx.fillStyle = SEQUENCE_COLORS.warning;
        ctx.fillRect(handleX - 2, clip.rect.y + 4, 4, Math.max(4, clip.rect.height - 8));
      }
    }
    ctx.restore();

    const playheadX = left + (clamp(playheadSeconds, 0, document.durationSeconds) - scrollXSeconds) * viewport.pxPerSecond;
    const homeX = left + (clamp(homeSeconds, 0, document.durationSeconds) - scrollXSeconds) * viewport.pxPerSecond;
    if (homeX >= left && homeX <= rect.width) {
      ctx.strokeStyle = SEQUENCE_COLORS.accent;
      ctx.lineWidth = 1;
      ctx.setLineDash([4, 4]);
      ctx.beginPath();
      ctx.moveTo(homeX + 0.5, top);
      ctx.lineTo(homeX + 0.5, rect.height);
      ctx.stroke();
      ctx.setLineDash([]);
      ctx.fillStyle = SEQUENCE_COLORS.accent;
      ctx.fillRect(homeX - 3, top, 7, 4);
    }
    if (playheadX >= left && playheadX <= rect.width) {
      ctx.strokeStyle = SEQUENCE_COLORS.warning;
      ctx.lineWidth = 1;
      ctx.beginPath();
      ctx.moveTo(playheadX + 0.5, top);
      ctx.lineTo(playheadX + 0.5, rect.height);
      ctx.stroke();
    }
    if (selectedTimeSeconds !== null) {
      const selectedX = left + (clamp(selectedTimeSeconds, 0, document.durationSeconds) - scrollXSeconds) * viewport.pxPerSecond;
      if (selectedX >= left && selectedX <= rect.width) {
        ctx.strokeStyle = SEQUENCE_COLORS.markMarquee;
        ctx.lineWidth = 1;
        ctx.beginPath();
        ctx.moveTo(selectedX + 0.5, top);
        ctx.lineTo(selectedX + 0.5, rect.height);
        ctx.stroke();
      }
    }
    if (marquee?.active === true) {
      const box = normalizedRect(marquee.startX, marquee.startY, marquee.x, marquee.y);
      ctx.fillStyle = marquee.mode === "marks" ? SEQUENCE_COLORS.markMarqueeFill : SEQUENCE_COLORS.effectMarqueeFill;
      ctx.strokeStyle = marquee.mode === "marks" ? SEQUENCE_COLORS.markMarquee : SEQUENCE_COLORS.warning;
      ctx.lineWidth = 1;
      ctx.fillRect(box.x, box.y, box.width, box.height);
      ctx.strokeRect(box.x + 0.5, box.y + 0.5, Math.max(0, box.width - 1), Math.max(0, box.height - 1));
    }

    ctx.strokeStyle = SEQUENCE_COLORS.playhead;
    ctx.lineWidth = 1;
    ctx.beginPath();
    ctx.moveTo(left + 0.5, top);
    ctx.lineTo(left, rect.height);
    ctx.stroke();
  }, [document, left, top, audioStripTop, audioStripHeight, viewport, visibleClips, selected, selectedEffectIds, selectedMarks, playheadSeconds, homeSeconds, selectedLaneIndex, selectedTimeSeconds, marquee, waveform.audio, visibleMarkCollections, mode, markDrafts, hover, clipRasters]);

  const seekFromCanvas = (event: MouseEvent<HTMLCanvasElement>) => {
    const x = event.nativeEvent.offsetX;
    if (x < left) return;
    const positionSeconds = clamp(Math.round((viewport.scrollXSeconds + (x - left) / viewport.pxPerSecond) / SEQUENCE_CANVAS.scrubStepSeconds) * SEQUENCE_CANVAS.scrubStepSeconds, 0, document.durationSeconds);
    void runSnapshotCommand(() => commands.audioSeek(positionSeconds));
  };
  const timeFromCanvasX = (x: number) => clamp(roundToNanosecond(viewport.scrollXSeconds + (x - left) / viewport.pxPerSecond), 0, document.durationSeconds);
  const addEffectFromContextMenu = async (script: SequenceEffectScript, menu: SequenceContextMenu) => {
    const hasMarksParams = script.params.some((param) => param.kind === "marks");
    let markCollectionKey = hasMarksParams ? activeMarkCollectionKey ?? document.markCollections[0]?.key ?? null : null;
    if (hasMarksParams && markCollectionKey === null) {
      const newCollectionKey = nextCollectionKey("Marks", document.markCollections);
      await runGuiEditCommand(() =>
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
    const scope: SequenceEffectScope = target.kind === "group" ? "wholeTarget" : "perFixture";
    await runGuiEditCommand(() =>
      commands.applySequenceGuiEdit({
        type: "addEffect",
        script: script.script,
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
      await runGuiEditCommand(() =>
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
    await runGuiEditCommand(() =>
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
      await runGuiEditCommand(() =>
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
    await runGuiEditCommand(() =>
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
    setSelected({ type: "mark", collectionKey, index: Math.max(0, nextIndex) });
  };
  const deleteSelectedEffect = async (effectId: number) => {
    await runGuiEditCommand(() => commands.applySequenceGuiEdit({ type: "deleteEffect", id: effectId }));
    setSelected(null);
    updateSequenceSelection(null);
  };
  const deleteContextMark = async (menu: Extract<SequenceContextMenu, { kind: "mark" }>) => {
    await runGuiEditCommand(() =>
      commands.applySequenceGuiEdit({
        type: "deleteMark",
        collectionKey: menu.collectionKey,
        index: menu.index
      })
    );
    setSelected(null);
    updateSequenceSelection(null);
  };
  const retargetContextEffect = async (effectId: number, target: LayoutTarget) => {
    await runGuiEditCommand(() => commands.applySequenceGuiEdit({ type: "retargetEffect", id: effectId, target }));
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
        const selectedMark = selected?.type === "mark" ? { collectionKey: selected.collectionKey, index: selected.index } : null;
        const focusedEffectId = selectedEffectId(selected);
        const activeSelection = sequenceSelection ?? selectionFromSingle(selected);
        if ((event.ctrlKey || event.metaKey) && !isTextEntryElement(event.target)) {
          const key = event.key.toLowerCase();
          if ((key === "c" || key === "x") && activeSelection !== null && selectionCount(activeSelection) > 0) {
            event.preventDefault();
            const editType = key === "c" ? "copy" : "cut";
            void runGuiEditCommand(() => commands.applySequenceSelectionEdit({ type: editType, selection: activeSelection }).then((result) => {
              updateSequenceSelection(result.selection);
              setSelected(singleSelectionFocus(result.selection));
              return result;
            }));
            return;
          }
          if (key === "v") {
            event.preventDefault();
            void runGuiEditCommand(() => commands.applySequenceSelectionEdit({
              type: "paste",
              anchor: { laneIndex: selectedLaneIndex as never, timeSeconds: selectedTimeSeconds as never }
            }).then((result) => {
              updateSequenceSelection(result.selection);
              setSelected(singleSelectionFocus(result.selection));
              return result;
            }));
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
          const deltaSeconds = (event.key === "ArrowLeft" ? -1 : 1) * (event.shiftKey ? SEQUENCE_CANVAS.shiftedNudgeSeconds : SEQUENCE_CANVAS.nudgeSeconds);
          const nextTimeSeconds = clamp(timeSeconds + deltaSeconds, 0, document.durationSeconds);
          const nextIndex = markIndexAfterMove(collection, selectedMark.index, nextTimeSeconds);
          const nextDrafts: MarkDraftLookup = new Map();
          setMarkDraft(nextDrafts, selectedMark, { collectionKey: selectedMark.collectionKey, index: selectedMark.index, timeSeconds: nextTimeSeconds, committedIndex: nextIndex });
          setMarkDrafts(nextDrafts);
          void runGuiEditCommand(() =>
            commands.applySequenceGuiEdit({
              type: "moveMark",
              collectionKey: selectedMark.collectionKey,
              index: selectedMark.index,
              timeSeconds: nextTimeSeconds
            })
          ).then(() => {
            setSelected({ type: "mark", collectionKey: selectedMark.collectionKey, index: nextIndex });
            setMarkDrafts(new Map());
          });
          return;
        }
        if ((event.key !== "Delete" && event.key !== "Backspace") || isTextEntryElement(event.target)) return;
        event.preventDefault();
        if (activeSelection !== null && selectionCount(activeSelection) > 1) {
          void runGuiEditCommand(() => commands.applySequenceSelectionEdit({ type: "delete", selection: activeSelection }).then((result) => {
            updateSequenceSelection(result.selection);
            setSelected(null);
            return result;
          }));
          return;
        }
        if (focusedEffectId !== null) {
          void deleteSelectedEffect(focusedEffectId);
          return;
        }
        if (selectedMark === null) return;
        void runGuiEditCommand(() =>
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
          setSelected({ type: "effect", id: hit.effect.id });
          updateSequenceSelection({ type: "effects", ids: [hit.effect.id] });
          setSequenceContextMenu({ kind: "effect", laneIndex: hit.laneIndex, startSeconds, effectId: hit.effect.id });
          return;
        }
        const markHit = hitSequenceMark(visibleMarkCollections, mode, x, y, left, audioStripTop, audioStripHeight, canvasSize.height, viewport);
        if (markHit !== null) {
          setSelected({ type: "mark", collectionKey: markHit.collectionKey, index: markHit.index });
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
        setMarkDrafts(new Map());
        if (x >= left && y < top) {
          drag.current = { kind: "sequenceScrub" };
          seekFromCanvas(event);
          return;
        }
        if (x < left && y >= top && document.lanes.length > 0) {
          const laneIndex = clamp(Math.floor((y - top + viewport.scrollY) / viewport.laneHeight), 0, document.lanes.length - 1);
          const lane = document.lanes[laneIndex];
          if (lane === undefined) return;
          const ids = document.effects.filter((effect) => targetsEqual(effect.target, lane.target)).map((effect) => effect.id);
          setSelectedLaneIndex(laneIndex);
          updateSequenceSelection(ids.length > 0 ? { type: "effects", ids } : null);
          setSelected(singleEffectSelectionFocus(ids));
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
          setSelected(nextSelection.type === "effects" ? singleEffectSelectionFocus(nextSelection.ids) ?? { type: "effect", id: hit.effect.id } : { type: "effect", id: hit.effect.id });
          setSelectedLaneIndex(hit.laneIndex);
          drag.current = {
            kind: "sequence",
            id: hit.effect.id,
            startX: event.nativeEvent.offsetX,
            startY: event.nativeEvent.offsetY,
            active: false,
            originalStartSeconds: hit.effect.startSeconds,
            laneIndex: hit.laneIndex,
            resize: hit.resize
          };
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
          setSelected({ type: "mark", collectionKey: mark.collectionKey, index: mark.index });
          setActiveMarkCollectionKey(markHit.collectionKey);
          drag.current = {
            kind: "mark",
            collectionKey: markHit.collectionKey,
            index: markHit.index,
            startX: x,
            startY: y,
            active: false,
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
            active: current.state.active || Math.hypot(event.nativeEvent.offsetX - current.state.startX, event.nativeEvent.offsetY - current.state.startY) >= SEQUENCE_DRAG_THRESHOLD_PX
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
          if (!current.active) {
            if (Math.hypot(event.nativeEvent.offsetX - current.startX, event.nativeEvent.offsetY - current.startY) < SEQUENCE_DRAG_THRESHOLD_PX) return;
            current.active = true;
          }
          const deltaSeconds = roundToNanosecond((event.nativeEvent.offsetX - current.startX) / viewport.pxPerSecond);
          const timeSeconds = clamp(current.originalTimeSeconds + deltaSeconds, 0, document.durationSeconds);
          setSelected({ type: "mark", collectionKey: current.collectionKey, index: current.index });
          const collection = document.markCollections.find((candidate) => candidate.key === current.collectionKey);
          const committedIndex = collection === undefined ? current.index : markIndexAfterMove(collection, current.index, timeSeconds);
          const activeSelection = sequenceSelectionRef.current;
          if (activeSelection?.type === "marks" && activeSelection.marks.length > 1 && activeSelection.marks.some((mark) => mark.collectionKey === current.collectionKey && mark.index === current.index)) {
            const constrainedDelta = constrainMarkDelta(document, activeSelection.marks, deltaSeconds);
            setMarkDrafts(markMoveDrafts(document, activeSelection.marks, constrainedDelta));
          } else {
            const nextDrafts: MarkDraftLookup = new Map();
            setMarkDraft(nextDrafts, { collectionKey: current.collectionKey, index: current.index }, { collectionKey: current.collectionKey, index: current.index, timeSeconds, committedIndex });
            setMarkDrafts(nextDrafts);
          }
          setDraft(null);
          setGroupDraft([]);
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
        if (!current.active) {
          if (Math.hypot(event.nativeEvent.offsetX - current.startX, event.nativeEvent.offsetY - current.startY) < SEQUENCE_DRAG_THRESHOLD_PX) return;
          current.active = true;
          setDragCursor("grabbing");
        }
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
            setGroupDraft(effectMoveDrafts(document, activeEffectSelection.ids, constrainedDelta, laneDelta));
          } else {
            const constrainedDelta = constrainEffectResizeDelta(document, activeEffectSelection.ids, current.resize, deltaSeconds);
            setGroupDraft(effectResizeDrafts(document, activeEffectSelection.ids, current.resize, constrainedDelta));
          }
          setDraft(null);
          return;
        }
        setGroupDraft([]);
        if (current.resize === "left") {
          const startSeconds = clamp(current.originalStartSeconds + deltaSeconds, 0, effect.startSeconds + effect.durationSeconds - MIN_EFFECT_DURATION_SECONDS);
          setDraft({ id: effect.id, startSeconds, durationSeconds: effect.startSeconds + effect.durationSeconds - startSeconds, laneIndex });
        } else if (current.resize === "right") {
          setDraft({ id: effect.id, startSeconds: effect.startSeconds, durationSeconds: clamp(effect.durationSeconds + deltaSeconds, MIN_EFFECT_DURATION_SECONDS, document.durationSeconds - effect.startSeconds), laneIndex });
        } else {
          setDraft({ id: effect.id, startSeconds: clamp(current.originalStartSeconds + deltaSeconds, 0, Math.max(0, document.durationSeconds - effect.durationSeconds)), durationSeconds: effect.durationSeconds, laneIndex });
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
          if (!current.active) {
            setMarkDrafts(new Map());
            setDraft(null);
            setGroupDraft([]);
            return;
          }
          const deltaSeconds = roundToNanosecond((event.nativeEvent.offsetX - current.startX) / viewport.pxPerSecond);
          const activeSelection = sequenceSelectionRef.current;
          if (activeSelection?.type === "marks" && activeSelection.marks.some((mark) => mark.collectionKey === current.collectionKey && mark.index === current.index)) {
            const constrainedDelta = constrainMarkDelta(document, activeSelection.marks, deltaSeconds);
            if (constrainedDelta === 0) {
              setMarkDrafts(new Map());
              return;
            }
            void runGuiEditCommand(() => commands.applySequenceSelectionEdit({
              type: "moveMarks",
              marks: activeSelection.marks,
              timeDeltaSeconds: constrainedDelta
            }).then((result) => {
              updateSequenceSelection(result.selection);
              setSelected(null);
              setMarkDrafts(new Map());
              return result;
            }));
            return;
          }
          const timeSeconds = clamp(current.originalTimeSeconds + deltaSeconds, 0, document.durationSeconds);
          if (timeSeconds === current.originalTimeSeconds) {
            setMarkDrafts(new Map());
            return;
          }
          const collection = document.markCollections.find((candidate) => candidate.key === current.collectionKey);
          const nextIndex = collection === undefined ? current.index : markIndexAfterMove(collection, current.index, timeSeconds);
          void runGuiEditCommand(() =>
            commands.applySequenceGuiEdit({
              type: "moveMark",
              collectionKey: current.collectionKey,
              index: current.index,
              timeSeconds
            })
          ).then(() => {
            setSelected({ type: "mark", collectionKey: current.collectionKey, index: nextIndex });
            setMarkDrafts(new Map());
          });
          return;
        }
        if (!current || current.kind !== "sequence") return;
        if (!current.active) {
          setDraft(null);
          setGroupDraft([]);
          return;
        }
        const activeSelection = sequenceSelectionRef.current;
        if (activeSelection?.type === "effects" && activeSelection.ids.length > 1 && activeSelection.ids.includes(current.id)) {
          const deltaSeconds = roundToNanosecond((event.nativeEvent.offsetX - current.startX) / viewport.pxPerSecond);
          const rawLaneIndex = clamp(Math.floor((event.nativeEvent.offsetY - top + viewport.scrollY) / viewport.laneHeight), 0, document.lanes.length - 1);
          const laneDelta = current.resize === "none" ? constrainEffectLaneDelta(document, activeSelection.ids, rawLaneIndex - current.laneIndex) : 0;
          const edit = current.resize === "none"
            ? { type: "moveEffects" as const, ids: activeSelection.ids, timeDeltaSeconds: constrainEffectMoveDelta(document, activeSelection.ids, deltaSeconds), laneDelta }
            : { type: "resizeEffects" as const, ids: activeSelection.ids, edge: current.resize, timeDeltaSeconds: constrainEffectResizeDelta(document, activeSelection.ids, current.resize, deltaSeconds) };
          if ((edit.type === "moveEffects" && edit.timeDeltaSeconds === 0 && edit.laneDelta === 0) || (edit.type === "resizeEffects" && edit.timeDeltaSeconds === 0)) {
            setDraft(null);
            setGroupDraft([]);
            return;
          }
          void runGuiEditCommand(() => commands.applySequenceSelectionEdit(edit).then((result) => {
            updateSequenceSelection(result.selection);
            setSelected(null);
            setDraft(null);
            setGroupDraft([]);
            return result;
          }));
          return;
        }
        const effect = document.effects.find((candidate) => candidate.id === current.id);
        if (effect === undefined) {
          setDraft(null);
          setGroupDraft([]);
          return;
        }
        const deltaSeconds = roundToNanosecond((event.nativeEvent.offsetX - current.startX) / viewport.pxPerSecond);
        const laneIndex =
          current.resize === "none"
            ? clamp(Math.floor((event.nativeEvent.offsetY - top + viewport.scrollY) / viewport.laneHeight), 0, document.lanes.length - 1)
            : current.laneIndex;
        const committedDraft =
          current.resize === "left"
            ? {
                id: effect.id,
                startSeconds: clamp(current.originalStartSeconds + deltaSeconds, 0, effect.startSeconds + effect.durationSeconds - MIN_EFFECT_DURATION_SECONDS),
                durationSeconds: effect.startSeconds + effect.durationSeconds - clamp(current.originalStartSeconds + deltaSeconds, 0, effect.startSeconds + effect.durationSeconds - MIN_EFFECT_DURATION_SECONDS),
                laneIndex
              }
            : current.resize === "right"
              ? {
                  id: effect.id,
                  startSeconds: effect.startSeconds,
                  durationSeconds: clamp(effect.durationSeconds + deltaSeconds, MIN_EFFECT_DURATION_SECONDS, document.durationSeconds - effect.startSeconds),
                  laneIndex
                }
              : {
                  id: effect.id,
                  startSeconds: clamp(current.originalStartSeconds + deltaSeconds, 0, Math.max(0, document.durationSeconds - effect.durationSeconds)),
                  durationSeconds: effect.durationSeconds,
                  laneIndex
                };
        const target = document.lanes[committedDraft.laneIndex]?.target ?? null;
        const isNoOp =
          current.resize === "none"
            ? committedDraft.startSeconds === effect.startSeconds && (target === null || targetsEqual(effect.target, target))
            : committedDraft.startSeconds === effect.startSeconds && committedDraft.durationSeconds === effect.durationSeconds;
        if (isNoOp) {
          setDraft(null);
          setGroupDraft([]);
          return;
        }
        const edit = () =>
          current.resize === "none"
            ? commands.applySequenceGuiEdit({
                type: "moveEffect",
                id: committedDraft.id,
                startSeconds: committedDraft.startSeconds,
                target
              })
            : commands.applySequenceGuiEdit({
                type: "resizeEffect",
                id: committedDraft.id,
                startSeconds: committedDraft.startSeconds,
                durationSeconds: committedDraft.durationSeconds
              });
        void runGuiEditCommand(edit).finally(() => {
          setDraft(null);
          setGroupDraft([]);
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
            const nextLaneHeight = clamp(current.laneHeight * Math.exp(-event.deltaY * SEQUENCE_CANVAS.wheelZoomScale), SEQUENCE_CANVAS.minLaneHeightPx, SEQUENCE_CANVAS.maxLaneHeightPx);
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
            const nextPxPerSecond = clamp(
              current.pxPerSecond * Math.exp(-event.deltaY * SEQUENCE_CANVAS.wheelZoomScale),
              minSequencePxPerSecond(timelineWidth, document.durationSeconds),
              SEQUENCE_CANVAS.maxZoomPxPerSecond
            );
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
                          key={`${script.script.path}:${script.script.effectName}`}
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
                  void runSnapshotCommand(() => commands.audioSeek(sequenceContextMenu.startSeconds));
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

type WaveformAudio = { durationSeconds: number; sampleRate: number; levels: WaveformLevel[] };

type WaveformLevel = { samplesPerPeak: number; mins: Float32Array; maxes: Float32Array };

type WaveformState = { key: string | null; audio: WaveformAudio | null };

type ClipRasterState = {
  requestKey: string;
  projectRevision: number | null;
  rasters: Map<string, DecodedClipRaster>;
  expectedRasterKeys: Map<number, string>;
  errors: Set<number>;
};

type DecodedClipRaster = {
  signature: string;
  image: CanvasImageSource;
  columns: number;
  rows: number;
  requestRows: number;
  byteLength: number;
  lastUsed: number;
};

type QueuedClipRasterDecode = {
  payload: SequenceClipRaster;
  keyContext: ClipRasterKeyContext;
};

const CLIP_RASTER_REQUEST_THROTTLE_MS = 50;
const CLIP_RASTER_DECODE_CHUNK_SIZE = 2;
const CLIP_RASTER_DECODED_BYTE_BUDGET = 64 * 1024 * 1024;
const WAVEFORM_CACHE_LIMIT = 4;

const waveformCache = new Map<string, { request: Promise<WaveformAudio | null>; lastUsed: number }>();
let waveformCacheAccess = 1;

type ClipRasterRequestItem = { effectId: number; displayColumnCount: number; requestedColumns: number; requestedRows: number };
type ClipRasterKeyContext = { projectRevision: number; rasterSettingsKey: string; requestedColumns: number; requestedRows: number };

function useSequenceClipRasters(
  document: SequenceEditorDocument,
  visibleClips: SequenceClipLayout[],
  laneHeight: number,
  settings: AppSettings | null
): ClipRasterState {
  const projectRevision = useAppStore((store) => store.snapshot?.projectRevision ?? null);
  const requestKey = `${document.path}:${document.objectKey}`;
  const rasterSettings = settings?.effectRaster ?? {
    renderScale: 1,
    maxColumns: 256,
    maxRows: 50,
    minFrameStride: 4
  };
  const rasterSettingsKey = `${rasterSettings.renderScale}:${rasterSettings.maxColumns}:${rasterSettings.maxRows}:${rasterSettings.minFrameStride}`;
  const rasterRequestKey = `${requestKey}:${rasterSettingsKey}`;
  const effectIds = useMemo(() => document.effects.map((effect) => effect.id), [document.effects]);
  const effectIdsKey = effectIds.join(",");
  const visibleRequestItems = useMemo(() => {
    const dpr = (window.devicePixelRatio || 1) * rasterSettings.renderScale;
    const displayRowCount = Math.max(1, Math.ceil(laneHeight * dpr));
    const items: ClipRasterRequestItem[] = [];
    const requested = new Set<number>();
    for (const clip of visibleClips) {
      if (requested.has(clip.effect.id)) continue;
      const displayColumnCount = Math.max(1, Math.ceil(clip.rect.width * dpr));
      const durationFrames = Math.max(1, clip.effect.durationSeconds * document.frameRate);
      items.push({
        effectId: clip.effect.id,
        displayColumnCount,
        requestedColumns: Math.min(displayColumnCount, Math.ceil(durationFrames / rasterSettings.minFrameStride), rasterSettings.maxColumns),
        requestedRows: displayRowCount
      });
      requested.add(clip.effect.id);
    }
    return items;
  }, [document.frameRate, laneHeight, rasterSettings.maxColumns, rasterSettings.minFrameStride, rasterSettings.renderScale, visibleClips]);
  const visibleRequestItemsKey = visibleRequestItems.map((item) => `${item.effectId}:${item.displayColumnCount}:${item.requestedColumns}:${item.requestedRows}`).join(",");
  const visibleRequestItemsRef = useRef<ClipRasterRequestItem[]>(visibleRequestItems);
  const rasters = useRef<Map<string, DecodedClipRaster>>(new Map());
  const expectedRasterKeys = useRef<Map<number, string>>(new Map());
  const rasterCacheAccess = useRef(1);
  const errors = useRef<Set<number>>(new Set());
  const cachedRequestKey = useRef(rasterRequestKey);
  const cachedProjectRevision = useRef(projectRevision);
  const projectRevisionRef = useRef(projectRevision);
  const [state, setState] = useState<ClipRasterState>({
    requestKey: rasterRequestKey,
    projectRevision,
    rasters: new Map(),
    expectedRasterKeys: new Map(),
    errors: new Set()
  });

  useEffect(() => {
    visibleRequestItemsRef.current = visibleRequestItems;
  }, [visibleRequestItems]);

  useEffect(() => {
    projectRevisionRef.current = projectRevision;
  }, [projectRevision]);

  useEffect(() => {
    if (projectRevision === null) return;
    const abortController = new AbortController();
    let pollTimeout: number | null = null;
    let requestTimeout: number | null = null;
    let decodeFrame: number | null = null;
    const decodeQueue: QueuedClipRasterDecode[] = [];
    let decoding = false;
    const displayRowCount = Math.max(1, Math.ceil(laneHeight * (window.devicePixelRatio || 1) * rasterSettings.renderScale));
    if (cachedProjectRevision.current !== projectRevision) {
      cachedProjectRevision.current = projectRevision;
      expectedRasterKeys.current.clear();
      errors.current.clear();
    }
    if (cachedRequestKey.current !== rasterRequestKey) {
      cachedRequestKey.current = rasterRequestKey;
      rasters.current.clear();
      expectedRasterKeys.current.clear();
      errors.current.clear();
    }
    const effectIdSet = new Set(effectIds);
    for (const [effectId, rasterKey] of [...expectedRasterKeys.current]) {
      if (!effectIdSet.has(effectId)) {
        expectedRasterKeys.current.delete(effectId);
        rasters.current.delete(rasterKey);
      }
    }
    for (const effectId of [...errors.current]) {
      if (!effectIdSet.has(effectId)) errors.current.delete(effectId);
    }

    const publishState = (nextProjectRevision: number) => {
      setState({
        requestKey: rasterRequestKey,
        projectRevision: nextProjectRevision,
        rasters: new Map(rasters.current),
        expectedRasterKeys: new Map(expectedRasterKeys.current),
        errors: new Set(errors.current)
      });
    };
    if (cachedRequestKey.current === rasterRequestKey && rasters.current.size === 0 && expectedRasterKeys.current.size === 0 && errors.current.size === 0) {
      publishState(projectRevision);
    }

    const scheduleDecode = (nextProjectRevision: number) => {
      if (decoding) return;
      decoding = true;
      const decodeNextChunk = async () => {
        if (clipRasterRequestCancelled(abortController.signal)) return;
        for (let index = 0; index < CLIP_RASTER_DECODE_CHUNK_SIZE; index += 1) {
          const queued = decodeQueue.shift();
          if (queued === undefined) break;
          const raster = queued.payload;
          try {
            const image = await decodeClipRaster(raster);
            if (clipRasterRequestCancelled(abortController.signal)) return;
            if (!Object.is(nextProjectRevision, projectRevisionRef.current)) return;
            const rasterKey = clipRasterKey(document.path, document.objectKey, raster.effectId, raster.signature, queued.keyContext);
            rasters.current.set(rasterKey, {
              signature: raster.signature,
              image,
              columns: raster.columns,
              rows: raster.rows,
              requestRows: queued.keyContext.requestedRows,
              byteLength: raster.columns * raster.rows * 4,
              lastUsed: rasterCacheAccess.current++
            });
            expectedRasterKeys.current.set(raster.effectId, rasterKey);
            evictDecodedClipRasters(rasters.current, new Set(expectedRasterKeys.current.values()));
            errors.current.delete(raster.effectId);
          } catch {
            const rasterKey = expectedRasterKeys.current.get(raster.effectId);
            if (rasterKey !== undefined) rasters.current.delete(rasterKey);
            expectedRasterKeys.current.delete(raster.effectId);
            errors.current.add(raster.effectId);
          }
        }
        publishState(nextProjectRevision);
        if (decodeQueue.length === 0) {
          decoding = false;
          decodeFrame = null;
          return;
        }
        decodeFrame = window.requestAnimationFrame(() => void decodeNextChunk());
      };
      decodeFrame = window.requestAnimationFrame(() => void decodeNextChunk());
    };

    const visibleRequestRasterItems = () => {
      const existingEffectIds = new Set(effectIds);
      const requested = new Set<number>();
      const items: ClipRasterRequestItem[] = [];
      for (const item of visibleRequestItemsRef.current) {
        if (!existingEffectIds.has(item.effectId) || requested.has(item.effectId)) continue;
        const rasterKey = expectedRasterKeys.current.get(item.effectId);
        const raster = rasterKey === undefined ? undefined : rasters.current.get(rasterKey);
        if (raster !== undefined) {
          raster.lastUsed = rasterCacheAccess.current++;
        }
        items.push(item);
        requested.add(item.effectId);
      }
      return items;
    };

    if (effectIds.length === 0) {
      publishState(projectRevision);
      return;
    }

    const pollResults = async (requestId: number, requestComplete: boolean, requestContexts: Map<number, ClipRasterKeyContext>): Promise<boolean> => {
      const batch = await commands.takeSequenceClipRasterResults({
        path: document.path,
        view: "sequence",
        objectKey: document.objectKey
      }, requestId);
      if (clipRasterRequestCancelled(abortController.signal)) return false;
      for (const raster of batch.ready) {
        const keyContext = requestContexts.get(raster.effectId);
        if (keyContext !== undefined) {
          decodeQueue.push({ payload: raster, keyContext });
        }
      }
      for (const error of batch.errors) {
        const rasterKey = expectedRasterKeys.current.get(error.effectId);
        if (rasterKey !== undefined) rasters.current.delete(rasterKey);
        expectedRasterKeys.current.delete(error.effectId);
        errors.current.add(error.effectId);
      }
      for (const unavailable of batch.unavailable) {
        const rasterKey = expectedRasterKeys.current.get(unavailable.effectId);
        if (rasterKey !== undefined) rasters.current.delete(rasterKey);
        expectedRasterKeys.current.delete(unavailable.effectId);
        errors.current.delete(unavailable.effectId);
      }
      if (decodeQueue.length > 0) {
        scheduleDecode(batch.projectRevision);
      } else {
        publishState(batch.projectRevision);
      }
      const complete = requestComplete || batch.complete || batch.projectRevision !== projectRevision;
      if (!complete) {
        return await new Promise((resolve) => {
          pollTimeout = window.setTimeout(() => {
            void pollResults(requestId, false, requestContexts).then(resolve);
          }, 100);
        });
      }
      return batch.projectRevision === projectRevision;
    };

    const requestRasters = async (requestItems: ClipRasterRequestItem[]): Promise<boolean> => {
      if (requestItems.length === 0) return true;
      const requestContexts = new Map<number, ClipRasterKeyContext>();
      const response = await commands.requestSequenceClipRasters({
        path: document.path,
        view: "sequence",
        objectKey: document.objectKey,
        items: requestItems.map((item) => {
          const rasterKeyContext = {
            projectRevision,
            rasterSettingsKey,
            requestedColumns: item.requestedColumns,
            requestedRows: item.requestedRows
          };
          requestContexts.set(item.effectId, rasterKeyContext);
          const expectedRasterKey = expectedRasterKeys.current.get(item.effectId) ?? null;
          const cached = expectedRasterKey === null ? null : rasters.current.get(expectedRasterKey) ?? null;
          const signature = cached !== null && cached.columns === item.requestedColumns && cached.requestRows === item.requestedRows ? cached.signature : null;
          if (signature !== null) {
            expectedRasterKeys.current.set(item.effectId, clipRasterKey(document.path, document.objectKey, item.effectId, signature, rasterKeyContext));
          }
          return { effectId: item.effectId, signature, displayColumnCount: item.displayColumnCount };
        }),
        displayRowCount
      });
      if (clipRasterRequestCancelled(abortController.signal)) return false;
      return await pollResults(response.requestId, response.complete, requestContexts);
    };

    requestTimeout = window.setTimeout(() => void requestRasters(visibleRequestRasterItems()), CLIP_RASTER_REQUEST_THROTTLE_MS);
    return () => {
      abortController.abort();
      if (pollTimeout !== null) window.clearTimeout(pollTimeout);
      window.clearTimeout(requestTimeout);
      if (decodeFrame !== null) window.cancelAnimationFrame(decodeFrame);
    };
  }, [document.objectKey, document.path, effectIds, effectIdsKey, laneHeight, projectRevision, rasterRequestKey, rasterSettings.renderScale, rasterSettingsKey, visibleRequestItemsKey]);

  return state.requestKey === rasterRequestKey ? state : {
    requestKey: rasterRequestKey,
    projectRevision,
    rasters: new Map(),
    expectedRasterKeys: new Map(),
    errors: new Set()
  };
}

function useSequenceWaveform(audio: SequenceAudio | null): WaveformState {
  const key = audio?.exists === true ? audio.resolvedPath : null;
  const [state, setState] = useState<WaveformState>({ key, audio: null });

  useEffect(() => {
    if (key === null) return;
    let cancelled = false;
    let cached = waveformCache.get(key);
    if (cached === undefined) {
      const request = decodeWaveformPeaks(key);
      cached = { request, lastUsed: waveformCacheAccess++ };
      waveformCache.set(key, cached);
      evictWaveformCache();
    } else {
      cached.lastUsed = waveformCacheAccess++;
    }
    const request = cached.request;
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

function clipRasterKey(
  path: string,
  objectKey: string | null,
  effectId: number,
  signature: string,
  context: ClipRasterKeyContext
): string {
  return JSON.stringify([
    path,
    objectKey,
    context.projectRevision,
    context.rasterSettingsKey,
    effectId,
    context.requestedColumns,
    context.requestedRows,
    signature
  ]);
}

function clipRasterRequestCancelled(signal: AbortSignal): boolean {
  return signal.aborted;
}

function evictDecodedClipRasters(rasters: Map<string, DecodedClipRaster>, protectedRasterKeys: Set<string>) {
  let byteLength = 0;
  for (const raster of rasters.values()) {
    byteLength += raster.byteLength;
  }
  while (byteLength > CLIP_RASTER_DECODED_BYTE_BUDGET) {
    let evictRasterKey: string | null = null;
    let oldest = Number.POSITIVE_INFINITY;
    for (const [rasterKey, raster] of rasters) {
      if (protectedRasterKeys.has(rasterKey)) continue;
      if (raster.lastUsed < oldest) {
        oldest = raster.lastUsed;
        evictRasterKey = rasterKey;
      }
    }
    if (evictRasterKey === null) return;
    const raster = rasters.get(evictRasterKey);
    if (raster === undefined) return;
    byteLength -= raster.byteLength;
    rasters.delete(evictRasterKey);
  }
}

function evictWaveformCache() {
  while (waveformCache.size > WAVEFORM_CACHE_LIMIT) {
    let evictKey: string | null = null;
    let oldest = Number.POSITIVE_INFINITY;
    for (const [key, entry] of waveformCache) {
      if (entry.lastUsed < oldest) {
        oldest = entry.lastUsed;
        evictKey = key;
      }
    }
    if (evictKey === null) return;
    waveformCache.delete(evictKey);
  }
}

function drawClipRaster(
  ctx: CanvasRenderingContext2D,
  raster: DecodedClipRaster,
  rect: { x: number; y: number; width: number; height: number }
) {
  ctx.imageSmoothingEnabled = false;
  ctx.drawImage(raster.image, rect.x, rect.y, rect.width, rect.height);
  ctx.imageSmoothingEnabled = true;
}

async function decodeClipRaster(payload: SequenceClipRaster): Promise<CanvasImageSource> {
  const raster = window.document.createElement("canvas");
  raster.width = payload.columns;
  raster.height = payload.rows;
  const rasterContext = raster.getContext("2d");
  if (rasterContext === null) throw new Error("Raster canvas context is unavailable.");
  const response = await fetch(convertFileSrc(payload.pixelsRgbaToken, "dawn-raster"));
  if (!response.ok) throw new Error(`Raster byte fetch failed with status ${response.status}.`);
  const arrayBuffer = await response.arrayBuffer();
  const expectedByteLength = payload.columns * payload.rows * 4;
  if (arrayBuffer.byteLength !== expectedByteLength) {
    throw new Error(`Raster byte length ${arrayBuffer.byteLength} did not match expected length ${expectedByteLength}.`);
  }
  const image = new ImageData(new Uint8ClampedArray(arrayBuffer), payload.columns, payload.rows);
  rasterContext.putImageData(image, 0, 0);
  return raster;
}

function scheduleSequenceViewportStateSave(path: string, objectKey: string, state: PersistedSequenceViewportState) {
  window.clearTimeout(sequenceViewportStateTimer);
  sequenceViewportStateTimer = window.setTimeout(() => {
    void commands.saveSequenceViewportState({ path, objectKey, state });
  }, 250);
}

function sequenceViewportFromPersisted(state: PersistedSequenceViewportState | undefined, settings: AppSettings | null): SequenceViewport {
  if (state === undefined) {
    return {
      pxPerSecond: settings?.sequenceInitialPxPerSecond ?? SEQUENCE_CANVAS.initialPxPerSecond,
      laneHeight: initialSequenceLaneHeight(settings),
      scrollXSeconds: 0,
      scrollY: 0
    };
  }
  return {
    pxPerSecond: clamp(state.pxPerSecond, SEQUENCE_CANVAS.minPxPerSecond, SEQUENCE_CANVAS.maxZoomPxPerSecond),
    laneHeight: clamp(state.laneHeight, SEQUENCE_CANVAS.minLaneHeightPx, SEQUENCE_CANVAS.maxLaneHeightPx),
    scrollXSeconds: Math.max(0, state.scrollXSeconds),
    scrollY: Math.max(0, state.scrollY)
  };
}

function initialSequencePxPerSecond(settings: AppSettings | null, timelineWidth: number, durationSeconds: number): number {
  const minPxPerSecond = minSequencePxPerSecond(timelineWidth, durationSeconds);
  if (settings?.sequenceInitialZoomMode === "fixedPxPerSecond") {
    return clamp(settings.sequenceInitialPxPerSecond, minPxPerSecond, SEQUENCE_CANVAS.maxPxPerSecond);
  }
  return clamp(minPxPerSecond, minPxPerSecond, SEQUENCE_CANVAS.maxPxPerSecond);
}

function minSequencePxPerSecond(timelineWidth: number, durationSeconds: number): number {
  if (!Number.isFinite(durationSeconds) || durationSeconds <= 0) {
    return SEQUENCE_CANVAS.minPxPerSecond;
  }
  return Math.max(SEQUENCE_CANVAS.minPxPerSecond, timelineWidth / durationSeconds);
}

function initialSequenceLaneHeight(settings: AppSettings | null): number {
  return clamp(settings?.sequenceInitialLaneHeightPx ?? SEQUENCE_CANVAS.initialLaneHeightPx, SEQUENCE_CANVAS.minLaneHeightPx, SEQUENCE_CANVAS.maxLaneHeightPx);
}

function drawClipRasterWarning(
  ctx: CanvasRenderingContext2D,
  rect: { x: number; y: number; width: number; height: number }
) {
  const size = Math.min(12, Math.max(6, rect.height - 6));
  ctx.fillStyle = "rgb(17 18 20 / 72%)";
  ctx.fillRect(rect.x, rect.y, rect.width, rect.height);
  ctx.fillStyle = SEQUENCE_COLORS.warning;
  ctx.beginPath();
  ctx.moveTo(rect.x + rect.width - size - 4, rect.y + 4);
  ctx.lineTo(rect.x + rect.width - 4, rect.y + 4);
  ctx.lineTo(rect.x + rect.width - 4, rect.y + size + 4);
  ctx.closePath();
  ctx.fill();
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
  const baseSamplesPerPeak = displaySamplesPerPeak(buffer.sampleRate);
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

function displaySamplesPerPeak(sampleRate: number): number {
  return clamp(Math.round(sampleRate * 0.02), 512, 4096);
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
  ctx.strokeStyle = SEQUENCE_COLORS.grid;
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
    ctx.fillStyle = SEQUENCE_COLORS.accent;
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
    ctx.strokeStyle = labeled ? SEQUENCE_COLORS.timelineMajor : SEQUENCE_COLORS.timelineMinor;
    ctx.beginPath();
    ctx.moveTo(x + 0.5, labeled ? 0 : top);
    ctx.lineTo(x + 0.5, height);
    ctx.stroke();
    if (labeled) {
      ctx.fillStyle = SEQUENCE_COLORS.timelineLabel;
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

function isTextEntryElement(target: EventTarget | null) {
  return target instanceof HTMLInputElement || target instanceof HTMLTextAreaElement || target instanceof HTMLSelectElement;
}
