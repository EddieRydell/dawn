use crate::model::show::Position2D;

/// Project a 2D position onto 1D for burst wipe (Chebyshev distance from center).
pub fn project_burst(pos: Position2D, cx: f64, cy: f64) -> f64 {
    let x = f64::from(pos.x);
    let y = f64::from(pos.y);
    let dx = (x - cx).abs();
    let dy = (y - cy).abs();
    let max_possible = 1.0f64.max(cx.max(1.0 - cx)).max(cy.max(1.0 - cy));
    (dx.max(dy) / max_possible).clamp(0.0, 1.0)
}

/// Project a 2D position onto 1D for circle wipe (Euclidean distance from center).
pub fn project_circle(pos: Position2D, cx: f64, cy: f64) -> f64 {
    let x = f64::from(pos.x);
    let y = f64::from(pos.y);
    let dx = x - cx;
    let dy = y - cy;
    let d = (dx * dx + dy * dy).sqrt();
    // Max possible distance from center in unit square
    let max_d = ((cx.max(1.0 - cx)).powi(2) + (cy.max(1.0 - cy)).powi(2)).sqrt();
    if max_d > 0.0 {
        (d / max_d).clamp(0.0, 1.0)
    } else {
        0.0
    }
}

/// Project a 2D position onto 1D for diamond wipe (Manhattan distance from center).
pub fn project_diamond(pos: Position2D, cx: f64, cy: f64) -> f64 {
    let x = f64::from(pos.x);
    let y = f64::from(pos.y);
    let dx = (x - cx).abs();
    let dy = (y - cy).abs();
    let max_d = cx.max(1.0 - cx) + cy.max(1.0 - cy);
    if max_d > 0.0 {
        ((dx + dy) / max_d).clamp(0.0, 1.0)
    } else {
        0.0
    }
}
