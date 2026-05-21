use indexmap::IndexMap;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use super::AnalysisDocument;

// ── Layer origin ─────────────────────────────────────────────────

/// Whether an annotation layer was produced by AI analysis or by a user.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[ts(export)]
#[serde(rename_all = "snake_case")]
pub enum LayerOrigin {
    Ai,
    User,
}

// ── Annotation layer ─────────────────────────────────────────────

/// A single named layer of analysis/annotation data.
///
/// Both AI-generated and user-created layers share the same structure:
/// an `AnalysisDocument`. This keeps raw model output, planner-facing marks,
/// and provenance together under the same editable layer abstraction.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct AnnotationLayer {
    /// Display name (also the key in `SongAnnotations.layers`).
    pub name: String,
    /// Whether this layer was produced by AI or created by a user.
    pub origin: LayerOrigin,
    /// The analysis/annotation data for this layer.
    pub data: AnalysisDocument,
}

// ── Song annotations container ───────────────────────────────────

/// Top-level container for all annotations associated with a song.
///
/// Stored as `{audio_file}.annotations.json` in the media directory.
/// Contains named layers of `AnalysisDocument` data. Each layer can be
/// AI-generated or user-created, with independent marks and provenance.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct SongAnnotations {
    /// Schema version for future migration support.
    pub version: u32,
    /// Which layer is currently active (read by backward-compat commands).
    /// `None` means no layer is selected; falls back to the first layer.
    pub active_layer: Option<String>,
    /// Named layers in insertion order.
    #[ts(as = "Vec<(String, AnnotationLayer)>")]
    pub layers: IndexMap<String, AnnotationLayer>,
}

impl SongAnnotations {
    /// Create an empty container with no layers.
    pub fn empty() -> Self {
        Self {
            version: 1,
            active_layer: None,
            layers: IndexMap::new(),
        }
    }

    /// Get the active layer's data, falling back to the first layer.
    pub fn active_data(&self) -> Option<&AnalysisDocument> {
        let layer = self
            .active_layer
            .as_ref()
            .and_then(|name| self.layers.get(name))
            .or_else(|| self.layers.values().next());
        layer.map(|l| &l.data)
    }

    /// Generate a unique layer name by appending a suffix if needed.
    ///
    /// Given base `"AI-analysis"`, returns `"AI-analysis"` if unused,
    /// then `"AI-analysis-2"`, `"AI-analysis-3"`, etc.
    pub fn unique_name(&self, base: &str) -> String {
        if !self.layers.contains_key(base) {
            return base.to_string();
        }
        let mut n = 2u32;
        loop {
            let candidate = format!("{base}-{n}");
            if !self.layers.contains_key(&candidate) {
                return candidate;
            }
            n = n.saturating_add(1);
        }
    }
}
