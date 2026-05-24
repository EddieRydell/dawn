import type { MouseEvent } from "react";
import type {
  GeometryRenderBounds,
  GeometryRenderPlan,
  GeometryRenderPoint,
  Transform
} from "../generated/bindings";

export type ViewBox = { x: number; y: number; width: number; height: number };
export type SvgPoint = { x: number; y: number };

export function renderGeometryPlan(plan: GeometryRenderPlan, classPrefix = "layout-fixture") {
  return (
    <>
      {plan.guides.map((guide, index) => {
        if (guide.type === "line") {
          return (
            <line
              key={`guide-${index}`}
              className={`${classPrefix}-guide`}
              x1={numberValue(guide.from.x)}
              y1={numberValue(guide.from.y)}
              x2={numberValue(guide.to.x)}
              y2={numberValue(guide.to.y)}
            />
          );
        }
        return (
          <path
            key={`guide-${index}`}
            className={`${classPrefix}-guide`}
            d={`M ${numberValue(guide.start.x)} ${numberValue(guide.start.y)} A ${numberValue(guide.radiusX)} ${numberValue(guide.radiusY)} ${numberValue(guide.rotation)} ${guide.largeArc ? 1 : 0} 1 ${numberValue(guide.end.x)} ${numberValue(guide.end.y)}`}
          />
        );
      })}
      {plan.emitters.map((point, index) => (
        <circle
          key={`emitter-${index}`}
          className={`${classPrefix}-emitter`}
          cx={numberValue(point.x)}
          cy={numberValue(point.y)}
          r={numberValue(plan.bulbRadius)}
        />
      ))}
    </>
  );
}

export function fixtureTransform(transform: Transform) {
  const rotation = ((transform.rotation?.z ?? 0) * Math.PI) / 180;
  const scaleX = transform.scale?.x ?? 1;
  const scaleY = transform.scale?.y ?? 1;
  const a = scaleX * Math.cos(rotation);
  const b = scaleX * Math.sin(rotation);
  const c = -scaleY * Math.sin(rotation);
  const d = scaleY * Math.cos(rotation);
  return `matrix(${a} ${b} ${c} ${d} ${transform.position.x ?? 0} ${transform.position.y ?? 0})`;
}

export function fitViewBox(bounds: GeometryRenderBounds, viewportAspect: number, options: { minSize: number; paddingScale: number; paddingBase: number }): ViewBox {
  const minX = numberValue(bounds.minX, -1);
  const minY = numberValue(bounds.minY, -1);
  const maxX = numberValue(bounds.maxX, 1);
  const maxY = numberValue(bounds.maxY, 1);
  const width = Math.max(maxX - minX, options.minSize);
  const height = Math.max(maxY - minY, options.minSize);
  const padding = Math.max(width, height) * options.paddingScale + options.paddingBase;
  let fittedWidth = width + padding * 2;
  let fittedHeight = height + padding * 2;
  const aspect = Math.max(viewportAspect, 0.1);
  if (fittedWidth / fittedHeight > aspect) {
    fittedHeight = fittedWidth / aspect;
  } else {
    fittedWidth = fittedHeight * aspect;
  }
  const centerX = (minX + maxX) / 2;
  const centerY = (minY + maxY) / 2;
  return {
    x: centerX - fittedWidth / 2,
    y: -(centerY + fittedHeight / 2),
    width: fittedWidth,
    height: fittedHeight
  };
}

export function zoomViewBox(viewBox: ViewBox, factor: number, minSize: number): ViewBox {
  const width = Math.max(viewBox.width * factor, minSize);
  const height = Math.max(viewBox.height * factor, minSize);
  return { x: viewBox.x + (viewBox.width - width) / 2, y: viewBox.y + (viewBox.height - height) / 2, width, height };
}

export function matchViewBoxAspect(viewBox: ViewBox, viewportAspect: number): ViewBox {
  const aspect = Math.max(viewportAspect, 0.1);
  let width = viewBox.width;
  let height = viewBox.height;
  if (width / height > aspect) {
    height = width / aspect;
  } else {
    width = height * aspect;
  }
  return {
    x: viewBox.x + (viewBox.width - width) / 2,
    y: viewBox.y + (viewBox.height - height) / 2,
    width,
    height
  };
}

export function ScaleBar({
  viewBox,
  units,
  svg
}: {
  viewBox: ViewBox;
  units: string;
  svg: SVGSVGElement | null;
}) {
  const length = niceScaleLength(viewBox.width * 0.18);
  const x = viewBox.x + viewBox.width * 0.055;
  const y = viewBox.y + viewBox.height * 0.9;
  const tick = viewBox.height * 0.018;
  const fontSize = screenPixelsToUserY(viewBox, svg, 12);
  const labelStrokeWidth = screenPixelsToUserY(viewBox, svg, 3);
  return (
    <g className="canvas-scale-bar">
      <line x1={x} y1={y} x2={x + length} y2={y} />
      <line x1={x} y1={y - tick} x2={x} y2={y + tick} />
      <line x1={x + length} y1={y - tick} x2={x + length} y2={y + tick} />
      <text x={x} y={y - tick * 1.8} fontSize={fontSize} strokeWidth={labelStrokeWidth}>{formatScaleLength(length)} {units}</text>
    </g>
  );
}

export function svgEventPoint(event: MouseEvent, svg: SVGSVGElement | null): SvgPoint {
  if (!svg) return { x: 0, y: 0 };
  const point = svg.createSVGPoint();
  point.x = event.clientX;
  point.y = event.clientY;
  const transformed = point.matrixTransform(svg.getScreenCTM()?.inverse());
  return { x: transformed.x, y: transformed.y };
}

export function svgPixelWidth(svg: SVGSVGElement | null) {
  return Math.max(svg?.clientWidth ?? 1, 1);
}

export function svgPixelHeight(svg: SVGSVGElement | null) {
  return Math.max(svg?.clientHeight ?? 1, 1);
}

export function renderPointsAsGeometry(points: GeometryRenderPoint[]) {
  return points.map((point) => ({ x: numberValue(point.x), y: numberValue(point.y), z: numberValue(point.z) }));
}

function screenPixelsToUserY(viewBox: ViewBox, svg: SVGSVGElement | null, pixels: number) {
  return (pixels / svgPixelHeight(svg)) * viewBox.height;
}

function niceScaleLength(target: number) {
  if (!Number.isFinite(target) || target <= 0) return 1;
  const exponent = Math.floor(Math.log10(target));
  const magnitude = 10 ** exponent;
  const normalized = target / magnitude;
  const step = normalized >= 5 ? 5 : normalized >= 2 ? 2 : 1;
  return step * magnitude;
}

function formatScaleLength(length: number) {
  return Number.isInteger(length) ? `${length}` : length.toFixed(length < 0.1 ? 3 : length < 1 ? 2 : 1).replace(/0+$/, "").replace(/\.$/, "");
}

function numberValue(value: number | null | undefined, fallback = 0) {
  return Number.isFinite(value) ? value as number : fallback;
}
