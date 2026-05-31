use serde::{Deserialize, Serialize};

use crate::model::{
    Distance, DistanceSpan, FixtureId, FixturePlacement, Geometry, Point3, Resolved, Transform,
    MIN_BULB_DIAMETER,
};

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GeometryRenderPoint {
    pub x: Distance,
    pub y: Distance,
    pub z: Distance,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GeometryRenderBounds {
    pub min_x: Distance,
    pub min_y: Distance,
    pub max_x: Distance,
    pub max_y: Distance,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum GeometryRenderGuide {
    Line {
        from: GeometryRenderPoint,
        to: GeometryRenderPoint,
    },
    Arc {
        start: GeometryRenderPoint,
        end: GeometryRenderPoint,
        radius_x: DistanceSpan,
        radius_y: DistanceSpan,
        rotation: f64,
        large_arc: bool,
        sweep_positive: bool,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GeometryRenderPlan {
    pub emitters: Vec<GeometryRenderPoint>,
    pub guides: Vec<GeometryRenderGuide>,
    pub bounds: GeometryRenderBounds,
    pub bulb_radius: DistanceSpan,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LayoutFixtureRenderPlan {
    pub id: FixtureId,
    pub emitters: Vec<GeometryRenderPoint>,
    pub guides: Vec<GeometryRenderGuide>,
    pub bounds: GeometryRenderBounds,
    pub bulb_radius: DistanceSpan,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LayoutRenderPlan {
    pub fixtures: Vec<LayoutFixtureRenderPlan>,
    pub bounds: GeometryRenderBounds,
}

pub(crate) fn geometry_summary(geometry: &Geometry) -> String {
    match geometry {
        Geometry::Points { points } => format!("{} point{}", points.len(), plural(points.len())),
        Geometry::Lines { pixels, .. } => format!("lines, {pixels} pixels"),
        Geometry::Arc { pixels, .. } => format!("arc, {pixels} pixels"),
    }
}

pub fn geometry_render_plan(
    geometry: &Geometry,
    bulb_diameter: DistanceSpan,
) -> GeometryRenderPlan {
    let bulb_radius = bulb_radius(bulb_diameter);
    let (emitters, guides) = match geometry {
        Geometry::Points { points } => (
            points.iter().map(render_point_from_point3).collect(),
            Vec::new(),
        ),
        Geometry::Lines { points, pixels } => {
            (sample_polyline_points(points, *pixels), line_guides(points))
        }
        Geometry::Arc {
            center,
            radius,
            start_degrees,
            end_degrees,
            pixels,
        } => {
            let emitters =
                sample_arc_points(center, *radius, *start_degrees, *end_degrees, *pixels);
            let start = arc_point(center, *radius, *start_degrees);
            let end = arc_point(center, *radius, *end_degrees);
            let guide = GeometryRenderGuide::Arc {
                start,
                end,
                radius_x: *radius,
                radius_y: *radius,
                rotation: 0.0,
                large_arc: (end_degrees - start_degrees).abs() > 180.0,
                sweep_positive: end_degrees >= start_degrees,
            };
            (emitters, vec![guide])
        }
    };
    let bounds =
        render_bounds(&emitters, &guides, bulb_radius).unwrap_or_else(default_render_bounds);
    GeometryRenderPlan {
        emitters,
        guides,
        bounds,
        bulb_radius,
    }
}

fn sample_polyline_points(points: &[Point3], pixels: u32) -> Vec<GeometryRenderPoint> {
    let count = (pixels as usize).max(1);
    if points.is_empty() {
        return Vec::new();
    }
    if points.len() == 1 {
        return vec![render_point_from_point3(&points[0])];
    }

    let segments = points
        .windows(2)
        .map(|pair| PolylineSegment {
            from: pair[0],
            to: pair[1],
            length: point_distance(&pair[0], &pair[1]),
        })
        .collect::<Vec<_>>();
    let total_length = segments.iter().map(|segment| segment.length).sum::<f64>();
    if total_length == 0.0 {
        return (0..count)
            .map(|_| render_point_from_point3(&points[0]))
            .collect();
    }

    if count == 1 {
        return vec![point_at_distance(&segments, total_length / 2.0)];
    }
    (0..count)
        .map(|index| {
            point_at_distance(
                &segments,
                total_length * (index as f64 / (count - 1) as f64),
            )
        })
        .collect()
}

#[derive(Debug, Clone, Copy)]
struct PolylineSegment {
    from: Point3,
    to: Point3,
    length: f64,
}

fn point_at_distance(segments: &[PolylineSegment], distance: f64) -> GeometryRenderPoint {
    let mut remaining = distance;
    for segment in segments {
        if segment.length == 0.0 {
            continue;
        }
        if remaining <= segment.length {
            return interpolate_point(&segment.from, &segment.to, remaining / segment.length);
        }
        remaining -= segment.length;
    }
    segments
        .last()
        .map(|segment| render_point_from_point3(&segment.to))
        .unwrap_or(GeometryRenderPoint {
            x: Distance::ZERO,
            y: Distance::ZERO,
            z: Distance::ZERO,
        })
}

fn sample_arc_points(
    center: &Point3,
    radius: DistanceSpan,
    start_degrees: f64,
    end_degrees: f64,
    pixels: u32,
) -> Vec<GeometryRenderPoint> {
    let count = (pixels as usize).max(1);
    if count == 1 {
        return vec![arc_point(
            center,
            radius,
            (start_degrees + end_degrees) / 2.0,
        )];
    }
    (0..count)
        .map(|index| {
            arc_point(
                center,
                radius,
                lerp(
                    start_degrees,
                    end_degrees,
                    index as f64 / (count - 1) as f64,
                ),
            )
        })
        .collect()
}

fn line_guides(points: &[Point3]) -> Vec<GeometryRenderGuide> {
    points
        .windows(2)
        .map(|pair| GeometryRenderGuide::Line {
            from: render_point_from_point3(&pair[0]),
            to: render_point_from_point3(&pair[1]),
        })
        .collect()
}

fn render_bounds(
    emitters: &[GeometryRenderPoint],
    guides: &[GeometryRenderGuide],
    bulb_radius: DistanceSpan,
) -> Option<GeometryRenderBounds> {
    let mut accumulator = BoundsAccumulator::new();
    for point in emitters {
        let radius = bulb_radius.as_micrometers() as i64;
        accumulator.include(
            offset_distance(point.x, -radius),
            offset_distance(point.y, -radius),
        );
        accumulator.include(
            offset_distance(point.x, radius),
            offset_distance(point.y, radius),
        );
    }
    for guide in guides {
        match guide {
            GeometryRenderGuide::Line { from, to } => {
                accumulator.include_point(*from);
                accumulator.include_point(*to);
            }
            GeometryRenderGuide::Arc {
                start,
                end,
                radius_x,
                radius_y,
                ..
            } => {
                let radius_x = radius_x.as_micrometers() as i64;
                let radius_y = radius_y.as_micrometers() as i64;
                accumulator.include(
                    offset_distance(start.x, -radius_x),
                    offset_distance(start.y, -radius_y),
                );
                accumulator.include(
                    offset_distance(start.x, radius_x),
                    offset_distance(start.y, radius_y),
                );
                accumulator.include(
                    offset_distance(end.x, -radius_x),
                    offset_distance(end.y, -radius_y),
                );
                accumulator.include(
                    offset_distance(end.x, radius_x),
                    offset_distance(end.y, radius_y),
                );
            }
        }
    }
    accumulator.finish()
}

pub fn layout_render_plan(fixtures: &[FixturePlacement<Resolved>]) -> LayoutRenderPlan {
    let mut accumulator = BoundsAccumulator::new();
    let fixtures = fixtures
        .iter()
        .map(|fixture| {
            let local_plan =
                geometry_render_plan(&fixture.fixture.geometry, fixture.fixture.bulb_diameter);
            let plan = transform_geometry_render_plan(&local_plan, &fixture.transform);
            include_bounds(&mut accumulator, plan.bounds);
            LayoutFixtureRenderPlan {
                id: fixture.id,
                emitters: plan.emitters,
                guides: plan.guides,
                bounds: plan.bounds,
                bulb_radius: plan.bulb_radius,
            }
        })
        .collect();

    LayoutRenderPlan {
        fixtures,
        bounds: accumulator.finish().unwrap_or_else(default_layout_bounds),
    }
}

pub fn layout_render_bounds(fixtures: &[FixturePlacement<Resolved>]) -> GeometryRenderBounds {
    layout_render_plan(fixtures).bounds
}

pub fn transform_geometry_render_plan(
    plan: &GeometryRenderPlan,
    transform: &Transform,
) -> GeometryRenderPlan {
    let bulb_radius = transformed_radius(plan.bulb_radius, transform);
    let emitters = plan
        .emitters
        .iter()
        .map(|point| transform_render_point(*point, transform))
        .collect::<Vec<_>>();
    let guides = plan
        .guides
        .iter()
        .map(|guide| transform_render_guide(guide, transform))
        .collect::<Vec<_>>();
    let bounds =
        render_bounds(&emitters, &guides, bulb_radius).unwrap_or_else(default_render_bounds);
    GeometryRenderPlan {
        emitters,
        guides,
        bounds,
        bulb_radius,
    }
}

fn default_layout_bounds() -> GeometryRenderBounds {
    GeometryRenderBounds {
        min_x: Distance::from_micrometers(-5_000_000),
        min_y: Distance::from_micrometers(-4_000_000),
        max_x: Distance::from_micrometers(5_000_000),
        max_y: Distance::from_micrometers(4_000_000),
    }
}

fn include_bounds(accumulator: &mut BoundsAccumulator, bounds: GeometryRenderBounds) {
    accumulator.include(bounds.min_x, bounds.min_y);
    accumulator.include(bounds.max_x, bounds.max_y);
}

fn transform_render_guide(
    guide: &GeometryRenderGuide,
    transform: &Transform,
) -> GeometryRenderGuide {
    match guide {
        GeometryRenderGuide::Line { from, to } => GeometryRenderGuide::Line {
            from: transform_render_point(*from, transform),
            to: transform_render_point(*to, transform),
        },
        GeometryRenderGuide::Arc {
            start,
            end,
            radius_x,
            radius_y,
            rotation,
            large_arc,
            sweep_positive,
        } => GeometryRenderGuide::Arc {
            start: transform_render_point(*start, transform),
            end: transform_render_point(*end, transform),
            radius_x: scale_span(*radius_x, transform.scale.x),
            radius_y: scale_span(*radius_y, transform.scale.y),
            rotation: rotation + transform.rotation.z,
            large_arc: *large_arc,
            sweep_positive: if transform.scale.x.signum() == transform.scale.y.signum() {
                *sweep_positive
            } else {
                !*sweep_positive
            },
        },
    }
}

#[derive(Debug, Clone, Copy)]
struct BoundsAccumulator {
    bounds: Option<GeometryRenderBounds>,
}

impl BoundsAccumulator {
    fn new() -> Self {
        Self { bounds: None }
    }

    fn include_point(&mut self, point: GeometryRenderPoint) {
        self.include(point.x, point.y);
    }

    fn include(&mut self, x: Distance, y: Distance) {
        self.bounds = Some(match self.bounds {
            Some(bounds) => GeometryRenderBounds {
                min_x: bounds.min_x.min(x),
                min_y: bounds.min_y.min(y),
                max_x: bounds.max_x.max(x),
                max_y: bounds.max_y.max(y),
            },
            None => GeometryRenderBounds {
                min_x: x,
                min_y: y,
                max_x: x,
                max_y: y,
            },
        });
    }

    fn finish(self) -> Option<GeometryRenderBounds> {
        self.bounds
    }
}

fn default_render_bounds() -> GeometryRenderBounds {
    GeometryRenderBounds {
        min_x: Distance::from_micrometers(-1_000_000),
        min_y: Distance::from_micrometers(-1_000_000),
        max_x: Distance::from_micrometers(1_000_000),
        max_y: Distance::from_micrometers(1_000_000),
    }
}

fn bulb_radius(value: DistanceSpan) -> DistanceSpan {
    DistanceSpan::from_micrometers(value.max(MIN_BULB_DIAMETER).as_micrometers() / 2)
}

fn transform_render_point(
    point: GeometryRenderPoint,
    transform: &Transform,
) -> GeometryRenderPoint {
    let radians = transform.rotation.z.to_radians();
    let x = point.x.as_meters_f64() * transform.scale.x;
    let y = point.y.as_meters_f64() * transform.scale.y;
    GeometryRenderPoint {
        x: rounded_distance(
            transform.position.x.as_meters_f64() + x * radians.cos() - y * radians.sin(),
        ),
        y: rounded_distance(
            transform.position.y.as_meters_f64() + x * radians.sin() + y * radians.cos(),
        ),
        z: rounded_distance(
            transform.position.z.as_meters_f64() + point.z.as_meters_f64() * transform.scale.z,
        ),
    }
}

fn transformed_radius(radius: DistanceSpan, transform: &Transform) -> DistanceSpan {
    scale_span(radius, transform.scale.x.abs().max(transform.scale.y.abs()))
}

fn render_point_from_point3(point: &Point3) -> GeometryRenderPoint {
    GeometryRenderPoint {
        x: point.x,
        y: point.y,
        z: point.z,
    }
}

fn interpolate_point(from: &Point3, to: &Point3, t: f64) -> GeometryRenderPoint {
    GeometryRenderPoint {
        x: rounded_distance(lerp(from.x.as_meters_f64(), to.x.as_meters_f64(), t)),
        y: rounded_distance(lerp(from.y.as_meters_f64(), to.y.as_meters_f64(), t)),
        z: rounded_distance(lerp(from.z.as_meters_f64(), to.z.as_meters_f64(), t)),
    }
}

fn point_distance(from: &Point3, to: &Point3) -> f64 {
    let dx = to.x.as_meters_f64() - from.x.as_meters_f64();
    let dy = to.y.as_meters_f64() - from.y.as_meters_f64();
    let dz = to.z.as_meters_f64() - from.z.as_meters_f64();
    (dx * dx + dy * dy + dz * dz).sqrt()
}

fn arc_point(center: &Point3, radius: DistanceSpan, degrees: f64) -> GeometryRenderPoint {
    let radians = degrees.to_radians();
    GeometryRenderPoint {
        x: rounded_distance(center.x.as_meters_f64() + radius.as_meters_f64() * radians.cos()),
        y: rounded_distance(center.y.as_meters_f64() + radius.as_meters_f64() * radians.sin()),
        z: center.z,
    }
}

fn rounded_distance(meters: f64) -> Distance {
    Distance::from_micrometers((meters * 1_000_000.0).round() as i64)
}

fn scale_span(span: DistanceSpan, scale: f64) -> DistanceSpan {
    DistanceSpan::from_micrometers((span.as_meters_f64() * scale.abs() * 1_000_000.0).round() as u64)
}

fn offset_distance(distance: Distance, offset_micrometers: i64) -> Distance {
    Distance::from_micrometers(distance.as_micrometers().saturating_add(offset_micrometers))
}

fn lerp(from: f64, to: f64, t: f64) -> f64 {
    from + (to - from) * t
}

fn plural(count: usize) -> &'static str {
    if count == 1 {
        ""
    } else {
        "s"
    }
}
