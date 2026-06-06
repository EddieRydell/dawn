import type { ActiveGuiDocumentDto, AppSnapshotDto, GeometryRenderBoundsDto, GeometryRenderPointDto, LayoutDocumentDto, LayoutFixturePlacementDto, Point3MetersDto, SequenceSelectionDto, TransformDto } from "../../bindings";

export type Point3 = { x: number; y: number; z: number };

export type Transform = { position: Point3; rotation: Point3; scale: Point3 };

export type PreviewTiming = {
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
  loopTotalMs: number;
  loopAccountedMs: number;
  loopUnaccountedMs: number;
  sleepActualMs: number;
  previewTransportLockMs: number;
  previewPublishLockMs: number;
  liveOutputLockMs: number;
  modelLockWaitMs: number;
  previewSnapshotMs: number;
  analysisHandleMs: number;
  audioPollMs: number;
  audioApplyMs: number;
  modelUpdateMs: number;
  renderMs: number;
  renderWallMs: number;
  renderOverheadMs: number;
  renderInvalidationMs: number;
  renderCacheMs: number;
  renderResultMs: number;
  rendererBuildMs: number;
  frameEvaluateMs: number;
  frameFixtureCloneMs: number;
  frameEffectLoopMs: number;
  frameOutputMs: number;
  publishMs: number;
  eventEmitMs: number;
  liveOutputMs: number;
  renderedActiveEffects: number;
  renderedSampledPixels: number;
  hasSink: boolean;
  publishedFrame: boolean;
  renderedFrame: boolean;
};

export type PreviewStateEvent = AppSnapshotDto["preview"] & { timing: PreviewTiming };

export type LivePreview = AppSnapshotDto["preview"] & { timing?: PreviewTiming };

export type ReadyGuiDocumentDto = Exclude<ActiveGuiDocumentDto, { type: "blocked" }>;

export type SequenceSelection = SequenceSelectionDto | null;

export type GuiFocus =
  | { type: "effect"; id: number }
  | { type: "mark"; collectionKey: string; index: number }
  | { type: "placement"; id: number }
  | { type: "point"; index: number }
  | null;

const GUI_CANVAS = {
  gridStepPx: 32,
  spatialPaddingPx: 42,
  pointHitMeters: 0.8,
  placementHitMeters: 1.2,
  meterRoundScale: 1_000_000,
  nanosecondRoundScale: 1_000_000_000
} as const;

const GUI_COLORS = {
  canvasBackground: "#17181b",
  canvasGrid: "#2c3036"
} as const;

export function normalizePoint(point: Point3MetersDto | GeometryRenderPointDto): Point3 {
  return {
    x: point.xMeters,
    y: point.yMeters,
    z: point.zMeters
  };
}

export function normalizeTransform(transform: TransformDto): Transform {
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

export function denormalizePoint(point: Point3): Point3MetersDto {
  return {
    xMeters: point.x,
    yMeters: point.y,
    zMeters: point.z
  };
}

export function denormalizeTransform(transform: Transform): TransformDto {
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

export type RenderBounds = {
  minX: number;
  minY: number;
  maxX: number;
  maxY: number;
};

export function normalizeBounds(bounds: GeometryRenderBoundsDto): RenderBounds {
  return {
    minX: bounds.minXMeters,
    minY: bounds.minYMeters,
    maxX: bounds.maxXMeters,
    maxY: bounds.maxYMeters
  };
}

export function drawSpatialCanvas(
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
  ctx.fillStyle = GUI_COLORS.canvasBackground;
  ctx.fillRect(0, 0, rect.width, rect.height);
  ctx.font = "12px Inter, sans-serif";
  const project = (point: Point3) => projectPoint(point, rect.width, rect.height, bounds);
  drawGrid(ctx, rect.width, rect.height);
  draw(ctx, project);
}

function drawGrid(ctx: CanvasRenderingContext2D, width: number, height: number) {
  ctx.strokeStyle = GUI_COLORS.canvasGrid;
  ctx.lineWidth = 1;
  for (let x = 0; x < width; x += GUI_CANVAS.gridStepPx) {
    ctx.beginPath();
    ctx.moveTo(x, 0);
    ctx.lineTo(x, height);
    ctx.stroke();
  }
  for (let y = 0; y < height; y += GUI_CANVAS.gridStepPx) {
    ctx.beginPath();
    ctx.moveTo(0, y);
    ctx.lineTo(width, y);
    ctx.stroke();
  }
}

function projectPoint(point: Point3, width: number, height: number, bounds: RenderBounds) {
  const padding = GUI_CANVAS.spatialPaddingPx;
  const spanX = Math.max(1, bounds.maxX - bounds.minX);
  const spanY = Math.max(1, bounds.maxY - bounds.minY);
  const scale = Math.min((width - padding * 2) / spanX, (height - padding * 2) / spanY);
  return {
    x: padding + (point.x - bounds.minX) * scale,
    y: height - padding - (point.y - bounds.minY) * scale
  };
}

export function unproject(x: number, y: number, canvas: HTMLCanvasElement | null, bounds: RenderBounds): Point3 {
  const rect = canvas?.getBoundingClientRect();
  const width = rect?.width ?? 1;
  const height = rect?.height ?? 1;
  const padding = GUI_CANVAS.spatialPaddingPx;
  const spanX = Math.max(1, bounds.maxX - bounds.minX);
  const spanY = Math.max(1, bounds.maxY - bounds.minY);
  const scale = Math.min((width - padding * 2) / spanX, (height - padding * 2) / spanY);
  return {
    x: bounds.minX + (x - padding) / scale,
    y: bounds.minY + (height - padding - y) / scale,
    z: 0
  };
}

export function nearestPlacement(document: LayoutDocumentDto, point: Point3): LayoutFixturePlacementDto | null {
  let best: LayoutFixturePlacementDto | null = null;
  let bestDistance = Infinity;
  for (const placement of document.fixtures) {
    const transform = normalizeTransform(placement.transform);
    const distance = Math.hypot(transform.position.x - point.x, transform.position.y - point.y);
    if (distance < bestDistance && distance < GUI_CANVAS.placementHitMeters) {
      best = placement;
      bestDistance = distance;
    }
  }
  return best;
}

export function nearestPoint(points: Point3[], point: Point3) {
  let best: number | null = null;
  let bestDistance = Infinity;
  for (let index = 0; index < points.length; index += 1) {
    const candidate = points[index];
    if (candidate === undefined) continue;
    const distance = Math.hypot(candidate.x - point.x, candidate.y - point.y);
    if (distance < bestDistance && distance < GUI_CANVAS.pointHitMeters) {
      best = index;
      bestDistance = distance;
    }
  }
  return best;
}

export function round6(value: number) {
  return Math.round(value * GUI_CANVAS.meterRoundScale) / GUI_CANVAS.meterRoundScale;
}

export function clamp(value: number, min: number, max: number) {
  return Math.max(min, Math.min(max, value));
}

export function roundToNanosecond(value: number) {
  return Math.round(value * GUI_CANVAS.nanosecondRoundScale) / GUI_CANVAS.nanosecondRoundScale;
}

export function formatSeconds(value: number) {
  const totalSeconds = Math.max(0, Math.floor(value));
  const minutes = Math.floor(totalSeconds / 60);
  const seconds = totalSeconds % 60;
  return `${minutes}:${String(seconds).padStart(2, "0")}`;
}

export function formatMs(value: number) {
  return `${value.toFixed(1)}ms`;
}

export function formatSignedMs(value: number) {
  const sign = value > 0 ? "+" : "";
  return `${sign}${formatMs(value)}`;
}
