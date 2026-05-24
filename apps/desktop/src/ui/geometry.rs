use dawn_project::render::{GeometryRenderBounds, GeometryRenderPoint};

pub fn bounds_center(bounds: &GeometryRenderBounds) -> GeometryRenderPoint {
    GeometryRenderPoint {
        x: (bounds.min_x + bounds.max_x) / 2.0,
        y: (bounds.min_y + bounds.max_y) / 2.0,
        z: 0.0,
    }
}
