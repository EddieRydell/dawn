use crate::model::show::Position2D;

/// Project a 2D position onto 1D for diagonal-up wipe (bottom-left to top-right).
pub fn project_diagonal_up(pos: Position2D) -> f64 {
    let x = f64::from(pos.x);
    let y = f64::from(pos.y);
    ((x + (1.0 - y)) * 0.5).clamp(0.0, 1.0)
}

/// Project a 2D position onto 1D for diagonal-down wipe (top-left to bottom-right).
pub fn project_diagonal_down(pos: Position2D) -> f64 {
    let x = f64::from(pos.x);
    let y = f64::from(pos.y);
    ((x + y) * 0.5).clamp(0.0, 1.0)
}
