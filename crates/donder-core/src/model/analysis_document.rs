use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use super::analysis::{DrumAnalysis, HarmonyAnalysis, MoodAnalysis, VocalPresence};
use super::{MarkTimeline, MusicalRole, TimeRange};

pub const ANALYSIS_DOCUMENT_VERSION: u32 = 1;
pub const AUDIO_UNDERSTANDING_VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema, TS)]
#[ts(export)]
pub struct AnalysisDocument {
    pub version: u32,
    pub raw: RawAnalysisBundle,
    pub marks: Option<MarkTimeline>,
    pub understanding: Option<AudioUnderstanding>,
    pub provenance: AnalysisProvenance,
}

impl Default for AnalysisDocument {
    fn default() -> Self {
        Self {
            version: ANALYSIS_DOCUMENT_VERSION,
            raw: RawAnalysisBundle::default(),
            marks: None,
            understanding: None,
            provenance: AnalysisProvenance::default(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema, TS)]
#[ts(export)]
pub struct AudioUnderstanding {
    pub version: u32,
    pub summary: Option<String>,
    pub focus_prompt: Option<String>,
    pub descriptors: Vec<UnderstandingDescriptor>,
    pub source_layers: Vec<UnderstandingSourceLayer>,
    pub hypotheses: Vec<UnderstandingHypothesis>,
}

impl Default for AudioUnderstanding {
    fn default() -> Self {
        Self {
            version: AUDIO_UNDERSTANDING_VERSION,
            summary: None,
            focus_prompt: None,
            descriptors: Vec::new(),
            source_layers: Vec::new(),
            hypotheses: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema, TS)]
#[ts(export)]
pub struct UnderstandingDescriptor {
    pub id: String,
    pub label: String,
    pub tags: Vec<String>,
    pub confidence: Option<f32>,
    pub time_range: TimeRange,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema, TS)]
#[ts(export)]
pub struct UnderstandingSourceLayer {
    pub id: String,
    pub label: String,
    pub role_hint: Option<MusicalRole>,
    pub confidence: f32,
    pub tags: Vec<String>,
    pub related_track_ids: Vec<String>,
    pub time_range: Option<TimeRange>,
    pub instrument_name: Option<String>,
    pub instrument_program: Option<u32>,
    pub note_count: u32,
    pub descriptor_count: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[ts(export)]
#[serde(rename_all = "snake_case")]
pub enum UnderstandingHypothesisKind {
    SourceRole,
    Texture,
    Motion,
    Phrase,
    Descriptor,
    InstrumentLayer,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema, TS)]
#[ts(export)]
pub struct UnderstandingHypothesis {
    pub id: String,
    pub label: String,
    pub kind: UnderstandingHypothesisKind,
    pub confidence: f32,
    pub tags: Vec<String>,
    pub related_track_ids: Vec<String>,
    pub time_range: Option<TimeRange>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema, TS)]
#[ts(export)]
pub struct RawAnalysisBundle {
    pub beat_grid: Option<BeatGridRaw>,
    pub structure: Option<StructureRaw>,
    pub stems: Option<StemSeparationRaw>,
    pub lyrics: Option<LyricsRaw>,
    pub pitch: Option<PitchRaw>,
    pub drum_events: Option<DrumEventsRaw>,
    pub vocal_activity: Option<VocalActivityRaw>,
    pub low_level: Option<LowLevelRaw>,
    pub semantic: Option<SemanticRaw>,
    pub mood: Option<MoodAnalysis>,
    pub harmony: Option<HarmonyAnalysis>,
    pub drums: Option<DrumAnalysis>,
    pub vocal_presence: Option<VocalPresence>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema, TS)]
#[ts(export)]
pub struct AnalysisProvenance {
    pub generated_at: Option<String>,
    pub pipeline_version: Option<String>,
    pub models: Vec<ModelRunInfo>,
    pub source_audio_path: Option<String>,
    pub source_audio_hash: Option<String>,
    pub gpu_used: bool,
    pub user_edits: Vec<UserEditRecord>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema, TS)]
#[ts(export)]
pub struct ModelRunInfo {
    pub stage: String,
    pub model_name: String,
    pub model_version: Option<String>,
    pub provider: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema, TS)]
#[ts(export)]
pub struct UserEditRecord {
    pub edited_at: String,
    pub target: String,
    pub operation: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema, TS)]
#[ts(export)]
pub struct BeatGridRaw {
    pub beats: Vec<f64>,
    pub downbeats: Vec<f64>,
    pub subdivisions_8: Vec<f64>,
    pub subdivisions_16: Vec<f64>,
    pub tempo_bpm: f64,
    pub beat_confidences: Vec<f32>,
    pub tempo_confidence: f32,
    pub time_signature_num: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema, TS)]
#[ts(export)]
pub struct StructureRaw {
    pub sections: Vec<SectionLabelSpan>,
    pub phrases: Vec<PhraseSpan>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema, TS)]
#[ts(export)]
pub struct StemSeparationRaw {
    pub vocals: Option<String>,
    pub drums: Option<String>,
    pub bass: Option<String>,
    pub other: Option<String>,
    pub extra: Vec<NamedStem>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema, TS)]
#[ts(export)]
pub struct LyricsRaw {
    pub full_text: String,
    pub language: String,
    pub words: Vec<WordSpan>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema, TS)]
#[ts(export)]
pub struct PitchRaw {
    pub notes: Vec<NoteSpan>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema, TS)]
#[ts(export)]
pub struct DrumEventsRaw {
    pub kicks: Vec<EventHit>,
    pub snares: Vec<EventHit>,
    pub hats: Vec<EventHit>,
    pub other_hits: Vec<EventHit>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema, TS)]
#[ts(export)]
pub struct VocalActivityRaw {
    pub segments: Vec<PhraseSpan>,
    pub onsets: Vec<EventHit>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema, TS)]
#[ts(export)]
pub struct LowLevelRaw {
    pub sample_rate_hz: f32,
    pub rms: Vec<f32>,
    pub onset_strength: Vec<f32>,
    pub spectral_centroid: Vec<f32>,
    pub chroma: Vec<f32>,
    pub chroma_length: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema, TS)]
#[ts(export)]
pub struct SemanticRaw {
    pub embedding_tracks: Vec<EmbeddingTrack>,
    pub descriptor_windows: Vec<DescriptorWindow>,
    pub salience_tracks: Vec<SalienceTrack>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema, TS)]
#[ts(export)]
pub struct NamedStem {
    pub name: String,
    pub path: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema, TS)]
#[ts(export)]
pub struct WordSpan {
    pub word: String,
    #[serde(flatten)]
    pub time_range: TimeRange,
    pub confidence: f32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema, TS)]
#[ts(export)]
pub struct NoteSpan {
    pub midi_note: u32,
    #[serde(flatten)]
    pub time_range: TimeRange,
    pub velocity: f32,
    pub instrument_name: Option<String>,
    pub instrument_program: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema, TS)]
#[ts(export)]
pub struct EventHit {
    pub time: f64,
    pub strength: f32,
    pub confidence: Option<f32>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema, TS)]
#[ts(export)]
pub struct EmbeddingTrack {
    pub id: String,
    pub name: String,
    pub model: Option<String>,
    pub sample_rate_hz: f32,
    pub dimensions: u32,
    pub values: Vec<f32>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema, TS)]
#[ts(export)]
pub struct DescriptorWindow {
    #[serde(flatten)]
    pub time_range: TimeRange,
    pub tags: Vec<String>,
    pub confidence: Option<f32>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema, TS)]
#[ts(export)]
pub struct SalienceTrack {
    pub id: String,
    pub name: String,
    pub sample_rate_hz: f32,
    pub values: Vec<f32>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema, TS)]
#[ts(export)]
pub struct SectionLabelSpan {
    pub label: String,
    #[serde(flatten)]
    pub time_range: TimeRange,
    pub confidence: f32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema, TS)]
#[ts(export)]
pub struct PhraseSpan {
    #[serde(flatten)]
    pub time_range: TimeRange,
    pub confidence: Option<f32>,
}

#[cfg(test)]
#[allow(clippy::expect_used)]
mod tests {
    use super::*;
    use crate::model::{
        AnalysisFeatureKind, EventTrack, MarkDomain, MarkSource, MarkTimeline, MarkTrack,
        MusicalRole, PointMark, PointMarkKind,
    };

    fn time_range(start: f64, end: f64) -> TimeRange {
        TimeRange::new(start, end).expect("valid test time range")
    }

    #[test]
    fn default_document_uses_v1_and_empty_raw() {
        let doc = AnalysisDocument::default();
        assert_eq!(doc.version, ANALYSIS_DOCUMENT_VERSION);
        assert_eq!(doc.raw, RawAnalysisBundle::default());
        assert!(doc.marks.is_none());
        assert!(doc.understanding.is_none());
        assert_eq!(doc.provenance, AnalysisProvenance::default());
    }

    #[test]
    fn analysis_document_roundtrips_through_json() {
        let document = AnalysisDocument {
            version: ANALYSIS_DOCUMENT_VERSION,
            raw: RawAnalysisBundle {
                beat_grid: Some(BeatGridRaw {
                    beats: vec![0.5, 1.0, 1.5],
                    downbeats: vec![0.5],
                    subdivisions_8: vec![0.25, 0.5, 0.75],
                    subdivisions_16: vec![0.125, 0.25, 0.375],
                    tempo_bpm: 128.0,
                    beat_confidences: vec![0.9, 0.8, 0.85],
                    tempo_confidence: 0.91,
                    time_signature_num: 4,
                }),
                structure: Some(StructureRaw {
                    sections: vec![SectionLabelSpan {
                        label: "drop".into(),
                        time_range: time_range(32.0, 64.0),
                        confidence: 0.84,
                    }],
                    phrases: vec![PhraseSpan {
                        time_range: time_range(32.0, 40.0),
                        confidence: Some(0.73),
                    }],
                }),
                stems: Some(StemSeparationRaw {
                    vocals: Some("stems/vocals.wav".into()),
                    drums: Some("stems/drums.wav".into()),
                    bass: Some("stems/bass.wav".into()),
                    other: Some("stems/other.wav".into()),
                    extra: vec![NamedStem {
                        name: "lead".into(),
                        path: "stems/lead.wav".into(),
                    }],
                }),
                lyrics: Some(LyricsRaw {
                    full_text: "lift me higher".into(),
                    language: "en".into(),
                    words: vec![WordSpan {
                        word: "lift".into(),
                        time_range: time_range(10.0, 10.5),
                        confidence: 0.96,
                    }],
                }),
                pitch: Some(PitchRaw {
                    notes: vec![NoteSpan {
                        midi_note: 64,
                        time_range: time_range(12.0, 12.5),
                        velocity: 0.88,
                        instrument_name: Some("Lead Synth".into()),
                        instrument_program: Some(81),
                    }],
                }),
                drum_events: Some(DrumEventsRaw {
                    kicks: vec![EventHit {
                        time: 1.0,
                        strength: 0.95,
                        confidence: Some(0.9),
                    }],
                    snares: vec![EventHit {
                        time: 1.5,
                        strength: 0.82,
                        confidence: Some(0.86),
                    }],
                    hats: vec![],
                    other_hits: vec![],
                }),
                vocal_activity: Some(VocalActivityRaw {
                    segments: vec![PhraseSpan {
                        time_range: time_range(9.5, 16.0),
                        confidence: Some(0.8),
                    }],
                    onsets: vec![EventHit {
                        time: 10.0,
                        strength: 0.7,
                        confidence: Some(0.75),
                    }],
                }),
                low_level: Some(LowLevelRaw {
                    sample_rate_hz: 86.13281,
                    rms: vec![0.2, 0.4, 0.6],
                    onset_strength: vec![0.1, 0.5, 0.2],
                    spectral_centroid: vec![1200.0, 1800.0],
                    chroma: vec![0.1, 0.2, 0.3, 0.4],
                    chroma_length: 2,
                }),
                semantic: Some(SemanticRaw {
                    embedding_tracks: vec![EmbeddingTrack {
                        id: "music_embedding".into(),
                        name: "Music Embedding".into(),
                        model: Some("beats-like".into()),
                        sample_rate_hz: 2.0,
                        dimensions: 2,
                        values: vec![0.1, 0.2, 0.3, 0.4],
                    }],
                    descriptor_windows: vec![DescriptorWindow {
                        time_range: time_range(32.0, 36.0),
                        tags: vec!["metallic".into(), "dense".into()],
                        confidence: Some(0.79),
                    }],
                    salience_tracks: vec![SalienceTrack {
                        id: "lead_salience".into(),
                        name: "Lead Salience".into(),
                        sample_rate_hz: 8.0,
                        values: vec![0.2, 0.6, 0.9],
                    }],
                }),
                mood: Some(crate::model::analysis::MoodAnalysis {
                    valence: 0.7,
                    arousal: 0.8,
                    danceability: 0.9,
                    genres: std::collections::HashMap::new(),
                }),
                harmony: Some(crate::model::analysis::HarmonyAnalysis {
                    key: "F minor".into(),
                    key_confidence: 0.81,
                    chords: Vec::new(),
                }),
                drums: None,
                vocal_presence: None,
            },
            marks: Some(MarkTimeline {
                version: 1,
                tracks: vec![MarkTrack::Events(EventTrack {
                    id: "lead.main_onsets".into(),
                    name: "Lead Onsets".into(),
                    domain: MarkDomain::Melody,
                    role: Some(MusicalRole::Lead),
                    source: Some(MarkSource::Model),
                    confidence: 0.86,
                    derived_from: vec![AnalysisFeatureKind::Pitch],
                    events: vec![PointMark {
                        id: "lead_1".into(),
                        kind: PointMarkKind::MelodyOnset,
                        time: 33.125,
                        strength: Some(0.74),
                        confidence: Some(0.83),
                        tags: vec!["foreground".into(), "bright".into()],
                    }],
                })],
            }),
            understanding: Some(AudioUnderstanding {
                version: AUDIO_UNDERSTANDING_VERSION,
                summary: Some("Lead-like melody with metallic texture".into()),
                focus_prompt: Some("Find the lead line and key textures".into()),
                descriptors: vec![UnderstandingDescriptor {
                    id: "descriptor_1".into(),
                    label: "Metallic Texture".into(),
                    tags: vec!["metallic".into(), "dense".into()],
                    confidence: Some(0.79),
                    time_range: time_range(32.0, 36.0),
                }],
                source_layers: vec![UnderstandingSourceLayer {
                    id: "layer_1".into(),
                    label: "Lead Synth Layer".into(),
                    role_hint: Some(MusicalRole::Lead),
                    confidence: 0.81,
                    tags: vec!["melody".into(), "synth".into()],
                    related_track_ids: vec!["lead.main_onsets".into()],
                    time_range: Some(time_range(32.0, 36.0)),
                    instrument_name: Some("Lead Synth".into()),
                    instrument_program: Some(81),
                    note_count: 1,
                    descriptor_count: 1,
                }],
                hypotheses: vec![UnderstandingHypothesis {
                    id: "source.lead".into(),
                    label: "Lead-like melody line".into(),
                    kind: UnderstandingHypothesisKind::SourceRole,
                    confidence: 0.86,
                    tags: vec!["melody".into(), "foreground".into()],
                    related_track_ids: vec!["lead.main_onsets".into()],
                    time_range: Some(time_range(32.0, 36.0)),
                }],
            }),
            provenance: AnalysisProvenance {
                generated_at: Some("2026-04-29T12:34:56Z".into()),
                pipeline_version: Some("analysis-v2".into()),
                models: vec![ModelRunInfo {
                    stage: "lead_onsets".into(),
                    model_name: "beats-like-encoder".into(),
                    model_version: Some("0.1.0".into()),
                    provider: Some("local".into()),
                }],
                source_audio_path: Some("media/song.wav".into()),
                source_audio_hash: Some("sha256:abc123".into()),
                gpu_used: true,
                user_edits: vec![UserEditRecord {
                    edited_at: "2026-04-29T13:00:00Z".into(),
                    target: "grid.beats".into(),
                    operation: "insert_event".into(),
                }],
            },
        };

        let json = serde_json::to_string(&document).expect("serialize document");
        let roundtrip: AnalysisDocument =
            serde_json::from_str(&json).expect("deserialize document");

        assert_eq!(roundtrip, document);
    }

    #[test]
    fn analysis_document_serializes_version_field() {
        let value = serde_json::to_value(AnalysisDocument::default()).expect("serialize");
        assert_eq!(
            value.get("version").and_then(serde_json::Value::as_u64),
            Some(ANALYSIS_DOCUMENT_VERSION as u64)
        );
    }
}
