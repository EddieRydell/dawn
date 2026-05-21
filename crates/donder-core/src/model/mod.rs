pub mod analysis;
pub mod analysis_document;
pub mod annotations;
pub mod automation;
pub mod blend_mode;
pub mod color;
pub mod color_gradient;
pub mod curve;
pub mod easing;
pub mod effect_kind;
pub mod fixture;
pub mod marks;
pub mod motion_path;
pub mod params;
pub mod show;
pub mod time_range;
pub mod timeline;

// Re-export commonly used types at the model level.
pub use analysis::{
    AnalysisFeatureKind, AnalysisFeatures, ChordEvent, DrumAnalysis, HarmonyAnalysis, MoodAnalysis,
    PythonEnvStatus, VocalPresence, VocalSegment,
};
pub use analysis_document::{
    AnalysisDocument, AnalysisProvenance, AudioUnderstanding, BeatGridRaw, DescriptorWindow,
    DrumEventsRaw, EmbeddingTrack, EventHit, LowLevelRaw, LyricsRaw, ModelRunInfo, NamedStem,
    NoteSpan, PhraseSpan, PitchRaw, RawAnalysisBundle, SalienceTrack, SectionLabelSpan,
    SemanticRaw, StemSeparationRaw, StructureRaw, UnderstandingDescriptor, UnderstandingHypothesis,
    UnderstandingHypothesisKind, UnderstandingSourceLayer, UserEditRecord, VocalActivityRaw,
    WordSpan, ANALYSIS_DOCUMENT_VERSION, AUDIO_UNDERSTANDING_VERSION,
};
pub use annotations::{AnnotationLayer, LayerOrigin, SongAnnotations};
pub use automation::{AutomationClip, ClipId};
pub use blend_mode::BlendMode;
pub use color::Color;
pub use color_gradient::{ColorGradient, ColorStop};
pub use curve::{Curve, CurvePoint};
pub use easing::EasingFunction;
pub use effect_kind::{BuiltInEffect, EffectKind};
pub use fixture::{
    BulbShape, ChannelOrder, ColorModel, Controller, ControllerId, ControllerProtocol,
    E131UniverseSize, FixtureDef, FixtureGroup, FixtureId, GroupId, GroupMember, OutputMapping,
    Patch, PixelType, Universe,
};
pub use marks::{
    CurveMarkKind, CurveTrack, EventTrack, MarkDomain, MarkSource, MarkTimeline, MarkTrack,
    MusicalRole, PointMark, PointMarkKind, SpanMark, SpanMarkKind, SpanTrack,
    MARK_TIMELINE_VERSION,
};
pub use motion_path::{LoopMode, MotionPath, Waypoint};
pub use params::{
    ColorMode, EffectParams, ParamKey, ParamSchema, ParamType, ParamValue, WipeDirection,
};
pub use show::{FixtureLayout, Layout, LayoutShape, Position2D, Show};
pub use time_range::TimeRange;
pub use timeline::{EffectId, EffectInstance, NodeId, NodeTimeline, Sequence, TrackItem};
