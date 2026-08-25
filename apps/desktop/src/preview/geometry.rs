use dawn_language::preview::PropGeometry as DomainGeometry;
use dawn_language::values::{DistanceSpan, Point3};

use crate::dto::{GeometryRenderPoint, Point3Meters};

pub(crate) fn geometry_emitters(geometry: &DomainGeometry) -> Vec<GeometryRenderPoint> {
    match geometry {
        DomainGeometry::Points { points } => {
            points.iter().map(|point| render_point(*point)).collect()
        }
        DomainGeometry::Lines {
            points,
            point_count,
        } => line_emitters(points, *point_count),
        DomainGeometry::Arc {
            center,
            radius,
            start_degrees,
            end_degrees,
            point_count,
        } => arc_emitters(*center, *radius, *start_degrees, *end_degrees, *point_count),
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
                x_meters: lerp(first.x.as_meters_f64(), last.x.as_meters_f64(), amount),
                y_meters: lerp(first.y.as_meters_f64(), last.y.as_meters_f64(), amount),
                z_meters: lerp(first.z.as_meters_f64(), last.z.as_meters_f64(), amount),
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
    let radius_meters = radius.as_meters_f64();
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

pub(crate) fn arc_point(center: Point3, radius_meters: f64, degrees: f64) -> GeometryRenderPoint {
    let radians = degrees.to_radians();
    GeometryRenderPoint {
        x_meters: center.x.as_meters_f64() + radius_meters * radians.cos(),
        y_meters: center.y.as_meters_f64() + radius_meters * radians.sin(),
        z_meters: center.z.as_meters_f64(),
    }
}

pub(crate) fn render_point(point: Point3) -> GeometryRenderPoint {
    GeometryRenderPoint {
        x_meters: point.x.as_meters_f64(),
        y_meters: point.y.as_meters_f64(),
        z_meters: point.z.as_meters_f64(),
    }
}

pub(crate) fn point3_meters(point: Point3) -> Point3Meters {
    Point3Meters {
        x_meters: point.x.as_meters_f64(),
        y_meters: point.y.as_meters_f64(),
        z_meters: point.z.as_meters_f64(),
    }
}

fn lerp(start: f64, end: f64, amount: f64) -> f64 {
    start + (end - start) * amount
}
