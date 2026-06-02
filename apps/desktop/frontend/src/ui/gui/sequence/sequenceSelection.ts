import type { SequenceDocumentDto, SequenceEffectDto, SequenceMarkCollectionDto, SequenceMarkRefDto, SequenceSelectionDto } from "../../../bindings";

import { clamp, type GuiFocus, type SequenceSelection } from "../shared";

import { markIndexAfterMove, type MarkDisplayMode } from "./marks";

import { targetsEqual } from "./sequencePreviewSignatures";

export type SequencePreview = { id: number; startSeconds: number; durationSeconds: number; laneIndex: number };

export type MarkPreview = { collectionKey: string; index: number; timeSeconds: number; committedIndex?: number };

export type MarkPreviewLookup = Map<string, Map<number, MarkPreview>>;

export type MarkRefLookup = Map<string, Set<number>>;

export type SequenceContextMenu =
  | { kind: "blank"; laneIndex: number; startSeconds: number }
  | { kind: "effect"; laneIndex: number; startSeconds: number; effectId: number }
  | { kind: "mark"; laneIndex: number; startSeconds: number; collectionKey: string; index: number };

export type SequenceHover =
  | null
  | { kind: "effect"; effectId: number; resize: "left" | "right" | "none" }
  | { kind: "mark"; collectionKey: string; index: number };

export type SequenceMarquee = { mode: "effects" | "marks"; startX: number; startY: number; x: number; y: number; active: boolean; shift: boolean; ctrl: boolean };

export const MIN_EFFECT_DURATION_SECONDS = 0.000000001;

const SEQUENCE_HIT_RADII = {
  effectResizeHandlePx: 8,
  markPx: 5
} as const;

export type SequenceViewport = {
  pxPerSecond: number;
  laneHeight: number;
  scrollXSeconds: number;
  scrollY: number;
};

export type SequenceClipLayout = {
  effect: SequenceEffectDto;
  laneIndex: number;
  rect: { x: number; y: number; width: number; height: number };
};

type SequenceClip = {
  effect: SequenceEffectDto;
  laneIndex: number;
};

type SequenceClipWithSlot = SequenceClip & { slot: number };

export type SequenceHit = {
  effect: SequenceEffectDto;
  laneIndex: number;
  resize: "left" | "right" | "none";
};

export type SequenceMarkHit = {
  collectionKey: string;
  index: number;
  timeSeconds: number;
};

export function buildSequenceClipLayout(
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

export function hitSequence(clips: SequenceClipLayout[], x: number, y: number): SequenceHit | null {
  for (const clip of [...clips].reverse()) {
    const { rect } = clip;
    if (x >= rect.x && x <= rect.x + rect.width && y >= rect.y && y <= rect.y + rect.height) {
      const resize: "left" | "right" | "none" =
        x - rect.x < SEQUENCE_HIT_RADII.effectResizeHandlePx ? "left" : rect.x + rect.width - x < SEQUENCE_HIT_RADII.effectResizeHandlePx ? "right" : "none";
      return {
        effect: clip.effect,
        laneIndex: clip.laneIndex,
        resize
      };
    }
  }
  return null;
}

export function hitSequenceMark(
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
      if (Math.abs(x - markX) <= SEQUENCE_HIT_RADII.markPx) {
        return { collectionKey: collection.key, index, timeSeconds };
      }
    }
  }
  return null;
}

export function sequenceHoverEqual(left: SequenceHover, right: SequenceHover) {
  if (left === right) return true;
  if (left === null || right === null || left.kind !== right.kind) return false;
  if (left.kind === "effect" && right.kind === "effect") {
    return left.effectId === right.effectId && left.resize === right.resize;
  }
  if (left.kind !== "mark" || right.kind !== "mark") return false;
  return left.collectionKey === right.collectionKey && left.index === right.index;
}

export function selectedEffectId(selected: GuiFocus): number | null {
  return selected?.type === "effect" ? selected.id : null;
}

export function selectionFromSingle(selected: GuiFocus): SequenceSelection {
  const effectId = selectedEffectId(selected);
  if (effectId !== null) return { type: "effects", ids: [effectId] };
  if (selected?.type === "mark") return { type: "marks", marks: [{ collectionKey: selected.collectionKey, index: selected.index }] };
  return null;
}

export function singleSelectionFocus(selection: SequenceSelection): GuiFocus {
  if (selection?.type === "effects") return singleEffectSelectionFocus(selection.ids);
  if (selection?.type === "marks" && selection.marks.length === 1) {
    const mark = selection.marks[0];
    return mark === undefined ? null : { type: "mark", collectionKey: mark.collectionKey, index: mark.index };
  }
  return null;
}

export function singleEffectSelectionFocus(ids: number[]): GuiFocus {
  if (ids.length !== 1) return null;
  const id = ids[0];
  return id === undefined ? null : { type: "effect", id };
}

export function selectionCount(selection: SequenceSelectionDto) {
  return selection.type === "effects" ? selection.ids.length : selection.marks.length;
}

export function selectionCompatibleWithFocusedItem(selection: SequenceSelectionDto, selected: GuiFocus) {
  const effectId = selectedEffectId(selected);
  if (effectId !== null) return selection.type === "effects" && selection.ids.includes(effectId);
  if (selected?.type === "mark") {
    const mark = { collectionKey: selected.collectionKey, index: selected.index };
    return selection.type === "marks" && markLookupHas(markRefLookup(selection.marks), mark);
  }
  return true;
}

export function nextEffectSelection(current: SequenceSelection, id: number, shift: boolean, ctrl: boolean): SequenceSelectionDto {
  if (current?.type !== "effects" || (!shift && !ctrl)) return { type: "effects", ids: [id] };
  const ids = new Set(current.ids);
  if (ctrl && ids.has(id)) ids.delete(id);
  else ids.add(id);
  return { type: "effects", ids: [...ids] };
}

export function nextMarkSelection(current: SequenceSelection, mark: SequenceMarkRefDto, shift: boolean, ctrl: boolean): SequenceSelectionDto {
  if (current?.type !== "marks" || (!shift && !ctrl)) return { type: "marks", marks: [mark] };
  const byCollection = markRefLookup(current.marks);
  if (ctrl && markLookupHas(byCollection, mark)) removeMarkRef(byCollection, mark);
  else addMarkRef(byCollection, mark);
  return { type: "marks", marks: markRefsFromLookup(byCollection) };
}

export function mergeSequenceSelection(current: SequenceSelection, next: SequenceSelectionDto, shift: boolean, ctrl: boolean): SequenceSelection {
  if ((!shift && !ctrl) || current?.type !== next.type) return next;
  if (next.type === "effects") {
    const ids = new Set(current.type === "effects" ? current.ids : []);
    for (const id of next.ids) {
      if (ctrl && ids.has(id)) ids.delete(id);
      else ids.add(id);
    }
    return { type: "effects", ids: [...ids] };
  }
  const marks = markRefLookup(current.type === "marks" ? current.marks : []);
  for (const mark of next.marks) {
    if (ctrl && markLookupHas(marks, mark)) removeMarkRef(marks, mark);
    else addMarkRef(marks, mark);
  }
  return { type: "marks", marks: markRefsFromLookup(marks) };
}

export function normalizedRect(startX: number, startY: number, x: number, y: number) {
  const left = Math.min(startX, x);
  const top = Math.min(startY, y);
  return { x: left, y: top, width: Math.abs(x - startX), height: Math.abs(y - startY) };
}

function rectsIntersect(left: { x: number; y: number; width: number; height: number }, right: { x: number; y: number; width: number; height: number }) {
  return left.x <= right.x + right.width && left.x + left.width >= right.x && left.y <= right.y + right.height && left.y + left.height >= right.y;
}

export function selectionFromMarqueeEffects(clips: SequenceClipLayout[], marquee: SequenceMarquee): SequenceSelectionDto {
  const box = normalizedRect(marquee.startX, marquee.startY, marquee.x, marquee.y);
  return { type: "effects", ids: clips.filter((clip) => rectsIntersect(box, clip.rect)).map((clip) => clip.effect.id) };
}

export function selectionFromMarqueeMarks(
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
      if (rectsIntersect(box, { x: x - SEQUENCE_HIT_RADII.markPx, y: y1, width: SEQUENCE_HIT_RADII.markPx * 2, height: y2 - y1 })) {
        marks.push({ collectionKey: collection.key, index });
      }
    });
  }
  return { type: "marks", marks };
}

export function constrainEffectMoveDelta(document: SequenceDocumentDto, ids: number[], deltaSeconds: number) {
  let minDelta = -Infinity;
  let maxDelta = Infinity;
  for (const effect of document.effects.filter((candidate) => ids.includes(candidate.id))) {
    minDelta = Math.max(minDelta, -effect.startSeconds);
    maxDelta = Math.min(maxDelta, document.durationSeconds - effect.durationSeconds - effect.startSeconds);
  }
  return clamp(deltaSeconds, minDelta, maxDelta);
}

export function constrainEffectLaneDelta(document: SequenceDocumentDto, ids: number[], laneDelta: number) {
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

export function effectMovePreviews(document: SequenceDocumentDto, ids: number[], deltaSeconds: number, laneDelta: number): SequencePreview[] {
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

export function effectResizePreviews(document: SequenceDocumentDto, ids: number[], edge: "left" | "right", deltaSeconds: number): SequencePreview[] {
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

export function constrainEffectResizeDelta(document: SequenceDocumentDto, ids: number[], edge: "left" | "right", deltaSeconds: number) {
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

export function constrainMarkDelta(document: SequenceDocumentDto, marks: SequenceMarkRefDto[], deltaSeconds: number) {
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

export function markMovePreviews(document: SequenceDocumentDto, marks: SequenceMarkRefDto[], deltaSeconds: number): MarkPreviewLookup {
  const previews: MarkPreviewLookup = new Map();
  for (const mark of marks) {
    const collection = document.markCollections.find((candidate) => candidate.key === mark.collectionKey);
    const timeSeconds = collection?.marksSeconds[mark.index];
    if (collection === undefined || timeSeconds === undefined) continue;
    const nextTimeSeconds = clamp(timeSeconds + deltaSeconds, 0, document.durationSeconds);
    setMarkPreview(previews, mark, {
      collectionKey: mark.collectionKey,
      index: mark.index,
      timeSeconds: nextTimeSeconds,
      committedIndex: markIndexAfterMove(collection, mark.index, nextTimeSeconds)
    });
  }
  return previews;
}

export function markSelectionConsumesKey(selected: GuiFocus, key: string) {
  return selected?.type === "mark" && (key === "ArrowLeft" || key === "ArrowRight");
}

export function markRefLookup(marks: SequenceMarkRefDto[]): MarkRefLookup {
  const lookup: MarkRefLookup = new Map();
  for (const mark of marks) {
    addMarkRef(lookup, mark);
  }
  return lookup;
}

function markLookupHas(lookup: MarkRefLookup, mark: SequenceMarkRefDto) {
  return lookup.get(mark.collectionKey)?.has(mark.index) ?? false;
}

function addMarkRef(lookup: MarkRefLookup, mark: SequenceMarkRefDto) {
  const collection = lookup.get(mark.collectionKey) ?? new Set<number>();
  collection.add(mark.index);
  lookup.set(mark.collectionKey, collection);
}

function removeMarkRef(lookup: MarkRefLookup, mark: SequenceMarkRefDto) {
  const collection = lookup.get(mark.collectionKey);
  if (collection === undefined) return;
  collection.delete(mark.index);
  if (collection.size === 0) lookup.delete(mark.collectionKey);
}

function markRefsFromLookup(lookup: MarkRefLookup): SequenceMarkRefDto[] {
  const marks: SequenceMarkRefDto[] = [];
  for (const [collectionKey, indexes] of lookup) {
    for (const index of indexes) {
      marks.push({ collectionKey, index });
    }
  }
  return marks;
}

export function getMarkPreview(lookup: MarkPreviewLookup, mark: SequenceMarkRefDto): MarkPreview | undefined {
  return lookup.get(mark.collectionKey)?.get(mark.index);
}

export function setMarkPreview(lookup: MarkPreviewLookup, mark: SequenceMarkRefDto, preview: MarkPreview) {
  const collection = lookup.get(mark.collectionKey) ?? new Map<number, MarkPreview>();
  collection.set(mark.index, preview);
  lookup.set(mark.collectionKey, collection);
}

export function markPreviewEntries(lookup: MarkPreviewLookup): MarkPreview[] {
  return [...lookup.values()].flatMap((collection) => [...collection.values()]);
}
