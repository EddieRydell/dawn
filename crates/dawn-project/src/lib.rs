#![deny(unsafe_code)]
#![cfg_attr(
    not(test),
    deny(
        clippy::expect_used,
        clippy::panic,
        clippy::todo,
        clippy::unimplemented,
        clippy::unwrap_used
    )
)]

mod diagnostics;
mod effect_script;
mod fs;
mod load;
mod lower;
mod model;
mod parse;
mod path;

pub use diagnostics::{
    DiagnosticSeverity, ProjectDiagnostic, ProjectDiagnosticKind, ProjectLoadResult, TextPosition,
    TextRange,
};
pub use fs::WorkspaceFs;
pub use load::load_project;
pub use model::{
    ArrayElementType, AutomationClip, ChannelRange, Color, ColorModel, Controller,
    ControllerDestination, ControllerOutput, Curve, CurvePoint, CurveUse, CurveValue,
    CurveValueType, DawnProject, Display, Distance, DistanceSpan, EffectParam,
    EffectParamArrayValue, EffectTarget, Fixture, FixtureId, FixturePlacement, Flags, Geometry,
    Group, GroupInstantiationId, Layout, LayoutTargetKind, LayoutTargetRef, ObjectKind, Patch,
    Point3, Project, Protocol, RgbChannelOrder, Rotation3, Route, Scale3, Sequence,
    SequenceEffect, SequenceEffectId, SequenceEffectScope, SequenceMarkCollection, Time, TimeSpan,
    Transform, Universe,
};
pub use path::Utf8PathBuf;
