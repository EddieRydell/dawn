use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use super::fixture::{Controller, FixtureDef, FixtureGroup, FixtureId, Patch};
use super::timeline::Sequence;

/// 2D position for preview rendering. Coordinates are normalized (0.0 to 1.0).
#[derive(Debug, Clone, Copy, Serialize, Deserialize, TS, JsonSchema)]
#[ts(export)]
pub struct Position2D {
    pub x: f32,
    pub y: f32,
}

/// Describes the geometric shape used to distribute fixture pixels in the layout.
#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
#[ts(export)]
pub enum LayoutShape {
    Line {
        start: Position2D,
        end: Position2D,
    },
    Arc {
        center: Position2D,
        radius: f32,
        start_angle: f32,
        end_angle: f32,
    },
    Rectangle {
        top_left: Position2D,
        bottom_right: Position2D,
    },
    Grid {
        top_left: Position2D,
        bottom_right: Position2D,
        columns: u32,
    },
    Custom,
}

impl LayoutShape {
    /// Generate evenly-distributed positions for `pixel_count` pixels along this shape.
    /// Returns `None` for `Custom` (positions are user-placed individually).
    #[allow(clippy::cast_precision_loss)]
    pub fn generate_positions(&self, pixel_count: usize) -> Option<Vec<Position2D>> {
        if pixel_count == 0 {
            return Some(Vec::new());
        }
        match self {
            LayoutShape::Line { start, end } => {
                let positions = (0..pixel_count)
                    .map(|i| {
                        let t = if pixel_count > 1 {
                            i as f32 / (pixel_count - 1) as f32
                        } else {
                            0.5
                        };
                        Position2D {
                            x: start.x + (end.x - start.x) * t,
                            y: start.y + (end.y - start.y) * t,
                        }
                    })
                    .collect();
                Some(positions)
            }
            LayoutShape::Arc {
                center,
                radius,
                start_angle,
                end_angle,
            } => {
                let positions = (0..pixel_count)
                    .map(|i| {
                        let t = if pixel_count > 1 {
                            i as f32 / (pixel_count - 1) as f32
                        } else {
                            0.5
                        };
                        let angle = start_angle + (end_angle - start_angle) * t;
                        Position2D {
                            x: center.x + radius * angle.cos(),
                            y: center.y + radius * angle.sin(),
                        }
                    })
                    .collect();
                Some(positions)
            }
            LayoutShape::Rectangle {
                top_left,
                bottom_right,
            } => {
                // Distribute pixels evenly around the rectangle perimeter
                let w = (bottom_right.x - top_left.x).abs();
                let h = (bottom_right.y - top_left.y).abs();
                let perimeter = 2.0 * (w + h);
                if perimeter == 0.0 {
                    return Some(vec![
                        Position2D {
                            x: top_left.x,
                            y: top_left.y,
                        };
                        pixel_count
                    ]);
                }
                let positions = (0..pixel_count)
                    .map(|i| {
                        let t = if pixel_count > 1 {
                            i as f32 / pixel_count as f32
                        } else {
                            0.0
                        };
                        let d = t * perimeter;
                        if d < w {
                            Position2D {
                                x: top_left.x + d,
                                y: top_left.y,
                            }
                        } else if d < w + h {
                            Position2D {
                                x: bottom_right.x,
                                y: top_left.y + (d - w),
                            }
                        } else if d < 2.0 * w + h {
                            Position2D {
                                x: bottom_right.x - (d - w - h),
                                y: bottom_right.y,
                            }
                        } else {
                            Position2D {
                                x: top_left.x,
                                y: bottom_right.y - (d - 2.0 * w - h),
                            }
                        }
                    })
                    .collect();
                Some(positions)
            }
            LayoutShape::Grid {
                top_left,
                bottom_right,
                columns,
            } => {
                let cols = (*columns).max(1) as usize;
                let rows = pixel_count.div_ceil(cols);
                let positions = (0..pixel_count)
                    .map(|i| {
                        let col = i % cols;
                        let row = i / cols;
                        let tx = if cols > 1 {
                            col as f32 / (cols - 1) as f32
                        } else {
                            0.5
                        };
                        let ty = if rows > 1 {
                            row as f32 / (rows - 1) as f32
                        } else {
                            0.5
                        };
                        Position2D {
                            x: top_left.x + (bottom_right.x - top_left.x) * tx,
                            y: top_left.y + (bottom_right.y - top_left.y) * ty,
                        }
                    })
                    .collect();
                Some(positions)
            }
            LayoutShape::Custom => None,
        }
    }
}

fn default_layout_shape() -> LayoutShape {
    LayoutShape::Custom
}

/// Maps a fixture's pixels to 2D positions for the preview renderer.
#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
#[ts(export)]
pub struct FixtureLayout {
    pub fixture_id: FixtureId,
    /// One position per pixel. Length must equal the fixture's pixel_count.
    pub pixel_positions: Vec<Position2D>,
    #[serde(default = "default_layout_shape")]
    pub shape: LayoutShape,
}

impl FixtureLayout {
    /// Check if `pixel_positions` length matches the expected pixel count for this fixture.
    /// The evaluator handles mismatches gracefully (falls back to evenly-spaced positions),
    /// but callers can use this to detect and warn about inconsistencies.
    pub fn validate_pixel_count(&self, expected: usize) -> bool {
        self.pixel_positions.len() == expected
    }
}

/// The spatial layout of all fixtures for preview rendering.
#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
#[ts(export)]
pub struct Layout {
    pub fixtures: Vec<FixtureLayout>,
}

/// The top-level show model. Contains everything needed to describe and render a light show.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct Show {
    pub name: String,
    pub fixtures: Vec<FixtureDef>,
    pub groups: Vec<FixtureGroup>,
    pub layout: Layout,
    pub sequences: Vec<Sequence>,
    /// Output patching: maps fixtures to physical controller outputs.
    pub patches: Vec<Patch>,
    /// Physical controllers in this show's setup.
    pub controllers: Vec<Controller>,
}

impl Show {
    /// Create an empty show with no fixtures, sequences, or controllers.
    pub fn empty() -> Self {
        Self {
            name: String::new(),
            fixtures: Vec::new(),
            groups: Vec::new(),
            layout: Layout {
                fixtures: Vec::new(),
            },
            sequences: Vec::new(),
            patches: Vec::new(),
            controllers: Vec::new(),
        }
    }
}
