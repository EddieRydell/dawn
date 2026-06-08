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
mod save;

pub use diagnostics::{
    DiagnosticSeverity, ProjectDiagnostic, ProjectDiagnosticKind, TextPosition, TextRange,
};
pub use effect_script::{
    lex, parse_module, BytecodeStats, CompiledEffect, EffectAst, EffectModuleAst,
    EffectParamSchema, EffectSampleScratch, EffectScriptKind, EffectVisibility, FixtureContext,
    GeneratedChildEffectRef, GeneratedChildTopology, GeneratorTarget, GeneratorTargetPixel,
    ParamDefault, PixelContext, PreparedEffectParams, RuntimeArrayValue, RuntimeError,
    RuntimeMarks, RuntimeValue, ScriptDiagnostic, ScriptType, Token,
};
pub use fs::WorkspaceFs;
pub use fs::{WorkspaceEntry, WorkspaceEntryKind};
pub use load::{load_project, ProjectLoadResult};
pub use model::{
    ArrayElementType, AssetPath, Authored, AutomationClip, ChannelRange, Color, ColorModel,
    Controller, ControllerDefinitionKey, ControllerDestination, ControllerOutput, Curve,
    CurveDefinitionKey, CurvePoint, CurveUse, CurveValue, CurveValueType, DawnFile, DawnImport,
    DawnObject, DawnProject, DefinitionKey, Display, DisplayDefinitionKey, Distance, DistanceSpan,
    EffectDefinition, EffectDefinitionKey, EffectParam, EffectParamArrayValue, EffectTarget,
    Fixture, FixtureDefinitionKey, FixtureId, FixturePlacement, Flags, Geometry, Group,
    GroupInstantiationId, InlineOrRef, Layout, LayoutDefinitionKey, LayoutTargetKind,
    LayoutTargetRef, ObjectKind, Patch, PatchDefinitionKey, Point3, Project, ProjectDefinitionKey,
    Protocol, Resolved, ResolvedAssetPath, ResolvedEffectDefinitionSource, ResolvedInlineOrRef,
    ResolvedObject, ResolvedProvenance, ResolvedSourceFile, ResolvedSourceObject, ResolvedStores,
    ResolvedSymbolRef, RgbChannelOrder, Rotation3, Route, Scale3, Sequence, SequenceDefinitionKey,
    SequenceEffect, SequenceEffectId, SequenceEffectScope, SequenceMarkCollection, SymbolRef, Time,
    TimeSpan, Transform, Universe,
};
pub use path::{canonicalize_path, resolve_import_path, utf8_path, PathStringExt, Utf8PathBuf};
pub use save::{save_project, ProjectSaveResult};
