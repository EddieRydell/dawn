use serde::{Deserialize, Serialize};

use crate::model::{
    FixturePlacement, Geometry, Point3, Resolved, Transform, BULB_SIZE_UNIT_RADIUS, MIN_BULB_SIZE,
};

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GeometryRenderPoint {
    pub x: f64,
    pub y: f64,
    pub z: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GeometryRenderBounds {
    pub min_x: f64,
    pub min_y: f64,
    pub max_x: f64,
    pub max_y: f64,
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
        radius_x: f64,
        radius_y: f64,
        rotation: f64,
        large_arc: bool,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GeometryRenderPlan {
    pub emitters: Vec<GeometryRenderPoint>,
    pub guides: Vec<GeometryRenderGuide>,
    pub bounds: GeometryRenderBounds,
    pub bulb_radius: f64,
}

pub(crate) fn geometry_summary(geometry: &Geometry) -> String {
    match geometry {
        Geometry::Points { points } => format!("{} point{}", points.len(), plural(points.len())),
        Geometry::Lines { pixels, .. } => format!("lines, {pixels} pixels"),
        Geometry::Arc { pixels, .. } => format!("arc, {pixels} pixels"),
    }
}

pub(crate) fn geometry_render_plan(geometry: &Geometry, bulb_size: f64) -> GeometryRenderPlan {
    let bulb_radius = bulb_radius(bulb_size);
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
            x: 0.0,
            y: 0.0,
            z: 0.0,
        })
}

fn sample_arc_points(
    center: &Point3,
    radius: f64,
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
    bulb_radius: f64,
) -> Option<GeometryRenderBounds> {
    let mut accumulator = BoundsAccumulator::new();
    for point in emitters {
        accumulator.include(point.x - bulb_radius, point.y - bulb_radius);
        accumulator.include(point.x + bulb_radius, point.y + bulb_radius);
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
                accumulator.include(start.x - radius_x.abs(), start.y - radius_y.abs());
                accumulator.include(start.x + radius_x.abs(), start.y + radius_y.abs());
                accumulator.include(end.x - radius_x.abs(), end.y - radius_y.abs());
                accumulator.include(end.x + radius_x.abs(), end.y + radius_y.abs());
            }
        }
    }
    accumulator.finish()
}

pub(crate) fn layout_render_bounds(
    fixtures: &[FixturePlacement<Resolved>],
) -> GeometryRenderBounds {
    let mut accumulator = BoundsAccumulator::new();
    for fixture in fixtures {
        let plan = geometry_render_plan(&fixture.fixture.geometry, fixture.fixture.bulb_size);
        for emitter in &plan.emitters {
            let point = transform_render_point(*emitter, &fixture.transform);
            let radius = transformed_radius(plan.bulb_radius, &fixture.transform);
            accumulator.include(point.x - radius, point.y - radius);
            accumulator.include(point.x + radius, point.y + radius);
        }
        for guide in &plan.guides {
            match guide {
                GeometryRenderGuide::Line { from, to } => {
                    accumulator.include_point(transform_render_point(*from, &fixture.transform));
                    accumulator.include_point(transform_render_point(*to, &fixture.transform));
                }
                GeometryRenderGuide::Arc {
                    start,
                    end,
                    radius_x,
                    radius_y,
                    ..
                } => {
                    let start = transform_render_point(*start, &fixture.transform);
                    let end = transform_render_point(*end, &fixture.transform);
                    let scale = fixture.transform.scale;
                    let radius_x = (radius_x * scale.x).abs();
                    let radius_y = (radius_y * scale.y).abs();
                    accumulator.include(start.x - radius_x, start.y - radius_y);
                    accumulator.include(start.x + radius_x, start.y + radius_y);
                    accumulator.include(end.x - radius_x, end.y - radius_y);
                    accumulator.include(end.x + radius_x, end.y + radius_y);
                }
            }
        }
    }
    accumulator.finish().unwrap_or(GeometryRenderBounds {
        min_x: -5.0,
        min_y: -4.0,
        max_x: 5.0,
        max_y: 4.0,
    })
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

    fn include(&mut self, x: f64, y: f64) {
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
        min_x: -1.0,
        min_y: -1.0,
        max_x: 1.0,
        max_y: 1.0,
    }
}

fn bulb_radius(value: f64) -> f64 {
    value.max(MIN_BULB_SIZE) * BULB_SIZE_UNIT_RADIUS
}

fn transform_render_point(
    point: GeometryRenderPoint,
    transform: &Transform,
) -> GeometryRenderPoint {
    let radians = transform.rotation.z.to_radians();
    let x = point.x * transform.scale.x;
    let y = point.y * transform.scale.y;
    GeometryRenderPoint {
        x: transform.position.x + x * radians.cos() - y * radians.sin(),
        y: transform.position.y + x * radians.sin() + y * radians.cos(),
        z: transform.position.z + point.z * transform.scale.z,
    }
}

fn transformed_radius(radius: f64, transform: &Transform) -> f64 {
    radius * transform.scale.x.abs().max(transform.scale.y.abs())
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
        x: lerp(from.x, to.x, t),
        y: lerp(from.y, to.y, t),
        z: lerp(from.z, to.z, t),
    }
}

fn point_distance(from: &Point3, to: &Point3) -> f64 {
    let dx = to.x - from.x;
    let dy = to.y - from.y;
    let dz = to.z - from.z;
    (dx * dx + dy * dy + dz * dz).sqrt()
}

fn arc_point(center: &Point3, radius: f64, degrees: f64) -> GeometryRenderPoint {
    let radians = degrees.to_radians();
    GeometryRenderPoint {
        x: center.x + radius * radians.cos(),
        y: center.y + radius * radians.sin(),
        z: center.z,
    }
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
#[cfg(test)]
mod geometry_render_tests {
    use super::*;
    use crate::model::DEFAULT_BULB_SIZE;

    #[test]
    fn line_sampling_covers_empty_single_midpoint_endpoints_and_zero_length() {
        assert!(sample_polyline_points(&[], 4).is_empty());

        let single = sample_polyline_points(&[point(2.0, 3.0, 4.0)], 4);
        assert_eq!(single, vec![render_point(2.0, 3.0, 4.0)]);

        let line = [point(0.0, 0.0, 0.0), point(10.0, 0.0, 0.0)];
        assert_eq!(
            sample_polyline_points(&line, 1),
            vec![render_point(5.0, 0.0, 0.0)]
        );
        assert_eq!(
            sample_polyline_points(&line, 3),
            vec![
                render_point(0.0, 0.0, 0.0),
                render_point(5.0, 0.0, 0.0),
                render_point(10.0, 0.0, 0.0)
            ]
        );

        let zero = [point(1.0, 1.0, 0.0), point(1.0, 1.0, 0.0)];
        assert_eq!(
            sample_polyline_points(&zero, 3),
            vec![
                render_point(1.0, 1.0, 0.0),
                render_point(1.0, 1.0, 0.0),
                render_point(1.0, 1.0, 0.0)
            ]
        );
    }

    #[test]
    fn arc_sampling_covers_midpoint_endpoints_and_large_arc() {
        let center = point(0.0, 0.0, 0.0);
        let midpoint = sample_arc_points(&center, 1.0, 0.0, 180.0, 1);
        assert_close(midpoint[0].x, 0.0);
        assert_close(midpoint[0].y, 1.0);

        let points = sample_arc_points(&center, 1.0, 0.0, 180.0, 3);
        assert_close(points[0].x, 1.0);
        assert_close(points[0].y, 0.0);
        assert_close(points[1].x, 0.0);
        assert_close(points[1].y, 1.0);
        assert_close(points[2].x, -1.0);
        assert_close(points[2].y, 0.0);

        let plan = geometry_render_plan(
            &Geometry::Arc {
                center,
                radius: 1.0,
                start_degrees: 0.0,
                end_degrees: 270.0,
                pixels: 4,
            },
            DEFAULT_BULB_SIZE,
        );
        assert!(matches!(
            plan.guides.as_slice(),
            [GeometryRenderGuide::Arc {
                large_arc: true,
                ..
            }]
        ));
    }

    fn point(x: f64, y: f64, z: f64) -> Point3 {
        Point3 { x, y, z }
    }

    fn render_point(x: f64, y: f64, z: f64) -> GeometryRenderPoint {
        GeometryRenderPoint { x, y, z }
    }

    fn assert_close(actual: f64, expected: f64) {
        assert!(
            (actual - expected).abs() < 0.000001,
            "expected {actual} to be close to {expected}"
        );
    }
}
