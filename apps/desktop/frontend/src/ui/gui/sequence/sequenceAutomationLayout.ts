import type { SequenceAutomationClip } from "../../../types";

import { clamp, roundToNanosecond } from "../shared";
import { THEME_COLORS, THEME_METRICS } from "../../../theme";
import type { SequenceClipLayout, SequenceViewport } from "./sequenceSelection";

export type AutomationDraft = {
  id: number;
  startSeconds: number;
  durationSeconds: number;
  anchorLaneIndex: number;
  laneIndex: number;
};

export type AutomationHover = { kind: "automation"; clipId: number; resize: "left" | "right" | "none" };

export type AutomationCurveDraft = {
  id: number;
  curve: Array<{ time: number; value: number }>;
};

export type AutomationClipLayout = {
  clip: SequenceAutomationClip;
  rect: { x: number; y: number; width: number; height: number };
};

export function automationLaneRowHeight(laneHeight: number): number {
  return Math.max(THEME_METRICS.automationRowMinHeight, laneHeight * 0.42);
}

export function automationRowCounts(clips: SequenceAutomationClip[], laneCount: number): number[] {
  const rows = Array.from({ length: laneCount }, () => 0);
  for (const clip of clips) {
    if (clip.anchorLaneIndex < 0 || clip.anchorLaneIndex >= laneCount) continue;
    rows[clip.anchorLaneIndex] = Math.max(rows[clip.anchorLaneIndex] ?? 0, clip.laneIndex + 1);
  }
  return rows;
}

export function expandedLaneTop(laneIndex: number, rowsByLane: number[], laneHeight: number, automationRowHeight: number): number {
  let top = 0;
  for (let index = 0; index < laneIndex; index += 1) {
    top += laneHeight + (rowsByLane[index] ?? 0) * automationRowHeight;
  }
  return top;
}

export function expandedLaneRowIndex(laneIndex: number, rowsByLane: number[]): number {
  let rowIndex = 0;
  for (let index = 0; index < laneIndex; index += 1) rowIndex += 1 + (rowsByLane[index] ?? 0);
  return rowIndex;
}

export function expandedTimelineHeight(laneCount: number, rowsByLane: number[], laneHeight: number, automationRowHeight: number): number {
  return expandedLaneTop(laneCount, rowsByLane, laneHeight, automationRowHeight);
}

export function laneIndexFromCanvasY(y: number, top: number, scrollY: number, laneCount: number, rowsByLane: number[], laneHeight: number, automationRowHeight: number): number {
  const contentY = Math.max(0, y - top + scrollY);
  let cursor = 0;
  for (let index = 0; index < laneCount; index += 1) {
    const blockHeight = laneHeight + (rowsByLane[index] ?? 0) * automationRowHeight;
    if (contentY < cursor + blockHeight) return index;
    cursor += blockHeight;
  }
  return Math.max(0, laneCount - 1);
}

export function remapEffectClipLayout(clip: SequenceClipLayout, rowsByLane: number[], automationRowHeight: number, viewport: SequenceViewport, top: number): SequenceClipLayout {
  const originalLaneTop = top + clip.laneIndex * viewport.laneHeight - viewport.scrollY;
  const laneLocalY = clip.rect.y - originalLaneTop;
  return {
    ...clip,
    rect: {
      ...clip.rect,
      y: top + expandedLaneTop(clip.laneIndex, rowsByLane, viewport.laneHeight, automationRowHeight) - viewport.scrollY + laneLocalY
    }
  };
}

export function buildAutomationClipLayout(clips: SequenceAutomationClip[], rowsByLane: number[], rowHeight: number, viewport: SequenceViewport, left: number, top: number, bounds: { width: number; height: number }): AutomationClipLayout[] {
  const visibleStartSeconds = viewport.scrollXSeconds;
  const visibleEndSeconds = viewport.scrollXSeconds + Math.max(1, bounds.width - left) / viewport.pxPerSecond;
  const byAutomationLane = new Map<string, SequenceAutomationClip[]>();
  for (const clip of clips) {
    if (clip.startSeconds + clip.durationSeconds < visibleStartSeconds || clip.startSeconds > visibleEndSeconds) continue;
    const key = `${clip.anchorLaneIndex}:${clip.laneIndex}`;
    const laneClips = byAutomationLane.get(key) ?? [];
    laneClips.push(clip);
    byAutomationLane.set(key, laneClips);
  }

  const layouts: AutomationClipLayout[] = [];
  for (const laneClips of byAutomationLane.values()) {
    for (const group of groupOverlappingAutomationClips(laneClips)) {
      const assigned = assignAutomationOverlapSlots(group);
      const slotCount = Math.max(1, Math.max(...assigned.map((clip) => clip.slot)) + 1);
      const slotHeight = rowHeight / slotCount;
      for (const clip of assigned) {
        const laneIndex = clip.anchorLaneIndex;
        const x = left + (clip.startSeconds - viewport.scrollXSeconds) * viewport.pxPerSecond;
        layouts.push({
          clip,
          rect: {
            x,
            y: top + expandedLaneTop(laneIndex, rowsByLane, viewport.laneHeight, rowHeight) - viewport.scrollY + viewport.laneHeight + clip.laneIndex * rowHeight + clip.slot * slotHeight + THEME_METRICS.sequenceClipSlotOffset,
            width: Math.max(THEME_METRICS.sequenceClipMinWidth, clip.durationSeconds * viewport.pxPerSecond),
            height: Math.max(THEME_METRICS.sequenceClipMinHeight, slotHeight - THEME_METRICS.sequenceClipHandleInset)
          }
        });
      }
    }
  }
  return layouts;
}

export function automationClipsWithDrafts(clips: SequenceAutomationClip[], draft: AutomationDraft | null, curveDraft: AutomationCurveDraft | null): SequenceAutomationClip[] {
  return clips.map((clip) =>
    clip.id === draft?.id
      ? { ...clip, startSeconds: draft.startSeconds, durationSeconds: draft.durationSeconds, anchorLaneIndex: draft.anchorLaneIndex, laneIndex: draft.laneIndex, curve: curveDraft?.id === clip.id ? curveDraft.curve : clip.curve }
      : curveDraft?.id === clip.id
        ? { ...clip, curve: curveDraft.curve }
        : clip
  );
}

export function automationHoverEqual(left: AutomationHover | null, right: AutomationHover | null) {
  if (left === right) return true;
  if (left === null || right === null) return false;
  return left.clipId === right.clipId && left.resize === right.resize;
}

function compareAutomationClipsByTime(left: SequenceAutomationClip, right: SequenceAutomationClip) {
  return left.startSeconds - right.startSeconds || left.startSeconds + left.durationSeconds - (right.startSeconds + right.durationSeconds) || left.id - right.id;
}

function groupOverlappingAutomationClips(clips: SequenceAutomationClip[]) {
  const sorted = [...clips].sort(compareAutomationClipsByTime);
  const groups: SequenceAutomationClip[][] = [];
  let current: SequenceAutomationClip[] = [];
  let currentEnd = -Infinity;
  for (const clip of sorted) {
    const end = clip.startSeconds + clip.durationSeconds;
    if (current.length === 0 || clip.startSeconds < currentEnd) {
      current.push(clip);
      currentEnd = Math.max(currentEnd, end);
    } else {
      groups.push(current);
      current = [clip];
      currentEnd = end;
    }
  }
  if (current.length > 0) groups.push(current);
  return groups;
}

function assignAutomationOverlapSlots(group: SequenceAutomationClip[]) {
  const slotEnds: number[] = [];
  return [...group].sort(compareAutomationClipsByTime).map((clip) => {
    const start = clip.startSeconds;
    const end = clip.startSeconds + clip.durationSeconds;
    let slot = slotEnds.findIndex((slotEnd) => slotEnd <= start);
    if (slot === -1) slot = slotEnds.length;
    slotEnds[slot] = end;
    return { ...clip, slot };
  });
}

export function hitAutomationClip(clips: AutomationClipLayout[], x: number, y: number) {
  for (const clip of [...clips].reverse()) {
    const { rect } = clip;
    if (x >= rect.x && x <= rect.x + rect.width && y >= rect.y && y <= rect.y + rect.height) {
      const resize: "left" | "right" | "none" = x - rect.x < THEME_METRICS.sequenceEffectResizeHitWidth ? "left" : rect.x + rect.width - x < THEME_METRICS.sequenceEffectResizeHitWidth ? "right" : "none";
      return { ...clip, resize };
    }
  }
  return null;
}

function automationCurveGraphRect(rect: { x: number; y: number; width: number; height: number }) {
  const padding = Math.min(THEME_METRICS.automationGraphPaddingMax, Math.max(THEME_METRICS.automationGraphPaddingMin, rect.height * THEME_METRICS.automationGraphPaddingRatio));
  return { x: rect.x + padding, y: rect.y + padding, width: Math.max(THEME_METRICS.visualMinSize, rect.width - padding * 2), height: Math.max(THEME_METRICS.visualMinSize, rect.height - padding * 2) };
}

export function sortAutomationCurve(curve: Array<{ time: number; value: number }>) {
  return [...curve].sort((left, right) => left.time - right.time);
}

function automationCurveCanvasPoints(curve: Array<{ time: number; value: number }>, rect: { x: number; y: number; width: number; height: number }) {
  const graph = automationCurveGraphRect(rect);
  return sortAutomationCurve(curve).map((point) => ({ x: graph.x + clamp(point.time, 0, 1) * graph.width, y: graph.y + (1 - clamp(point.value, 0, 1)) * graph.height }));
}

export function hitAutomationCurvePoint(clip: AutomationClipLayout, x: number, y: number): number | null {
  const points = automationCurveCanvasPoints(clip.clip.curve, clip.rect);
  for (let index = points.length - 1; index >= 0; index -= 1) {
    const point = points[index];
    if (point !== undefined && Math.hypot(point.x - x, point.y - y) <= 7) return index;
  }
  return null;
}

export function automationCurvePointFromCanvas(rect: { x: number; y: number; width: number; height: number }, x: number, y: number) {
  const graph = automationCurveGraphRect(rect);
  return { time: roundToNanosecond(clamp((x - graph.x) / graph.width, 0, 1)), value: Math.round(clamp(1 - (y - graph.y) / graph.height, 0, 1) * 1000) / 1000 };
}

export function replaceAutomationCurvePoint(curve: Array<{ time: number; value: number }>, index: number, point: { time: number; value: number }) {
  return curve.map((candidate, candidateIndex) => (candidateIndex === index ? point : candidate)).filter((candidate) => Number.isFinite(candidate.time) && Number.isFinite(candidate.value)).sort((left, right) => left.time - right.time);
}

export function removeAutomationCurvePoint(curve: Array<{ time: number; value: number }>, index: number) {
  return sortAutomationCurve(curve).filter((_, candidateIndex) => candidateIndex !== index).filter((candidate) => Number.isFinite(candidate.time) && Number.isFinite(candidate.value));
}

export function drawAutomationCurve(ctx: CanvasRenderingContext2D, clip: SequenceAutomationClip, rect: { x: number; y: number; width: number; height: number }) {
  const graph = automationCurveGraphRect(rect);
  ctx.fillStyle = THEME_COLORS.automationGraph;
  ctx.fillRect(graph.x, graph.y, graph.width, graph.height);
  ctx.lineWidth = THEME_METRICS.visualLineWidth;
  ctx.strokeStyle = THEME_COLORS.automationGraphGrid;
  ctx.beginPath();
  for (let column = 1; column < THEME_METRICS.automationGridColumns; column += 1) {
    const x = graph.x + (graph.width * column) / THEME_METRICS.automationGridColumns + THEME_METRICS.visualHairlineOffset;
    ctx.moveTo(x, graph.y); ctx.lineTo(x, graph.y + graph.height);
  }
  for (let row = 1; row < THEME_METRICS.automationGridRows; row += 1) {
    const y = graph.y + (graph.height * row) / THEME_METRICS.automationGridRows + THEME_METRICS.visualHairlineOffset;
    ctx.moveTo(graph.x, y); ctx.lineTo(graph.x + graph.width, y);
  }
  ctx.stroke();
  ctx.strokeStyle = THEME_COLORS.automationGraphMajorGrid;
  ctx.beginPath();
  ctx.moveTo(graph.x, graph.y + graph.height / 2 + THEME_METRICS.visualHairlineOffset);
  ctx.lineTo(graph.x + graph.width, graph.y + graph.height / 2 + THEME_METRICS.visualHairlineOffset);
  ctx.moveTo(graph.x + graph.width / 2 + THEME_METRICS.visualHairlineOffset, graph.y);
  ctx.lineTo(graph.x + graph.width / 2 + THEME_METRICS.visualHairlineOffset, graph.y + graph.height);
  ctx.stroke();
  ctx.strokeStyle = THEME_COLORS.sequenceGrid;
  ctx.strokeRect(graph.x + THEME_METRICS.visualHairlineOffset, graph.y + THEME_METRICS.visualHairlineOffset, Math.max(0, graph.width - THEME_METRICS.visualLineWidth), Math.max(0, graph.height - THEME_METRICS.visualLineWidth));
  if (clip.curve.length === 0) return;
  const points = automationCurveCanvasPoints(clip.curve, rect);
  ctx.strokeStyle = THEME_COLORS.accent;
  ctx.lineWidth = THEME_METRICS.automationCurveWidth;
  ctx.lineCap = "round"; ctx.lineJoin = "round"; ctx.beginPath();
  points.forEach((point, index) => { if (index === 0) ctx.moveTo(point.x, point.y); else ctx.lineTo(point.x, point.y); });
  ctx.stroke();
  ctx.lineCap = "butt"; ctx.lineJoin = "miter";
  for (const point of points) {
    ctx.fillStyle = THEME_COLORS.text; ctx.strokeStyle = THEME_COLORS.page; ctx.lineWidth = THEME_METRICS.visualLineWidthStrong;
    ctx.beginPath(); ctx.arc(point.x, point.y, THEME_METRICS.automationPointRadius, 0, Math.PI * 2); ctx.fill(); ctx.stroke();
  }
}
