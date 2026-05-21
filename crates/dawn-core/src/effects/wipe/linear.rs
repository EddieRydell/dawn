use crate::model::show::Position2D;

/// Project a 2D position onto 1D for horizontal wipe (uses X axis).
pub fn project_horizontal(pos: Position2D) -> f64 {
    f64::from(pos.x)
}

/// Project a 2D position onto 1D for vertical wipe (uses Y axis).
pub fn project_vertical(pos: Position2D) -> f64 {
    f64::from(pos.y)
}
