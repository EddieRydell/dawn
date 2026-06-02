import { useEffect, useState } from "react";

import type { SequenceMarkCollectionDto } from "../../../bindings";

import type { GuiFocus } from "../shared";

import { getMarkPreview, markPreviewEntries, setMarkPreview, type MarkPreviewLookup, type MarkRefLookup } from "./sequenceSelection";

export type MarkDisplayMode = "overlay" | "strip" | "hidden";

const DEFAULT_MARK_COLORS = ["#38bdf8", "#f97316", "#22c55e", "#e879f9", "#facc15", "#ef4444"];

const MARK_DRAWING = {
  cullPaddingPx: 6,
  selectedCapHalfWidthPx: 4,
  selectedStroke: "#fffaf0"
} as const;

let markDisplayMode: MarkDisplayMode = "overlay";

export function setGlobalMarkDisplayMode(nextMode: MarkDisplayMode) {
  markDisplayMode = nextMode;
  window.dispatchEvent(new CustomEvent<MarkDisplayMode>("dawn-mark-display-mode", { detail: nextMode }));
}

export function useMarkDisplayMode() {
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

export function drawSequenceMarks(
  ctx: CanvasRenderingContext2D,
  collections: SequenceMarkCollectionDto[],
  selected: GuiFocus,
  selectedMarks: MarkRefLookup,
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
      const mark = { collectionKey: collection.key, index };
      const preview = getMarkPreview(previews, mark);
      const drawnTimeSeconds = preview?.timeSeconds ?? timeSeconds;
      const x = left + (drawnTimeSeconds - scrollXSeconds) * pxPerSecond;
      if (x < left - MARK_DRAWING.cullPaddingPx || x > left + width + MARK_DRAWING.cullPaddingPx) continue;
      const isSelected =
        (selected?.type === "mark" && selected.collectionKey === collection.key && selected.index === index) ||
        (selectedMarks.get(collection.key)?.has(index) ?? false);
      ctx.strokeStyle = collection.color;
      ctx.lineWidth = isSelected ? 2 : 1;
      ctx.globalAlpha = mode === "strip" ? 0.95 : 0.75;
      ctx.beginPath();
      ctx.moveTo(x + 0.5, y1);
      ctx.lineTo(x + 0.5, y2);
      ctx.stroke();
      if (isSelected) {
        ctx.globalAlpha = 1;
        ctx.strokeStyle = MARK_DRAWING.selectedStroke;
        ctx.lineWidth = 1;
        ctx.beginPath();
        ctx.moveTo(x - MARK_DRAWING.selectedCapHalfWidthPx, y1 + 0.5);
        ctx.lineTo(x + MARK_DRAWING.selectedCapHalfWidthPx, y1 + 0.5);
        ctx.stroke();
      }
    }
  }
  ctx.restore();
}

export function committedMarkPreviews(collections: SequenceMarkCollectionDto[], previews: MarkPreviewLookup) {
  const next: MarkPreviewLookup = new Map();
  for (const preview of markPreviewEntries(previews)) {
    if (preview.committedIndex === undefined) {
      setMarkPreview(next, preview, preview);
      continue;
    }
    const collection = collections.find((candidate) => candidate.key === preview.collectionKey);
    if (collection?.marksSeconds[preview.committedIndex] !== preview.timeSeconds) {
      setMarkPreview(next, preview, preview);
    }
  }
  return next;
}

export function markIndexAfterMove(collection: SequenceMarkCollectionDto, index: number, timeSeconds: number) {
  const sorted = collection.marksSeconds
    .map((markTimeSeconds, markIndex) => ({
      markIndex,
      timeSeconds: markIndex === index ? timeSeconds : markTimeSeconds
    }))
    .sort((left, right) => left.timeSeconds - right.timeSeconds || left.markIndex - right.markIndex);
  return Math.max(0, sorted.findIndex((mark) => mark.markIndex === index));
}

export function nextCollectionKey(name: string, collections: SequenceMarkCollectionDto[]) {
  const used = new Set(collections.map((collection) => collection.key));
  const base = snakeCaseKey(name);
  if (!used.has(base)) return base;
  for (let suffix = 2; ; suffix += 1) {
    const key = `${base}_${suffix}`;
    if (!used.has(key)) return key;
  }
}

export function defaultMarkColor(index: number) {
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
