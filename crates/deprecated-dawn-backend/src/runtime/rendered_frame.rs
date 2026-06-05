use dawn_language::model::{Color, DistanceSpan, FixtureId};
use dawn_language::render::{GeometryRenderBounds, GeometryRenderPoint};

use crate::output::sequence::{
    OutputFixtureFrame, OutputFrame, OutputFrameStatus, OutputPixelFrame, OutputSourceKind,
    OutputSourceMetadata,
};

#[derive(Debug, Clone)]
pub struct RenderedFrame {
    pub source: RenderedFrameSource,
    pub time_seconds: f64,
    pub generation: u64,
    pub status: RenderedFrameStatus,
    pub bounds: GeometryRenderBounds,
    pub fixtures: Vec<RenderedFixtureFrame>,
}

#[derive(Debug, Clone)]
pub struct RenderedFrameSource {
    pub label: String,
    pub kind: RenderedFrameSourceKind,
    pub duration_seconds: f64,
    pub fps: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RenderedFrameSourceKind {
    Sequence,
    Empty,
}

#[derive(Debug, Clone)]
pub enum RenderedFrameStatus {
    Live,
    Idle(String),
    Error(String),
}

#[derive(Debug, Clone)]
pub struct RenderedFixtureFrame {
    pub id: FixtureId,
    pub name: String,
    pub bulb_radius: DistanceSpan,
    pub pixels: Vec<RenderedPixelFrame>,
}

#[derive(Debug, Clone)]
pub struct RenderedPixelFrame {
    pub position: GeometryRenderPoint,
    pub color: Color,
}

impl From<OutputFrame> for RenderedFrame {
    fn from(value: OutputFrame) -> Self {
        Self {
            source: value.source.into(),
            time_seconds: value.time_seconds,
            generation: value.generation,
            status: value.status.into(),
            bounds: value.bounds,
            fixtures: value.fixtures.into_iter().map(Into::into).collect(),
        }
    }
}

impl From<&OutputFrame> for RenderedFrame {
    fn from(value: &OutputFrame) -> Self {
        value.clone().into()
    }
}

impl From<OutputSourceMetadata> for RenderedFrameSource {
    fn from(value: OutputSourceMetadata) -> Self {
        Self {
            label: value.label,
            kind: value.kind.into(),
            duration_seconds: value.duration_seconds,
            fps: value.fps,
        }
    }
}

impl From<OutputSourceKind> for RenderedFrameSourceKind {
    fn from(value: OutputSourceKind) -> Self {
        match value {
            OutputSourceKind::Sequence => Self::Sequence,
            OutputSourceKind::Empty => Self::Empty,
        }
    }
}

impl From<OutputFrameStatus> for RenderedFrameStatus {
    fn from(value: OutputFrameStatus) -> Self {
        match value {
            OutputFrameStatus::Live => Self::Live,
            OutputFrameStatus::Idle(message) => Self::Idle(message),
            OutputFrameStatus::Error(message) => Self::Error(message),
        }
    }
}

impl From<OutputFixtureFrame> for RenderedFixtureFrame {
    fn from(value: OutputFixtureFrame) -> Self {
        Self {
            id: value.id,
            name: value.name,
            bulb_radius: value.bulb_radius,
            pixels: value.pixels.into_iter().map(Into::into).collect(),
        }
    }
}

impl From<OutputPixelFrame> for RenderedPixelFrame {
    fn from(value: OutputPixelFrame) -> Self {
        Self {
            position: value.position,
            color: value.color,
        }
    }
}

impl From<RenderedFrame> for OutputFrame {
    fn from(value: RenderedFrame) -> Self {
        Self {
            source: OutputSourceMetadata {
                label: value.source.label,
                kind: value.source.kind.into(),
                duration_seconds: value.source.duration_seconds,
                fps: value.source.fps,
            },
            time_seconds: value.time_seconds,
            generation: value.generation,
            status: value.status.into(),
            bounds: value.bounds,
            fixtures: value.fixtures.into_iter().map(Into::into).collect(),
        }
    }
}

impl From<RenderedFrameSourceKind> for OutputSourceKind {
    fn from(value: RenderedFrameSourceKind) -> Self {
        match value {
            RenderedFrameSourceKind::Sequence => Self::Sequence,
            RenderedFrameSourceKind::Empty => Self::Empty,
        }
    }
}

impl From<RenderedFrameStatus> for OutputFrameStatus {
    fn from(value: RenderedFrameStatus) -> Self {
        match value {
            RenderedFrameStatus::Live => Self::Live,
            RenderedFrameStatus::Idle(message) => Self::Idle(message),
            RenderedFrameStatus::Error(message) => Self::Error(message),
        }
    }
}

impl From<RenderedFixtureFrame> for OutputFixtureFrame {
    fn from(value: RenderedFixtureFrame) -> Self {
        Self {
            id: value.id,
            name: value.name,
            bulb_radius: value.bulb_radius,
            pixels: value.pixels.into_iter().map(Into::into).collect(),
        }
    }
}

impl From<RenderedPixelFrame> for OutputPixelFrame {
    fn from(value: RenderedPixelFrame) -> Self {
        Self {
            position: value.position,
            color: value.color,
        }
    }
}
