use dawn_language::setup::Geometry as DomainGeometry;
use dawn_language::values::{Color, Distance, DistanceSpan, Point3};

use crate::dto::{
    Geometry, GeometryRenderBounds, GeometryRenderGuide, GeometryRenderPlan, GeometryRenderPoint,
    LayoutFixturePlacement, Point3Meters, ResolvedLayoutFixture,
};

pub(crate) fn geometry(geometry: &DomainGeometry) -> Geometry {
    match geometry {
        DomainGeometry::Points { points } => Geometry::Points {
            points: points.iter().map(|point| point3_meters(*point)).collect(),
        },
        DomainGeometry::Lines { points, pixels } => Geometry::Lines {
            points: points.iter().map(|point| point3_meters(*point)).collect(),
            pixels: *pixels,
        },
        DomainGeometry::Arc {
            center,
            radius,
            start_degrees,
            end_degrees,
            pixels,
        } => Geometry::Arc {
            center: point3_meters(*center),
            radius_meters: distance_span_meters(*radius),
            start_degrees: *start_degrees,
            end_degrees: *end_degrees,
            pixels: *pixels,
        },
    }
}

pub(crate) fn render_plan(
    geometry: &DomainGeometry,
    bulb_radius: DistanceSpan,
) -> GeometryRenderPlan {
    let emitters = emitters(geometry);
    let guides = guides(geometry);
    let bounds = bounds_for_points(emitters.iter().cloned());
    GeometryRenderPlan {
        emitters,
        guides,
        bounds,
        bulb_radius_meters: distance_span_meters(bulb_radius),
    }
}

fn emitters(geometry: &DomainGeometry) -> Vec<GeometryRenderPoint> {
    match geometry {
        DomainGeometry::Points { points } => {
            points.iter().map(|point| render_point(*point)).collect()
        }
        DomainGeometry::Lines { points, pixels } => line_emitters(points, *pixels),
        DomainGeometry::Arc {
            center,
            radius,
            start_degrees,
            end_degrees,
            pixels,
        } => arc_emitters(*center, *radius, *start_degrees, *end_degrees, *pixels),
    }
}

fn guides(geometry: &DomainGeometry) -> Vec<GeometryRenderGuide> {
    match geometry {
        DomainGeometry::Lines { points, .. } => points
            .windows(2)
            .filter_map(|window| {
                let [from, to] = window else { return None };
                Some(GeometryRenderGuide::Line {
                    from: render_point(*from),
                    to: render_point(*to),
                })
            })
            .collect(),
        DomainGeometry::Arc {
            center,
            radius,
            start_degrees,
            end_degrees,
            ..
        } => {
            let radius_meters = distance_span_meters(*radius);
            vec![GeometryRenderGuide::Arc {
                start: arc_point(*center, radius_meters, *start_degrees),
                end: arc_point(*center, radius_meters, *end_degrees),
                radius_x_meters: radius_meters,
                radius_y_meters: radius_meters,
                rotation: 0.0,
                large_arc: (*end_degrees - *start_degrees).abs() > 180.0,
                sweep_positive: end_degrees >= start_degrees,
            }]
        }
        DomainGeometry::Points { .. } => Vec::new(),
    }
}

fn line_emitters(points: &[Point3], pixels: u32) -> Vec<GeometryRenderPoint> {
    if points.is_empty() || pixels == 0 {
        return Vec::new();
    }
    if points.len() == 1 || pixels == 1 {
        return vec![render_point(points[0])];
    }
    let (first, last) = (points[0], points[points.len() - 1]);
    (0..pixels)
        .map(|index| {
            let amount = f64::from(index) / f64::from(pixels - 1);
            GeometryRenderPoint {
                x_meters: lerp(distance_meters(first.x), distance_meters(last.x), amount),
                y_meters: lerp(distance_meters(first.y), distance_meters(last.y), amount),
                z_meters: lerp(distance_meters(first.z), distance_meters(last.z), amount),
            }
        })
        .collect()
}

fn arc_emitters(
    center: Point3,
    radius: DistanceSpan,
    start_degrees: f64,
    end_degrees: f64,
    pixels: u32,
) -> Vec<GeometryRenderPoint> {
    if pixels == 0 {
        return Vec::new();
    }
    let radius_meters = distance_span_meters(radius);
    (0..pixels)
        .map(|index| {
            let amount = if pixels == 1 {
                0.0
            } else {
                f64::from(index) / f64::from(pixels - 1)
            };
            arc_point(
                center,
                radius_meters,
                lerp(start_degrees, end_degrees, amount),
            )
        })
        .collect()
}

fn arc_point(center: Point3, radius_meters: f64, degrees: f64) -> GeometryRenderPoint {
    let radians = degrees.to_radians();
    GeometryRenderPoint {
        x_meters: distance_meters(center.x) + radius_meters * radians.cos(),
        y_meters: distance_meters(center.y) + radius_meters * radians.sin(),
        z_meters: distance_meters(center.z),
    }
}

fn render_point(point: Point3) -> GeometryRenderPoint {
    GeometryRenderPoint {
        x_meters: distance_meters(point.x),
        y_meters: distance_meters(point.y),
        z_meters: distance_meters(point.z),
    }
}

pub(crate) fn point3_meters(point: Point3) -> Point3Meters {
    Point3Meters {
        x_meters: distance_meters(point.x),
        y_meters: distance_meters(point.y),
        z_meters: distance_meters(point.z),
    }
}

fn distance_meters(distance: Distance) -> f64 {
    distance.micrometers as f64 / 1_000_000.0
}

pub(crate) fn distance_span_meters(distance: DistanceSpan) -> f64 {
    distance.micrometers as f64 / 1_000_000.0
}

fn lerp(start: f64, end: f64, amount: f64) -> f64 {
    start + (end - start) * amount
}

fn bounds_for_points(points: impl Iterator<Item = GeometryRenderPoint>) -> GeometryRenderBounds {
    let mut bounds: Option<(f64, f64, f64, f64)> = None;
    for point in points {
        bounds = Some(match bounds {
            None => (
                point.x_meters,
                point.y_meters,
                point.x_meters,
                point.y_meters,
            ),
            Some((min_x, min_y, max_x, max_y)) => (
                min_x.min(point.x_meters),
                min_y.min(point.y_meters),
                max_x.max(point.x_meters),
                max_y.max(point.y_meters),
            ),
        });
    }
    let (min_x, min_y, mut max_x, mut max_y) = bounds.unwrap_or((0.0, 0.0, 1.0, 1.0));
    if (max_x - min_x).abs() < 1.0 {
        max_x = min_x + 1.0;
    }
    if (max_y - min_y).abs() < 1.0 {
        max_y = min_y + 1.0;
    }
    GeometryRenderBounds {
        min_x_meters: min_x,
        min_y_meters: min_y,
        max_x_meters: max_x,
        max_y_meters: max_y,
    }
}

pub(crate) fn layout_bounds(fixtures: &[LayoutFixturePlacement]) -> GeometryRenderBounds {
    bounds_for_points(fixtures.iter().map(|fixture| GeometryRenderPoint {
        x_meters: fixture.transform.position.x_meters,
        y_meters: fixture.transform.position.y_meters,
        z_meters: fixture.transform.position.z_meters,
    }))
}

pub(crate) fn geometry_summary(geometry: &DomainGeometry) -> String {
    match geometry {
        DomainGeometry::Points { points } => format!("{} points", points.len()),
        DomainGeometry::Lines { pixels, .. } => format!("{pixels} line pixels"),
        DomainGeometry::Arc { pixels, .. } => format!("{pixels} arc pixels"),
    }
}

pub(crate) fn empty_resolved_fixture() -> ResolvedLayoutFixture {
    ResolvedLayoutFixture {
        name: "Missing fixture".to_string(),
        color_model: "rgb".to_string(),
        bulb_diameter_meters: 0.05,
        geometry_summary: "Missing".to_string(),
        render_plan: GeometryRenderPlan {
            emitters: Vec::new(),
            guides: Vec::new(),
            bounds: GeometryRenderBounds {
                min_x_meters: 0.0,
                min_y_meters: 0.0,
                max_x_meters: 1.0,
                max_y_meters: 1.0,
            },
            bulb_radius_meters: 0.025,
        },
        source_path: String::new(),
        object_key: None,
    }
}

pub(crate) fn color_hex(color: Color) -> String {
    format!("#{:02x}{:02x}{:02x}", color.red, color.green, color.blue)
}
