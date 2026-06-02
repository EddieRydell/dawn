import { useEffect, useState } from "react";

import type { SequenceMarkCollectionDto, SequenceMarkRefDto } from "../../../bindings";

import type { MarkPreview, MarkPreviewLookup } from "./sequenceSelection";

export type MarkDisplayMode = "overlay" | "strip" | "hidden";

const DEFAULT_MARK_COLORS = ["#38bdf8", "#f97316", "#22c55e", "#e879f9", "#facc15", "#ef4444"];

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

export function committedMarkPreviews(collections: SequenceMarkCollectionDto[], previews: MarkPreviewLookup) {
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

export function parseSelectedMark(selected: string | null): { collectionKey: string; index: number } | null {
  if (selected === null || !selected.startsWith("mark:")) return null;
  const [, collectionKey, rawIndex] = selected.split(":");
  const index = Number(rawIndex);
  if (collectionKey === undefined || collectionKey.length === 0 || !Number.isInteger(index) || index < 0) return null;
  return { collectionKey, index };
}

export function markKey(mark: SequenceMarkRefDto) {
  return `${mark.collectionKey}:${mark.index}`;
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
