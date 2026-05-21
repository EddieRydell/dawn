use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use ts_rs::TS;

use super::automation::{AutomationClip, ClipId};
use super::blend_mode::BlendMode;
use super::effect_kind::EffectKind;
use super::fixture::{FixtureId, GroupId};
use super::motion_path::MotionPath;
use super::params::{EffectParams, ParamKey};
use super::time_range::TimeRange;

// ── EffectId ────────────────────────────────────────────────────────

/// Unique identifier for an effect instance.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, TS, JsonSchema)]
#[serde(transparent)]
#[ts(export)]
pub struct EffectId(pub String);

impl EffectId {
    /// Generate a new random effect ID.
    pub fn generate() -> Self {
        Self(uuid::Uuid::new_v4().to_string())
    }

    /// Borrow the inner string as a `&str`.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

// ── NodeId ─────────────────────────────────────────────────────────

/// Identifies a node in the element tree. Each fixture or group can have
/// its own timeline of effects. Effects on a group node cascade to all
/// descendant fixtures.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, TS, JsonSchema)]
#[ts(export)]
pub enum NodeId {
    Fixture(FixtureId),
    Group(GroupId),
}

impl std::fmt::Display for NodeId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            NodeId::Fixture(FixtureId(id)) => write!(f, "f:{id}"),
            NodeId::Group(GroupId(id)) => write!(f, "g:{id}"),
        }
    }
}

impl std::str::FromStr for NodeId {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if let Some(rest) = s.strip_prefix("f:") {
            let id: u32 = rest
                .parse()
                .map_err(|e| format!("invalid fixture id: {e}"))?;
            Ok(NodeId::Fixture(FixtureId(id)))
        } else if let Some(rest) = s.strip_prefix("g:") {
            let id: u32 = rest.parse().map_err(|e| format!("invalid group id: {e}"))?;
            Ok(NodeId::Group(GroupId(id)))
        } else {
            Err(format!("invalid NodeId: {s}"))
        }
    }
}

/// Serde helper for `HashMap<NodeId, V>` — serializes keys as strings (`"f:1"`, `"g:2"`).
mod node_id_map {
    use super::{Deserialize, HashMap, NodeId, Serialize};
    use serde::de::{self, MapAccess, Visitor};
    use serde::ser::SerializeMap;

    pub fn serialize<V: Serialize, S: serde::Serializer>(
        map: &HashMap<NodeId, V>,
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        let mut ser_map = serializer.serialize_map(Some(map.len()))?;
        for (k, v) in map {
            ser_map.serialize_entry(&k.to_string(), v)?;
        }
        ser_map.end()
    }

    pub fn deserialize<'de, V: Deserialize<'de>, D: serde::Deserializer<'de>>(
        deserializer: D,
    ) -> Result<HashMap<NodeId, V>, D::Error> {
        struct NodeIdMapVisitor<V>(std::marker::PhantomData<V>);

        impl<'de, V: Deserialize<'de>> Visitor<'de> for NodeIdMapVisitor<V> {
            type Value = HashMap<NodeId, V>;

            fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str("a map with NodeId string keys (e.g. \"f:1\", \"g:2\")")
            }

            fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Self::Value, A::Error> {
                let mut result = HashMap::with_capacity(map.size_hint().unwrap_or(0));
                while let Some((key, value)) = map.next_entry::<String, V>()? {
                    let node_id: NodeId = key.parse().map_err(de::Error::custom)?;
                    result.insert(node_id, value);
                }
                Ok(result)
            }
        }

        deserializer.deserialize_map(NodeIdMapVisitor(std::marker::PhantomData))
    }
}

// ── EffectInstance ──────────────────────────────────────────────────

/// A placed effect on the timeline. Fully describes what happens, when, and to what.
#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
#[ts(export)]
pub struct EffectInstance {
    #[serde(default = "EffectId::generate")]
    pub id: EffectId,
    pub kind: EffectKind,
    pub params: EffectParams,
    pub time_range: TimeRange,
    pub blend_mode: BlendMode,
    /// Opacity of this effect (0.0 = transparent, 1.0 = fully opaque).
    /// Values outside [0.0, 1.0] are safe: `Color::scale()` clamps the factor,
    /// and the evaluator uses opacity only via `scale()`.
    pub opacity: f64,
    /// Links effect parameters to automation clips by ClipId.
    /// When a linked clip is active, its evaluated value overrides the bundled default.
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    #[ts(as = "HashMap<String, ClipId>")]
    pub param_links: HashMap<ParamKey, ClipId>,
}

/// A single item on a track: either an effect or an automation clip.
/// Items are stored sorted by start time for binary search.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(tag = "item_type")]
#[ts(export)]
pub enum TrackItem {
    Effect(EffectInstance),
    Clip(AutomationClip),
}

impl TrackItem {
    /// The unique ID of this item, regardless of type.
    pub fn id_str(&self) -> &str {
        match self {
            TrackItem::Effect(e) => e.id.as_str(),
            TrackItem::Clip(c) => c.id.as_str(),
        }
    }

    pub fn time_range(&self) -> TimeRange {
        match self {
            TrackItem::Effect(e) => e.time_range,
            TrackItem::Clip(c) => c.time_range,
        }
    }

    pub fn as_effect(&self) -> Option<&EffectInstance> {
        match self {
            TrackItem::Effect(e) => Some(e),
            TrackItem::Clip(_) => None,
        }
    }

    pub fn as_clip(&self) -> Option<&AutomationClip> {
        match self {
            TrackItem::Clip(c) => Some(c),
            TrackItem::Effect(_) => None,
        }
    }

    pub fn as_effect_mut(&mut self) -> Option<&mut EffectInstance> {
        match self {
            TrackItem::Effect(e) => Some(e),
            TrackItem::Clip(_) => None,
        }
    }

    pub fn as_clip_mut(&mut self) -> Option<&mut AutomationClip> {
        match self {
            TrackItem::Clip(c) => Some(c),
            TrackItem::Effect(_) => None,
        }
    }
}

/// A timeline of effects and automation clips attached to a single node
/// (fixture or group) in the element tree. Items are kept sorted by start
/// time for binary search optimization.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct NodeTimeline {
    pub items: Vec<TrackItem>,
}

impl Default for NodeTimeline {
    fn default() -> Self {
        Self::new()
    }
}

impl NodeTimeline {
    /// Create an empty timeline.
    pub fn new() -> Self {
        Self { items: Vec::new() }
    }

    /// Iterator over all effects in this timeline.
    pub fn effects(&self) -> impl Iterator<Item = &EffectInstance> {
        self.items.iter().filter_map(TrackItem::as_effect)
    }

    /// Iterator over all automation clips in this timeline.
    pub fn clips(&self) -> impl Iterator<Item = &AutomationClip> {
        self.items.iter().filter_map(TrackItem::as_clip)
    }

    /// Find a clip by ID.
    pub fn clip_by_id(&self, id: &ClipId) -> Option<&AutomationClip> {
        self.clips().find(|c| &c.id == id)
    }

    /// Find an effect by ID.
    pub fn effect_by_id(&self, id: &EffectId) -> Option<&EffectInstance> {
        self.effects().find(|e| &e.id == id)
    }

    /// Find an effect by ID (mutable).
    pub fn effect_by_id_mut(&mut self, id: &EffectId) -> Option<&mut EffectInstance> {
        self.items.iter_mut().find_map(|item| match item {
            TrackItem::Effect(e) if &e.id == id => Some(e),
            _ => None,
        })
    }

    /// Find a clip by ID (mutable).
    pub fn clip_by_id_mut(&mut self, id: &ClipId) -> Option<&mut AutomationClip> {
        self.items.iter_mut().find_map(|item| match item {
            TrackItem::Clip(c) if &c.id == id => Some(c),
            _ => None,
        })
    }

    /// Insert an item sorted by start time. Returns the insert index.
    pub fn add_item(&mut self, item: TrackItem) -> usize {
        let start = item.time_range().start();
        let pos = self
            .items
            .partition_point(|i| i.time_range().start() < start);
        self.items.insert(pos, item);
        pos
    }

    /// Re-sort items by start time.
    pub fn resort_items(&mut self) {
        self.items.sort_by(|a, b| {
            a.time_range()
                .start()
                .partial_cmp(&b.time_range().start())
                .unwrap_or(std::cmp::Ordering::Equal)
        });
    }
}

/// A sequence is the top-level timeline container. One sequence per song/show.
/// Deserialization runs `validated()` automatically via `#[serde(from = "SequenceRaw")]`.
#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct Sequence {
    pub name: String,
    /// Duration in seconds.
    pub duration: f64,
    /// Target frames per second for evaluation.
    pub frame_rate: f64,
    /// Audio file path, if any.
    pub audio_file: Option<String>,
    /// Timelines keyed by element node. Each fixture or group can have its own
    /// timeline of effects. Group-level effects cascade to descendant fixtures.
    #[serde(with = "node_id_map")]
    #[ts(as = "HashMap<String, NodeTimeline>")]
    pub node_timelines: HashMap<NodeId, NodeTimeline>,
    /// Named motion paths. Key = path name.
    pub motion_paths: HashMap<String, MotionPath>,
}

#[derive(Deserialize)]
struct SequenceRaw {
    name: String,
    duration: f64,
    frame_rate: f64,
    audio_file: Option<String>,
    #[serde(default, with = "node_id_map")]
    node_timelines: HashMap<NodeId, NodeTimeline>,
    #[serde(default)]
    motion_paths: HashMap<String, MotionPath>,
}

impl<'de> Deserialize<'de> for Sequence {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let raw = SequenceRaw::deserialize(deserializer)?;
        Sequence {
            name: raw.name,
            duration: raw.duration,
            frame_rate: raw.frame_rate,
            audio_file: raw.audio_file,
            node_timelines: raw.node_timelines,
            motion_paths: raw.motion_paths,
        }
        .validated()
        .map_err(serde::de::Error::custom)
    }
}

impl Sequence {
    /// Build a lookup map from ClipId → AutomationClip across all node timelines.
    ///
    /// Clips are global (cross-node links work), so this collects from every timeline.
    /// Used by the evaluator and thumbnail renderer to resolve automation links.
    pub fn clip_lookup(&self) -> HashMap<&ClipId, &AutomationClip> {
        self.node_timelines
            .values()
            .flat_map(NodeTimeline::clips)
            .map(|clip| (&clip.id, clip))
            .collect()
    }

    /// Get or create a mutable timeline for the given node.
    pub fn timeline_mut(&mut self, node_id: &NodeId) -> &mut NodeTimeline {
        self.node_timelines.entry(node_id.clone()).or_default()
    }

    /// Validates sequence parameters, returning an error for invalid values.
    /// Duration and frame rate must be positive and finite.
    pub fn validated(self) -> Result<Self, String> {
        if !self.duration.is_finite() || self.duration <= 0.0 {
            return Err(format!(
                "Invalid sequence duration: {} (must be positive and finite)",
                self.duration
            ));
        }
        if !self.frame_rate.is_finite() || self.frame_rate <= 0.0 {
            return Err(format!(
                "Invalid sequence frame_rate: {} (must be positive and finite)",
                self.frame_rate
            ));
        }
        Ok(self)
    }
}

#[cfg(test)]
#[allow(clippy::expect_used, clippy::float_cmp, clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn sequence_deser_negative_duration_is_error() {
        let json = r#"{"name":"test","duration":-5.0,"frame_rate":60.0,"audio_file":null,"node_timelines":{}}"#;
        let result = serde_json::from_str::<Sequence>(json);
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(
            err.contains("duration"),
            "error should mention duration: {err}"
        );
    }

    #[test]
    fn sequence_deser_negative_frame_rate_is_error() {
        let json = r#"{"name":"test","duration":10.0,"frame_rate":-1.0,"audio_file":null,"node_timelines":{}}"#;
        let result = serde_json::from_str::<Sequence>(json);
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(
            err.contains("frame_rate"),
            "error should mention frame_rate: {err}"
        );
    }

    #[test]
    fn sequence_validated_rejects_infinity() {
        let result = Sequence {
            name: "test".to_string(),
            duration: f64::INFINITY,
            frame_rate: f64::NEG_INFINITY,
            audio_file: None,
            node_timelines: HashMap::new(),
            motion_paths: HashMap::new(),
        }
        .validated();
        assert!(result.is_err());
    }

    #[test]
    fn sequence_validated_accepts_valid() {
        let result = Sequence {
            name: "test".to_string(),
            duration: 60.0,
            frame_rate: 30.0,
            audio_file: None,
            node_timelines: HashMap::new(),
            motion_paths: HashMap::new(),
        }
        .validated();
        assert!(result.is_ok());
        let seq = result.unwrap();
        assert_eq!(seq.duration, 60.0);
        assert_eq!(seq.frame_rate, 30.0);
    }
}
