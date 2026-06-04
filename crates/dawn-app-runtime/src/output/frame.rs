use dawn_language::model::{Color, Distance, DistanceSpan, FixtureId};
use dawn_language::render::{GeometryRenderBounds, GeometryRenderPoint};

#[derive(Debug, Clone)]
pub struct OutputFrame {
    pub source: OutputSourceMetadata,
    pub time_seconds: f64,
    pub generation: u64,
    pub status: OutputFrameStatus,
    pub bounds: GeometryRenderBounds,
    pub fixtures: Vec<OutputFixtureFrame>,
}

#[derive(Debug, Clone)]
pub struct OutputSourceMetadata {
    pub label: String,
    pub kind: OutputSourceKind,
    pub duration_seconds: f64,
    pub fps: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputSourceKind {
    Sequence,
    Empty,
}

#[derive(Debug, Clone)]
pub enum OutputFrameStatus {
    Live,
    Idle(String),
    Error(String),
}

#[derive(Debug, Clone)]
pub struct OutputFixtureFrame {
    pub id: FixtureId,
    pub name: String,
    pub bulb_radius: DistanceSpan,
    pub pixels: Vec<OutputPixelFrame>,
}

#[derive(Debug, Clone)]
pub struct OutputPixelFrame {
    pub position: GeometryRenderPoint,
    pub color: Color,
}

pub trait OutputSink {
    fn write_frame(&self, frame: OutputFrame);
}

pub fn empty_frame(generation: u64, message: impl Into<String>) -> OutputFrame {
    OutputFrame {
        source: OutputSourceMetadata {
            label: "No preview source".to_string(),
            kind: OutputSourceKind::Empty,
            duration_seconds: 0.0,
            fps: 0,
        },
        time_seconds: 0.0,
        generation,
        status: OutputFrameStatus::Idle(message.into()),
        bounds: GeometryRenderBounds {
            min_x: Distance::from_micrometers(-5_000_000),
            min_y: Distance::from_micrometers(-4_000_000),
            max_x: Distance::from_micrometers(5_000_000),
            max_y: Distance::from_micrometers(4_000_000),
        },
        fixtures: Vec::new(),
    }
}
