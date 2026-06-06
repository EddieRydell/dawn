import { convertFileSrc } from "@tauri-apps/api/core";

import * as ContextMenu from "@radix-ui/react-context-menu";

import { useCallback, useEffect, useMemo, useRef, useState, type MouseEvent } from "react";

import { Trash2 } from "lucide-react";

import { commands } from "../../../api";

import type { LayoutTargetDto, SequenceAudioDto, SequenceDocumentDto, SequenceEffectPreviewDto, SequenceEffectPreviewResultDto, SequenceEffectScopeDto, SequenceEffectScriptDto } from "../../../bindings";

import { runRuntimeCommand } from "../../../store";

import { clamp, formatSeconds, roundToNanosecond, type GuiFocus, type SequenceSelection } from "../shared";

import { defaultMarkColor, drawSequenceMarks, committedMarkPreviews, markIndexAfterMove, nextCollectionKey, useMarkDisplayMode, type PendingMark } from "./marks";

import { sequencePreviewSignatures, targetsEqual } from "./sequencePreviewSignatures";
import type { SequencePreviewClock } from "./SequenceTransportControls";

import { buildSequenceClipLayout, constrainEffectLaneDelta, constrainEffectMoveDelta, constrainEffectResizeDelta, constrainMarkDelta, effectMovePreviews, effectResizePreviews, hitSequence, hitSequenceMark, markMovePreviews, markRefLookup, mergeSequenceSelection, MIN_EFFECT_DURATION_SECONDS, nextEffectSelection, nextMarkSelection, normalizedRect, selectedEffectId, selectionCount, selectionFromMarqueeEffects, selectionFromMarqueeMarks, sequenceHoverEqual, setMarkPreview, singleEffectSelectionFocus, selectionFromSingle, type MarkPreviewLookup, type SequenceContextMenu, type SequenceHover, type SequenceMarquee, type SequencePreview, type SequenceViewport } from "./sequenceSelection";

const SEQUENCE_CANVAS = {
  leftGutterPx: 128,
  topPx: 66,
  audioStripTopPx: 28,
  initialPxPerSecond: 80,
  initialLaneHeightPx: 42,
  minPxPerSecond: 20,
  maxPxPerSecond: 600,
  maxZoomPxPerSecond: 12000,
  minLaneHeightPx: 24,
  maxLaneHeightPx: 120,
  wheelZoomScale: 0.002,
  scrubStepSeconds: 0.01,
  nudgeSeconds: 0.001,
  shiftedNudgeSeconds: 0.01
} as const;

const PREVIEW_REQUEST_THROTTLE_MS = 50;
const PREVIEW_CANVAS_DECODE_CHUNK_SIZE = 2;
const MAX_EFFECT_RASTER_BATCH = 64;

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

type SequenceDragState =
  | null
  | { kind: "sequence"; id: number; startX: number; originalStartSeconds: number; laneIndex: number; resize: "none" | "left" | "right" }
  | { kind: "mark"; collectionKey: string; index: number; startX: number; originalTimeSeconds: number }
  | { kind: "marquee"; state: SequenceMarquee }
  | { kind: "sequenceScrub" };

type SequenceClipboard =
  | {
      type: "effects";
      effects: Array<{
        script: NonNullable<SequenceDocumentDto["effects"][number]["scriptSource"]>;
        target: LayoutTargetDto;
        scope: SequenceEffectScopeDto;
        offsetSeconds: number;
      }>;
    }
  | { type: "marks"; marks: Array<{ collectionKey: string; offsetSeconds: number }> };

export function SequenceCanvas({
  document,
  previewPositionSeconds,
  previewHomeSeconds,
  previewClock,
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
  previewClock: SequencePreviewClock;
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
  const baseCanvas = useRef<HTMLCanvasElement | null>(null);
  const drag = useRef<SequenceDragState>(null);
  const sequenceClipboard = useRef<SequenceClipboard | null>(null);
  const sequenceSelectionRef = useRef<SequenceSelection>(sequenceSelection);
  const [preview, setPreview] = useState<SequencePreview | null>(null);
  const [groupPreview, setGroupPreview] = useState<SequencePreview[]>([]);
  const [markPreviews, setMarkPreviews] = useState<MarkPreviewLookup>(() => new Map());
  const [pendingMarks, setPendingMarks] = useState<PendingMark[]>([]);
  const [sequenceContextMenu, setSequenceContextMenu] = useState<SequenceContextMenu | null>(null);
  const [hover, setHover] = useState<SequenceHover>(null);
  const [dragCursor, setDragCursor] = useState<"grabbing" | null>(null);
  const [selectedLaneIndex, setSelectedLaneIndex] = useState<number | null>(null);
  const [selectedTimeSeconds, setSelectedTimeSeconds] = useState<number | null>(null);
  const [marquee, setMarquee] = useState<SequenceMarquee | null>(null);
  const [canvasSize, setCanvasSize] = useState({ width: 0, height: 0 });
  const [viewport, setViewport] = useState<SequenceViewport>({ pxPerSecond: SEQUENCE_CANVAS.initialPxPerSecond, laneHeight: SEQUENCE_CANVAS.initialLaneHeightPx, scrollXSeconds: 0, scrollY: 0 });
  const [previewImages, setPreviewImages] = useState<Map<number, SequencePreviewImage>>(() => new Map());
  const [previewRequestTick, setPreviewRequestTick] = useState(0);
  const previewImagesRef = useRef(previewImages);
  const pendingMarksRef = useRef(pendingMarks);
  const inFlightPreviewSignatures = useRef<Set<string>>(new Set());
  const activePreviewRequest = useRef<{
    key: string;
    requestId: number;
    signatures: Set<string>;
  } | null>(null);
  const nextPreviewRequestId = useRef(1);
  const nextPendingMarkId = useRef(1);
  const initializedViewportKey = useRef<string | null>(null);
  const left = SEQUENCE_CANVAS.leftGutterPx;
  const top = SEQUENCE_CANVAS.topPx;
  const audioStripTop = SEQUENCE_CANVAS.audioStripTopPx;
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
  const previewViewportKey = useMemo(
    () => [viewport.scrollXSeconds, viewport.scrollY, viewport.pxPerSecond, viewport.laneHeight, canvasSize.width, canvasSize.height].join(":"),
    [canvasSize.height, canvasSize.width, viewport.laneHeight, viewport.pxPerSecond, viewport.scrollXSeconds, viewport.scrollY]
  );
  const previewRequestKey = `${document.path}:${document.objectKey}`;

  const updateSequenceSelection = useCallback((selection: SequenceSelection) => {
    sequenceSelectionRef.current = selection;
    setSequenceSelection(selection);
  }, [setSequenceSelection]);

  const copySequenceSelection = useCallback((selection: Exclude<SequenceSelection, null>) => {
    if (selection.type === "effects") {
      const selectedEffects = document.effects.filter((effect) => selection.ids.includes(effect.id) && effect.scriptSource !== null);
      if (selectedEffects.length === 0) {
        sequenceClipboard.current = null;
        return;
      }
      const firstStartSeconds = Math.min(...selectedEffects.map((effect) => effect.startSeconds));
      sequenceClipboard.current = {
        type: "effects",
        effects: selectedEffects.map((effect) => ({
          script: effect.scriptSource as NonNullable<typeof effect.scriptSource>,
          target: effect.target,
          scope: effect.scope,
          offsetSeconds: effect.startSeconds - firstStartSeconds
        }))
      };
      return;
    }
    const selectedMarks = selection.marks
      .map((mark) => {
        const timeSeconds = document.markCollections.find((collection) => collection.key === mark.collectionKey)?.marksSeconds[mark.index];
        return timeSeconds === undefined ? null : { collectionKey: mark.collectionKey, timeSeconds };
      })
      .filter((mark): mark is { collectionKey: string; timeSeconds: number } => mark !== null);
    if (selectedMarks.length === 0) {
      sequenceClipboard.current = null;
      return;
    }
    const firstTimeSeconds = Math.min(...selectedMarks.map((mark) => mark.timeSeconds));
    sequenceClipboard.current = {
      type: "marks",
      marks: selectedMarks.map((mark) => ({
        collectionKey: mark.collectionKey,
        offsetSeconds: mark.timeSeconds - firstTimeSeconds
      }))
    };
  }, [document.effects, document.markCollections]);

  const deleteSequenceSelection = useCallback((selection: Exclude<SequenceSelection, null>) => {
    updateSequenceSelection(null);
    setSelected(null);
    return commands.applySequenceDocumentEdit(
      selection.type === "effects"
        ? { type: "deleteEffects", ids: selection.ids }
        : { type: "deleteMarks", marks: selection.marks }
    );
  }, [setSelected, updateSequenceSelection]);

  const pasteSequenceClipboard = useCallback(async () => {
    const clipboard = sequenceClipboard.current;
    if (clipboard === null) return;
    const anchorSeconds = selectedTimeSeconds ?? 0;
    if (clipboard.type === "marks") {
      for (const mark of clipboard.marks) {
        await commands.applySequenceDocumentEdit({
          type: "addMark",
          collectionKey: mark.collectionKey,
          timeSeconds: clamp(anchorSeconds + mark.offsetSeconds, 0, document.durationSeconds)
        });
      }
      updateSequenceSelection(null);
      setSelected(null);
      return;
    }
    for (const effect of clipboard.effects) {
      await commands.applySequenceDocumentEdit({
        type: "addEffect",
        script: effect.script,
        target: effect.target,
        scope: effect.scope,
        startSeconds: clamp(anchorSeconds + effect.offsetSeconds, 0, document.durationSeconds),
        markCollectionKey: activeMarkCollectionKey
      });
    }
    updateSequenceSelection(null);
    setSelected(null);
  }, [activeMarkCollectionKey, document.durationSeconds, selectedTimeSeconds, setSelected, updateSequenceSelection]);

  useEffect(() => {
    sequenceSelectionRef.current = sequenceSelection;
  }, [sequenceSelection]);

  useEffect(() => {
    previewImagesRef.current = previewImages;
  }, [previewImages]);

  useEffect(() => {
    pendingMarksRef.current = pendingMarks;
  }, [pendingMarks]);

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
          pxPerSecond: clamp(timelineWidth / Math.max(1, document.durationSeconds), SEQUENCE_CANVAS.minPxPerSecond, SEQUENCE_CANVAS.maxPxPerSecond),
          laneHeight: SEQUENCE_CANVAS.initialLaneHeightPx,
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

  const effectivePreview = preview !== null && sequencePreviewCommitted(document, preview) ? null : preview;
  const effectiveGroupPreview = useMemo(
    () => groupPreview.length > 0 && groupPreview.every((candidate) => sequencePreviewCommitted(document, candidate)) ? [] : groupPreview,
    [document, groupPreview]
  );
  const effectiveMarkPreviews = useMemo(
    () => committedMarkPreviews(document.markCollections, markPreviews),
    [document.markCollections, markPreviews]
  );
  const visibleClips = useMemo(
    () => buildSequenceClipLayout(document, effectiveGroupPreview.length > 0 ? effectiveGroupPreview : effectivePreview === null ? [] : [effectivePreview], viewport, left, top),
    [document, effectiveGroupPreview, effectivePreview, left, top, viewport]
  );
  const visiblePreviewEffectIds = useMemo(() => {
    if (canvasSize.width <= 0 || canvasSize.height <= 0) return [];
    const ids: number[] = [];
    const seen = new Set<number>();
    for (const clip of visibleClips
      .filter(
        (clip) =>
          clip.rect.x + clip.rect.width >= left &&
          clip.rect.x <= canvasSize.width &&
          clip.rect.y + clip.rect.height >= top &&
          clip.rect.y <= canvasSize.height
      )
      .sort((leftClip, rightClip) => leftClip.rect.y - rightClip.rect.y || leftClip.rect.x - rightClip.rect.x)) {
      if (seen.has(clip.effect.id)) continue;
      seen.add(clip.effect.id);
      ids.push(clip.effect.id);
    }
    return ids;
  }, [canvasSize.height, canvasSize.width, left, top, visibleClips]);
  const prioritizedPreviewEffectIds = useMemo(() => {
    const ids = [...visiblePreviewEffectIds];
    const seen = new Set(ids);
    for (const effect of document.effects) {
      if (seen.has(effect.id)) continue;
      seen.add(effect.id);
      ids.push(effect.id);
    }
    return ids;
  }, [document.effects, visiblePreviewEffectIds]);
  const selectedEffectIds = useMemo(() => new Set<number>(sequenceSelection?.type === "effects" ? sequenceSelection.ids : []), [sequenceSelection]);
  const selectedMarks = useMemo(
    () => markRefLookup(sequenceSelection?.type === "marks" ? sequenceSelection.marks : []),
    [sequenceSelection]
  );
  const visiblePendingMarks = useMemo(
    () => pendingMarks.filter((mark) => visibleMarkCollectionKeys.has(mark.collectionKey)),
    [pendingMarks, visibleMarkCollectionKeys]
  );

  useEffect(() => {
    setPendingMarks((current) => {
      const next = current.filter((mark) => {
        const collection = document.markCollections.find((candidate) => candidate.key === mark.collectionKey);
        if (collection === undefined) return true;
        return markOccurrenceCount(collection.marksSeconds, mark.timeSeconds) < mark.occurrence;
      });
      pendingMarksRef.current = next;
      return next;
    });
  }, [document.markCollections]);

  useEffect(() => {
    const target = canvas.current;
    if (!target || canvasSize.width <= 0 || canvasSize.height <= 0 || prioritizedPreviewEffectIds.length === 0) return;
    if (activePreviewRequest.current !== null) return;

    const missingEffects = prioritizedPreviewEffectIds
      .map((id) => ({ id, signature: effectPreviewSignatures.get(id) }))
      .filter((effect): effect is { id: number; signature: string } => {
        if (effect.signature === undefined) return false;
        if (previewImagesRef.current.get(effect.id)?.signature === effect.signature) return false;
        return !inFlightPreviewSignatures.current.has(effect.signature);
      });
    if (missingEffects.length === 0) return;
    const batchEffects = missingEffects.slice(0, MAX_EFFECT_RASTER_BATCH);

    const request = { cancelled: false };
    const timeout = window.setTimeout(() => {
      if (previewRequestCancelled(request)) return;
      for (const effect of batchEffects) {
        inFlightPreviewSignatures.current.add(effect.signature);
      }
      const requestId = nextPreviewRequestId.current;
      nextPreviewRequestId.current += 1;
      activePreviewRequest.current = {
        key: previewRequestKey,
        requestId,
        signatures: new Set(batchEffects.map((effect) => effect.signature))
      };
      void commands
        .requestSequenceEffectPreviews(
          document.path,
          document.objectKey,
          requestId,
          batchEffects.map((effect) => ({ effectId: effect.id, signature: effect.signature }))
        )
        .catch(() => {
          if (previewRequestCancelled(request)) return;
          if (activePreviewRequest.current?.requestId === requestId) {
            activePreviewRequest.current = null;
          }
          for (const effect of batchEffects) {
            inFlightPreviewSignatures.current.delete(effect.signature);
          }
          setPreviewImages((current) => {
            const next = new Map(current);
            for (const effect of batchEffects) {
              if (effectPreviewSignaturesRef.current.get(effect.id) !== effect.signature) continue;
              next.set(effect.id, { signature: effect.signature, status: "unavailable" });
            }
            return next;
          });
        })
        .finally(() => {
          setPreviewRequestTick((tick) => tick + 1);
        });
    }, PREVIEW_REQUEST_THROTTLE_MS);

    return () => {
      request.cancelled = true;
      window.clearTimeout(timeout);
    };
  }, [canvasSize.height, canvasSize.width, document.objectKey, document.path, effectPreviewSignatures, previewRequestKey, previewRequestTick, previewViewportKey, prioritizedPreviewEffectIds]);

  const applyPreviewResults = useCallback(async (results: SequenceEffectPreviewResultDto[]) => {
    const ready: { preview: SequenceEffectPreviewDto; signature: string }[] = [];
    const unavailable: { effectId: number; signature: string }[] = [];
    for (const result of results) {
      const activeRequest = activePreviewRequest.current;
      if (activeRequest === null || result.requestId !== activeRequest.requestId || !activeRequest.signatures.has(result.signature)) {
        continue;
      }
      activeRequest.signatures.delete(result.signature);
      inFlightPreviewSignatures.current.delete(result.signature);
      if (result.type === "ready") {
        const effectId = result.preview.effectId;
        if (effectPreviewSignaturesRef.current.get(effectId) !== result.signature) continue;
        ready.push({ preview: result.preview, signature: result.signature });
      } else if (result.type === "unavailable") {
        if (effectPreviewSignaturesRef.current.get(result.effectId) !== result.signature) continue;
        unavailable.push({ effectId: result.effectId, signature: result.signature });
      } else {
        console.error(result.message);
        if (effectPreviewSignaturesRef.current.get(result.effectId) !== result.signature) continue;
        unavailable.push({ effectId: result.effectId, signature: result.signature });
      }
    }
    if (activePreviewRequest.current !== null && activePreviewRequest.current.signatures.size === 0) {
      activePreviewRequest.current = null;
    }
    if (unavailable.length > 0) {
      setPreviewImages((current) => {
        const next = new Map(current);
        for (const result of unavailable) {
          if (effectPreviewSignaturesRef.current.get(result.effectId) !== result.signature) continue;
          next.set(result.effectId, { signature: result.signature, status: "unavailable" });
        }
        return next;
      });
    }
    for (let index = 0; index < ready.length; index += PREVIEW_CANVAS_DECODE_CHUNK_SIZE) {
      const chunk = ready.slice(index, index + PREVIEW_CANVAS_DECODE_CHUNK_SIZE);
      const decoded = chunk.flatMap((result) => {
        if (effectPreviewSignaturesRef.current.get(result.preview.effectId) !== result.signature) return [];
        return [{
          effectId: result.preview.effectId,
          signature: result.signature,
          canvas: previewCanvasFromRaster(result.preview)
        }];
      });
      if (decoded.length > 0) {
        setPreviewImages((current) => {
          const next = new Map(current);
          for (const image of decoded) {
            if (effectPreviewSignaturesRef.current.get(image.effectId) !== image.signature) continue;
            next.set(image.effectId, {
              signature: image.signature,
              status: "ready",
              canvas: image.canvas
            });
          }
          return next;
        });
      }
      if (index + PREVIEW_CANVAS_DECODE_CHUNK_SIZE < ready.length) {
        await nextAnimationFrame();
      }
    }
    setPreviewRequestTick((tick) => tick + 1);
  }, []);

  useEffect(() => {
    let cancelled = false;
    let polling = false;
    const poll = async () => {
      const activeRequest = activePreviewRequest.current;
      if (polling || activeRequest === null || activeRequest.key !== previewRequestKey) return;
      polling = true;
      try {
        const batch = await commands.takeSequenceEffectPreviewResults(document.path, document.objectKey);
        if (!cancelled && batch.results.length > 0) {
          await applyPreviewResults(batch.results);
        }
      } finally {
        polling = false;
      }
    };
    const interval = window.setInterval(() => void poll(), PREVIEW_REQUEST_THROTTLE_MS);
    void poll();

    return () => {
      cancelled = true;
      window.clearInterval(interval);
    };
  }, [applyPreviewResults, document.objectKey, document.path, previewRequestKey]);

  useEffect(() => {
    const activeRequest = activePreviewRequest.current;
    if (activeRequest === null) return;
    for (const signature of activeRequest.signatures) {
      inFlightPreviewSignatures.current.delete(signature);
    }
    activePreviewRequest.current = null;
    setPreviewRequestTick((tick) => tick + 1);
  }, [effectPreviewSignatures, previewRequestKey, previewViewportKey]);

  useEffect(() => {
    const target = canvas.current;
    if (!target) return;
    let base = baseCanvas.current;
    if (!base) {
      base = window.document.createElement("canvas");
      baseCanvas.current = base;
    }
    const rect = target.getBoundingClientRect();
    const dpr = window.devicePixelRatio || 1;
    target.width = Math.max(1, Math.floor(rect.width * dpr));
    target.height = Math.max(1, Math.floor(rect.height * dpr));
    base.width = target.width;
    base.height = target.height;
    const ctx = base.getContext("2d");
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
    ctx.fillStyle = SEQUENCE_COLORS.textMuted;
    ctx.fillText(document.audio?.fileName ?? "Audio", 12, audioStripTop + audioStripHeight / 2 + 4);
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
      effectiveMarkPreviews,
      visiblePendingMarks
    );

    ctx.save();
    ctx.beginPath();
    ctx.rect(left, top, timelineWidth, rect.height - top);
    ctx.clip();
    for (const clip of visibleClips) {
      if (clip.rect.x + clip.rect.width < left || clip.rect.x > rect.width || clip.rect.y + clip.rect.height < top || clip.rect.y > rect.height) {
        continue;
      }
      ctx.fillStyle = SEQUENCE_COLORS.textFaint;
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
      ctx.strokeStyle = SEQUENCE_COLORS.clipBorder;
      ctx.lineWidth = 1;
      ctx.strokeRect(clip.rect.x + 0.5, clip.rect.y + 0.5, Math.max(0, clip.rect.width - 1), Math.max(0, clip.rect.height - 1));
    }
    ctx.restore();

    const visible = target.getContext("2d");
    if (!visible) return;
    visible.setTransform(dpr, 0, 0, dpr, 0, 0);
    visible.clearRect(0, 0, rect.width, rect.height);
    visible.drawImage(base, 0, 0, rect.width, rect.height);
  }, [document, effectPreviewSignatures, left, top, audioStripTop, audioStripHeight, viewport, visibleClips, selected, selectedMarks, previewImages, selectedLaneIndex, waveform.audio, visibleMarkCollections, mode, effectiveMarkPreviews, visiblePendingMarks]);

  useEffect(() => {
    const target = canvas.current;
    const base = baseCanvas.current;
    if (!target || !base) return;
    let frame = 0;
    const drawOverlay = () => {
      const rect = target.getBoundingClientRect();
      const dpr = window.devicePixelRatio || 1;
      const ctx = target.getContext("2d");
      if (!ctx) return;
      ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
      ctx.clearRect(0, 0, rect.width, rect.height);
      ctx.drawImage(base, 0, 0, rect.width, rect.height);
      ctx.font = "12px Inter, sans-serif";

      const timelineWidth = Math.max(1, rect.width - left);
      const maxScrollXSeconds = Math.max(0, document.durationSeconds - timelineWidth / viewport.pxPerSecond);
      const scrollXSeconds = clamp(viewport.scrollXSeconds, 0, maxScrollXSeconds);

    ctx.save();
    ctx.beginPath();
    ctx.rect(left, top, timelineWidth, rect.height - top);
    ctx.clip();
    for (const clip of visibleClips) {
      if (clip.rect.x + clip.rect.width < left || clip.rect.x > rect.width || clip.rect.y + clip.rect.height < top || clip.rect.y > rect.height) {
        continue;
      }
      const hoverResize = hover?.kind === "effect" && hover.effectId === clip.effect.id ? hover.resize : null;
      if (hoverResize !== null) {
        ctx.fillStyle = SEQUENCE_COLORS.overlay;
        ctx.fillRect(clip.rect.x, clip.rect.y, clip.rect.width, clip.rect.height);
      }
      const clipSelected = selectedEffectIds.has(clip.effect.id) || (selected?.type === "effect" && selected.id === clip.effect.id);
      if (clipSelected || hoverResize !== null) {
        ctx.strokeStyle = clipSelected ? SEQUENCE_COLORS.clipSelected : SEQUENCE_COLORS.clipHover;
        ctx.lineWidth = 2;
        ctx.strokeRect(clip.rect.x + 0.5, clip.rect.y + 0.5, Math.max(0, clip.rect.width - 1), Math.max(0, clip.rect.height - 1));
      }
      if (hoverResize === "left" || hoverResize === "right") {
        const handleX = hoverResize === "left" ? clip.rect.x : clip.rect.x + clip.rect.width;
        ctx.fillStyle = SEQUENCE_COLORS.warning;
        ctx.fillRect(handleX - 2, clip.rect.y + 4, 4, Math.max(4, clip.rect.height - 8));
      }
    }
    ctx.restore();

    const playheadX = left + (clamp(previewClock.currentPositionSeconds(), 0, document.durationSeconds) - scrollXSeconds) * viewport.pxPerSecond;
    const homeX = left + (clamp(previewClock.currentHomeSeconds(), 0, document.durationSeconds) - scrollXSeconds) * viewport.pxPerSecond;
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
      if (previewClock.isPlaying()) {
        frame = window.requestAnimationFrame(drawOverlay);
      }
    };
    drawOverlay();
    return () => {
      window.cancelAnimationFrame(frame);
    };
  }, [document.durationSeconds, left, top, viewport, visibleClips, selected, selectedEffectIds, previewPositionSeconds, previewHomeSeconds, previewClock, selectedTimeSeconds, marquee, hover]);

  const seekFromCanvas = (event: MouseEvent<HTMLCanvasElement>) => {
    const x = event.nativeEvent.offsetX;
    if (x < left) return;
    const positionSeconds = clamp(Math.round((viewport.scrollXSeconds + (x - left) / viewport.pxPerSecond) / SEQUENCE_CANVAS.scrubStepSeconds) * SEQUENCE_CANVAS.scrubStepSeconds, 0, document.durationSeconds);
    void runRuntimeCommand(() => commands.previewSeek(positionSeconds));
  };
  const timeFromCanvasX = (x: number) => clamp(roundToNanosecond(viewport.scrollXSeconds + (x - left) / viewport.pxPerSecond), 0, document.durationSeconds);
  const addEffectFromContextMenu = async (script: SequenceEffectScriptDto, menu: SequenceContextMenu) => {
    const hasMarksParams = script.params.some((param) => param.kind === "marks");
    let markCollectionKey = hasMarksParams ? activeMarkCollectionKey ?? document.markCollections[0]?.key ?? null : null;
    if (hasMarksParams && markCollectionKey === null) {
      const newCollectionKey = nextCollectionKey("Marks", document.markCollections);
      await runRuntimeCommand(() =>
        commands.applySequenceDocumentEdit({
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
    await runRuntimeCommand(() =>
      commands.applySequenceDocumentEdit({
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
      await runRuntimeCommand(() =>
        commands.applySequenceDocumentEdit({
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
    await addOptimisticMark(targetCollectionKey, menu.startSeconds);
  };
  const addMarkAtTime = async (timeSeconds: number) => {
    let collectionKey = activeMarkCollectionKey ?? document.markCollections[0]?.key ?? null;
    if (collectionKey === null) {
      const newCollectionKey = nextCollectionKey("Marks", document.markCollections);
      await runRuntimeCommand(() =>
        commands.applySequenceDocumentEdit({
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
    await addOptimisticMark(collectionKey, timeSeconds);
  };
  const addOptimisticMark = async (collectionKey: string, rawTimeSeconds: number) => {
    const timeSeconds = roundToNanosecond(clamp(rawTimeSeconds, 0, document.durationSeconds));
    const collection = document.markCollections.find((candidate) => candidate.key === collectionKey);
    if (collection === undefined) return;
    const existingOccurrence = markOccurrenceCount(collection.marksSeconds, timeSeconds);
    const pendingOccurrence = pendingMarksRef.current.filter((mark) => mark.collectionKey === collectionKey && mark.timeSeconds === timeSeconds).length;
    const pending: PendingMark = {
      id: nextPendingMarkId.current,
      collectionKey,
      color: collection.color,
      timeSeconds,
      occurrence: existingOccurrence + pendingOccurrence + 1
    };
    nextPendingMarkId.current += 1;
    setPendingMarks((current) => {
      const next = [...current, pending];
      pendingMarksRef.current = next;
      return next;
    });
    try {
      await runRuntimeCommand(() =>
        commands.applySequenceDocumentEdit({
          type: "addMark",
          collectionKey,
          timeSeconds
        })
      );
    } catch (error) {
      setPendingMarks((current) => {
        const next = current.filter((mark) => mark.id !== pending.id);
        pendingMarksRef.current = next;
        return next;
      });
      throw error;
    }
  };
  const deleteSelectedEffect = async (effectId: number) => {
    await runRuntimeCommand(() => commands.applySequenceDocumentEdit({ type: "deleteEffect", id: effectId }));
    setSelected(null);
    updateSequenceSelection(null);
  };
  const deleteContextMark = async (menu: Extract<SequenceContextMenu, { kind: "mark" }>) => {
    await runRuntimeCommand(() =>
      commands.applySequenceDocumentEdit({
        type: "deleteMark",
        collectionKey: menu.collectionKey,
        index: menu.index
      })
    );
    setSelected(null);
    updateSequenceSelection(null);
  };
  const retargetContextEffect = async (effectId: number, target: LayoutTargetDto) => {
    await runRuntimeCommand(() => commands.applySequenceDocumentEdit({ type: "retargetEffect", id: effectId, target }));
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
            copySequenceSelection(activeSelection);
            if (key === "x") {
              void runRuntimeCommand(() => deleteSequenceSelection(activeSelection));
            }
            return;
          }
          if (key === "v") {
            event.preventDefault();
            void runRuntimeCommand(pasteSequenceClipboard);
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
          const nextPreviews: MarkPreviewLookup = new Map();
          setMarkPreview(nextPreviews, selectedMark, { collectionKey: selectedMark.collectionKey, index: selectedMark.index, timeSeconds: nextTimeSeconds, committedIndex: nextIndex });
          setMarkPreviews(nextPreviews);
          void runRuntimeCommand(() =>
            commands.applySequenceDocumentEdit({
              type: "moveMark",
              collectionKey: selectedMark.collectionKey,
              index: selectedMark.index,
              timeSeconds: nextTimeSeconds
            })
          )
            .then(() => {
              setSelected({ type: "mark", collectionKey: selectedMark.collectionKey, index: nextIndex });
            })
            .catch(() => {
              setMarkPreviews(new Map());
            });
          return;
        }
        if ((event.key !== "Delete" && event.key !== "Backspace") || isTextEntryElement(event.target)) return;
        event.preventDefault();
        if (activeSelection !== null && selectionCount(activeSelection) > 1) {
          void runRuntimeCommand(() => deleteSequenceSelection(activeSelection));
          return;
        }
        if (focusedEffectId !== null) {
          void deleteSelectedEffect(focusedEffectId);
          return;
        }
        if (selectedMark === null) return;
        void runRuntimeCommand(() =>
          commands.applySequenceDocumentEdit({
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
        setMarkPreviews(new Map());
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
          setSelected({ type: "mark", collectionKey: mark.collectionKey, index: mark.index });
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
          setSelected({ type: "mark", collectionKey: current.collectionKey, index: current.index });
          const collection = document.markCollections.find((candidate) => candidate.key === current.collectionKey);
          const committedIndex = collection === undefined ? current.index : markIndexAfterMove(collection, current.index, timeSeconds);
          const activeSelection = sequenceSelectionRef.current;
          if (activeSelection?.type === "marks" && activeSelection.marks.length > 1 && activeSelection.marks.some((mark) => mark.collectionKey === current.collectionKey && mark.index === current.index)) {
            const constrainedDelta = constrainMarkDelta(document, activeSelection.marks, deltaSeconds);
            setMarkPreviews(markMovePreviews(document, activeSelection.marks, constrainedDelta));
          } else {
            const nextPreviews: MarkPreviewLookup = new Map();
            setMarkPreview(nextPreviews, { collectionKey: current.collectionKey, index: current.index }, { collectionKey: current.collectionKey, index: current.index, timeSeconds, committedIndex });
            setMarkPreviews(nextPreviews);
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
            const edits = [...markMovePreviews(document, activeSelection.marks, constrainedDelta).values()]
              .flatMap((collection) => [...collection.values()])
              .map((mark) => ({
                collectionKey: mark.collectionKey,
                index: mark.index,
                timeSeconds: mark.timeSeconds
              }));
            void runRuntimeCommand(() => commands.applySequenceDocumentEdit({ type: "moveMarks", edits }))
              .then(() => {
                updateSequenceSelection(null);
                setSelected(null);
              })
              .catch(() => {
                setMarkPreviews(new Map());
              });
            return;
          }
          const timeSeconds = clamp(current.originalTimeSeconds + deltaSeconds, 0, document.durationSeconds);
          const collection = document.markCollections.find((candidate) => candidate.key === current.collectionKey);
          const nextIndex = collection === undefined ? current.index : markIndexAfterMove(collection, current.index, timeSeconds);
          void runRuntimeCommand(() =>
            commands.applySequenceDocumentEdit({
              type: "moveMark",
              collectionKey: current.collectionKey,
              index: current.index,
              timeSeconds
            })
          )
            .then(() => {
              setSelected({ type: "mark", collectionKey: current.collectionKey, index: nextIndex });
            })
            .catch(() => {
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
            ? {
                type: "moveEffects" as const,
                edits: effectMovePreviews(document, activeSelection.ids, constrainEffectMoveDelta(document, activeSelection.ids, deltaSeconds), laneDelta).map((effect) => ({
                  id: effect.id,
                  startSeconds: effect.startSeconds,
                  target: document.lanes[effect.laneIndex]?.target ?? null
                }))
              }
            : {
                type: "resizeEffects" as const,
                edits: effectResizePreviews(document, activeSelection.ids, current.resize, constrainEffectResizeDelta(document, activeSelection.ids, current.resize, deltaSeconds)).map((effect) => ({
                  id: effect.id,
                  startSeconds: effect.startSeconds,
                  durationSeconds: effect.durationSeconds
                }))
              };
          void runRuntimeCommand(() => commands.applySequenceDocumentEdit(edit))
            .then(() => {
              updateSequenceSelection(null);
              setSelected(null);
            })
            .catch(() => {
              setPreview(null);
              setGroupPreview([]);
            });
          return;
        }
        if (!preview) return;
        const committedPreview = preview;
        const edit = () =>
          current.resize === "none"
            ? commands.applySequenceDocumentEdit({
                type: "moveEffect",
                id: committedPreview.id,
                startSeconds: committedPreview.startSeconds,
                target: document.lanes[committedPreview.laneIndex]?.target ?? null
              })
            : commands.applySequenceDocumentEdit({
                type: "resizeEffect",
                id: committedPreview.id,
                startSeconds: committedPreview.startSeconds,
                durationSeconds: committedPreview.durationSeconds
              });
        void runRuntimeCommand(edit).catch(() => {
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
            const nextPxPerSecond = clamp(current.pxPerSecond * Math.exp(-event.deltaY * SEQUENCE_CANVAS.wheelZoomScale), SEQUENCE_CANVAS.minPxPerSecond, SEQUENCE_CANVAS.maxZoomPxPerSecond);
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
                  void runRuntimeCommand(() => commands.previewSeek(sequenceContextMenu.startSeconds));
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

export type SequencePreviewImage = {
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

function nextAnimationFrame() {
  return new Promise<void>((resolve) => {
    window.requestAnimationFrame(() => {
      resolve();
    });
  });
}

function previewRequestCancelled(request: { cancelled: boolean }) {
  return request.cancelled;
}

function markOccurrenceCount(marksSeconds: number[], timeSeconds: number) {
  return marksSeconds.filter((markTimeSeconds) => roundToNanosecond(markTimeSeconds) === timeSeconds).length;
}

function sequencePreviewCommitted(document: SequenceDocumentDto, preview: SequencePreview) {
  const effect = document.effects.find((candidate) => candidate.id === preview.id);
  const lane = document.lanes[preview.laneIndex];
  return (
    effect !== undefined &&
    lane !== undefined &&
    Math.abs(effect.startSeconds - preview.startSeconds) <= 0.000000001 &&
    Math.abs(effect.durationSeconds - preview.durationSeconds) <= 0.000000001 &&
    targetsEqual(effect.target, lane.target)
  );
}

function validPreviewImage(image: SequencePreviewImage | undefined, signature: string | undefined) {
  if (image === undefined || signature === undefined) return undefined;
  return image.signature === signature ? image : undefined;
}

function isTextEntryElement(target: EventTarget | null) {
  return target instanceof HTMLInputElement || target instanceof HTMLTextAreaElement || target instanceof HTMLSelectElement;
}
