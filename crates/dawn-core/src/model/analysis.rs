use std::collections::HashMap;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use super::TimeRange;

/// Provenance labels for which raw analysis sources contributed to a mark track.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[ts(export)]
#[serde(rename_all = "snake_case")]
pub enum AnalysisFeatureKind {
    Beats,
    Structure,
    Semantic,
    Mood,
    Harmony,
    Lyrics,
    Pitch,
    Drums,
    VocalPresence,
    LowLevel,
    Stems,
}

/// Boolean flags for which analysis stages to run.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize, JsonSchema, TS)]
#[ts(export)]
#[serde(default)]
pub struct AnalysisFeatures {
    pub beats: bool,
    pub structure: bool,
    pub stems: bool,
    pub lyrics: bool,
    pub mood: bool,
    pub harmony: bool,
    pub low_level: bool,
    pub pitch: bool,
    pub drums: bool,
    pub vocal_presence: bool,
}

impl AnalysisFeatures {
    pub fn all() -> Self {
        Self {
            beats: true,
            structure: true,
            stems: true,
            lyrics: true,
            mood: true,
            harmony: true,
            low_level: true,
            pitch: true,
            drums: true,
            vocal_presence: true,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema, TS)]
#[ts(export)]
pub struct MoodAnalysis {
    pub valence: f64,
    pub arousal: f64,
    pub danceability: f64,
    pub genres: HashMap<String, f64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema, TS)]
#[ts(export)]
pub struct HarmonyAnalysis {
    pub key: String,
    pub key_confidence: f64,
    pub chords: Vec<ChordEvent>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema, TS)]
#[ts(export)]
pub struct ChordEvent {
    pub label: String,
    #[serde(flatten)]
    pub time_range: TimeRange,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema, TS)]
#[ts(export)]
pub struct DrumAnalysis {
    pub onsets: Vec<f64>,
    pub strengths: Vec<f64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema, TS)]
#[ts(export)]
pub struct VocalPresence {
    pub segments: Vec<VocalSegment>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema, TS)]
#[ts(export)]
pub struct VocalSegment {
    #[serde(flatten)]
    pub time_range: TimeRange,
}

/// Runtime snapshot of the Python environment and sidecar status.
#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema, TS)]
#[ts(export)]
pub struct PythonEnvStatus {
    pub uv_available: bool,
    pub python_installed: bool,
    pub venv_exists: bool,
    pub deps_installed: bool,
    pub installed_models: Vec<String>,
    pub sidecar_running: bool,
    pub sidecar_port: u32,
    pub gpu_available: bool,
}
