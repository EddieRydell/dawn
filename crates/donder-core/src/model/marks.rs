use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use super::{AnalysisFeatureKind, TimeRange};

pub const MARK_TIMELINE_VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema, TS)]
#[ts(export)]
pub struct MarkTimeline {
    pub version: u32,
    pub tracks: Vec<MarkTrack>,
}

impl Default for MarkTimeline {
    fn default() -> Self {
        Self {
            version: MARK_TIMELINE_VERSION,
            tracks: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema, TS)]
#[ts(export)]
#[serde(tag = "track_type", rename_all = "snake_case")]
pub enum MarkTrack {
    Events(EventTrack),
    Spans(SpanTrack),
    Curve(CurveTrack),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[ts(export)]
#[serde(rename_all = "snake_case")]
pub enum MarkDomain {
    Grid,
    Rhythm,
    Melody,
    Bass,
    Vocal,
    Texture,
    Structure,
    Cue,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[ts(export)]
#[serde(rename_all = "snake_case")]
pub enum MusicalRole {
    Lead,
    Support,
    Bass,
    Drums,
    Vocal,
    Pad,
    Fx,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[ts(export)]
#[serde(rename_all = "snake_case")]
pub enum MarkSource {
    Mix,
    VocalsStem,
    DrumsStem,
    BassStem,
    OtherStem,
    Model,
    User,
    Derived,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema, TS)]
#[ts(export)]
pub struct EventTrack {
    pub id: String,
    pub name: String,
    pub domain: MarkDomain,
    pub role: Option<MusicalRole>,
    pub source: Option<MarkSource>,
    pub confidence: f32,
    pub derived_from: Vec<AnalysisFeatureKind>,
    pub events: Vec<PointMark>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema, TS)]
#[ts(export)]
pub struct SpanTrack {
    pub id: String,
    pub name: String,
    pub domain: MarkDomain,
    pub role: Option<MusicalRole>,
    pub source: Option<MarkSource>,
    pub confidence: f32,
    pub derived_from: Vec<AnalysisFeatureKind>,
    pub spans: Vec<SpanMark>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema, TS)]
#[ts(export)]
pub struct CurveTrack {
    pub id: String,
    pub name: String,
    pub kind: CurveMarkKind,
    pub source: Option<MarkSource>,
    pub confidence: f32,
    pub sample_rate_hz: f32,
    pub values: Vec<f32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[ts(export)]
#[serde(rename_all = "snake_case")]
pub enum PointMarkKind {
    Beat,
    Downbeat,
    Subdivision8,
    Subdivision16,
    Kick,
    Snare,
    Hat,
    BassHit,
    MelodyOnset,
    VocalOnset,
    Accent,
    Impact,
    FillHit,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[ts(export)]
#[serde(rename_all = "snake_case")]
pub enum SpanMarkKind {
    Note,
    SectionIntro,
    SectionVerse,
    SectionChorus,
    SectionBridge,
    SectionDrop,
    Phrase,
    Build,
    Breakdown,
    LeadPhrase,
    VocalPhrase,
    TextureWindow,
    Sustain,
    Call,
    Response,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[ts(export)]
#[serde(rename_all = "snake_case")]
pub enum CurveMarkKind {
    Energy,
    Tension,
    RhythmicDensity,
    SpectralBrightness,
    SpectralNoisiness,
    LeadSalience,
    BassSalience,
    VocalSalience,
    StereoWidth,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema, TS)]
#[ts(export)]
pub struct PointMark {
    pub id: String,
    pub kind: PointMarkKind,
    pub time: f64,
    pub strength: Option<f32>,
    pub confidence: Option<f32>,
    pub tags: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema, TS)]
#[ts(export)]
pub struct SpanMark {
    pub id: String,
    pub kind: SpanMarkKind,
    #[serde(flatten)]
    pub time_range: TimeRange,
    pub strength: Option<f32>,
    pub confidence: Option<f32>,
    pub tags: Vec<String>,
}

#[cfg(test)]
#[allow(clippy::expect_used)]
mod tests {
    use super::*;

    fn time_range(start: f64, end: f64) -> TimeRange {
        TimeRange::new(start, end).expect("valid test time range")
    }

    #[test]
    fn default_timeline_uses_v1_and_has_no_tracks() {
        let timeline = MarkTimeline::default();
        assert_eq!(timeline.version, MARK_TIMELINE_VERSION);
        assert!(timeline.tracks.is_empty());
    }

    #[test]
    fn mark_timeline_roundtrips_through_json() {
        let timeline = MarkTimeline {
            version: MARK_TIMELINE_VERSION,
            tracks: vec![
                MarkTrack::Events(EventTrack {
                    id: "grid.beats".into(),
                    name: "Beat Grid".into(),
                    domain: MarkDomain::Grid,
                    role: None,
                    source: Some(MarkSource::Derived),
                    confidence: 0.95,
                    derived_from: vec![AnalysisFeatureKind::Beats],
                    events: vec![PointMark {
                        id: "beat_1".into(),
                        kind: PointMarkKind::Beat,
                        time: 1.25,
                        strength: Some(1.0),
                        confidence: Some(0.99),
                        tags: vec!["downbeat_candidate".into()],
                    }],
                }),
                MarkTrack::Spans(SpanTrack {
                    id: "structure.sections".into(),
                    name: "Sections".into(),
                    domain: MarkDomain::Structure,
                    role: None,
                    source: Some(MarkSource::Model),
                    confidence: 0.82,
                    derived_from: vec![AnalysisFeatureKind::Structure],
                    spans: vec![SpanMark {
                        id: "section_1".into(),
                        kind: SpanMarkKind::SectionVerse,
                        time_range: time_range(0.0, 16.0),
                        strength: Some(0.7),
                        confidence: Some(0.88),
                        tags: vec!["verse".into()],
                    }],
                }),
                MarkTrack::Curve(CurveTrack {
                    id: "curve.energy".into(),
                    name: "Energy".into(),
                    kind: CurveMarkKind::Energy,
                    source: Some(MarkSource::Derived),
                    confidence: 0.77,
                    sample_rate_hz: 8.0,
                    values: vec![0.1, 0.5, 0.9],
                }),
            ],
        };

        let json = serde_json::to_string(&timeline).expect("serialize timeline");
        let roundtrip: MarkTimeline = serde_json::from_str(&json).expect("deserialize timeline");

        assert_eq!(roundtrip, timeline);
    }

    #[test]
    fn track_type_discriminator_is_snake_case() {
        let track = MarkTrack::Curve(CurveTrack {
            id: "curve.energy".into(),
            name: "Energy".into(),
            kind: CurveMarkKind::Energy,
            source: Some(MarkSource::Derived),
            confidence: 0.5,
            sample_rate_hz: 4.0,
            values: vec![0.25, 0.5],
        });

        let value = serde_json::to_value(track).expect("serialize track");
        assert_eq!(
            value.get("track_type").and_then(serde_json::Value::as_str),
            Some("curve")
        );
    }
}
