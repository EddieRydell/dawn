use std::collections::{BTreeMap, HashMap, HashSet};
use std::error::Error;
use std::fmt;
use std::fs;
use std::path::{Component, Path, PathBuf};

use indexmap::IndexMap;
use serde::de::{self, Visitor};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

#[derive(Debug, Clone, Copy, Deserialize, Serialize)]
pub enum Authored {}

#[derive(Debug, Clone, Copy, Deserialize, Serialize)]
pub enum Resolved {}

pub type AuthoredProject = Project<Authored>;
pub type ResolvedProject = Project<Resolved>;
pub type DawnFile = IndexMap<String, DawnObject<Authored>>;

pub trait ModelMode {
    type ProjectDisplay: fmt::Debug + Clone + Serialize + for<'de> Deserialize<'de>;
    type ProjectSequence: fmt::Debug + Clone + Serialize + for<'de> Deserialize<'de>;
    type DisplayController: fmt::Debug + Clone + Serialize + for<'de> Deserialize<'de>;
    type DisplayPatch: fmt::Debug + Clone + Serialize + for<'de> Deserialize<'de>;
    type DisplayLayout: fmt::Debug + Clone + Serialize + for<'de> Deserialize<'de>;
    type LayoutFixture: fmt::Debug + Clone + Serialize + for<'de> Deserialize<'de>;
    type FixturePlacementFixture: fmt::Debug + Clone + Serialize + for<'de> Deserialize<'de>;
    type GroupMember: fmt::Debug + Clone + Serialize + for<'de> Deserialize<'de>;
    type RouteFixture: fmt::Debug + Clone + Serialize + for<'de> Deserialize<'de>;
    type RouteController: fmt::Debug + Clone + Serialize + for<'de> Deserialize<'de>;
    type SequenceAudio: fmt::Debug + Clone + Serialize + for<'de> Deserialize<'de>;
    type EffectTargetGroup: fmt::Debug + Clone + Serialize + for<'de> Deserialize<'de>;
    type EffectTargetFixture: fmt::Debug + Clone + Serialize + for<'de> Deserialize<'de>;
    type SequenceEffectScript: fmt::Debug + Clone + Serialize + for<'de> Deserialize<'de>;
    type AutomationClipCurve: fmt::Debug + Clone + Serialize + for<'de> Deserialize<'de>;
    type AutomationClipTarget: fmt::Debug + Clone + Serialize + for<'de> Deserialize<'de>;
}

impl ModelMode for Authored {
    type ProjectDisplay = InlineOrImport<Display<Authored>>;
    type ProjectSequence = InlineOrImport<Sequence<Authored>>;
    type DisplayController = InlineOrImport<Controller>;
    type DisplayPatch = InlineOrImport<Patch<Authored>>;
    type DisplayLayout = InlineOrImport<Layout<Authored>>;
    type LayoutFixture = FixturePlacement<Authored>;
    type FixturePlacementFixture = InlineOrImport<Fixture>;
    type GroupMember = FixtureRef;
    type RouteFixture = FixtureRef;
    type RouteController = ControllerRef;
    type SequenceAudio = Option<ImportRef>;
    type EffectTargetGroup = GroupRef;
    type EffectTargetFixture = FixtureRef;
    type SequenceEffectScript = InlineOrImport<String>;
    type AutomationClipCurve = InlineOrImport<Curve>;
    type AutomationClipTarget = SequenceEffectRef;
}

impl ModelMode for Resolved {
    type ProjectDisplay = Display<Resolved>;
    type ProjectSequence = Sequence<Resolved>;
    type DisplayController = Controller;
    type DisplayPatch = Patch<Resolved>;
    type DisplayLayout = Layout<Resolved>;
    type LayoutFixture = FixturePlacement<Resolved>;
    type FixturePlacementFixture = Fixture;
    type GroupMember = FixtureIndex;
    type RouteFixture = FixtureIndex;
    type RouteController = ControllerIndex;
    type SequenceAudio = Option<DawnPath>;
    type EffectTargetGroup = GroupIndex;
    type EffectTargetFixture = FixtureIndex;
    type SequenceEffectScript = ScriptSource;
    type AutomationClipCurve = InlineOrImport<Curve>;
    type AutomationClipTarget = SequenceEffectIndex;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Deserialize, Serialize)]
#[serde(transparent)]
pub struct FixtureIndex(pub usize);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Deserialize, Serialize)]
#[serde(transparent)]
pub struct ControllerIndex(pub usize);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Deserialize, Serialize)]
#[serde(transparent)]
pub struct GroupIndex(pub usize);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Deserialize, Serialize)]
#[serde(transparent)]
pub struct SequenceEffectIndex(pub usize);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Time {
    pub milliseconds: u64,
}

impl Serialize for Time {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&format!("{}ms", self.milliseconds))
    }
}

impl<'de> Deserialize<'de> for Time {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct TimeVisitor;
        impl Visitor<'_> for TimeVisitor {
            type Value = Time;

            fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str("a suffixed time like 1m10s, 12s500ms, or 120943ms")
            }

            fn visit_u64<E>(self, _: u64) -> Result<Self::Value, E>
            where
                E: de::Error,
            {
                Err(E::custom("time must use an `ms`, `s`, or `m` suffix"))
            }

            fn visit_str<E>(self, value: &str) -> Result<Self::Value, E>
            where
                E: de::Error,
            {
                parse_time_ms(value)
                    .map(|milliseconds| Time { milliseconds })
                    .map_err(E::custom)
            }
        }

        deserializer.deserialize_any(TimeVisitor)
    }
}

fn parse_time_ms(value: &str) -> Result<u64, &'static str> {
    if value.is_empty() {
        return Err("time must not be empty");
    }

    let mut rest = value;
    let mut total = 0u64;
    while !rest.is_empty() {
        let digits = rest
            .find(|character: char| !character.is_ascii_digit())
            .unwrap_or(rest.len());
        if digits == 0 {
            return Err("time segment must start with an integer");
        }

        let amount = rest[..digits]
            .parse::<u64>()
            .map_err(|_| "time amount must be an integer")?;
        rest = &rest[digits..];

        let (multiplier, suffix_len) = if rest.starts_with("ms") {
            (1, 2)
        } else if rest.starts_with('s') {
            (1_000, 1)
        } else if rest.starts_with('m') {
            (60_000, 1)
        } else {
            return Err("time segment must use `ms`, `s`, or `m`");
        };

        let segment = amount.checked_mul(multiplier).ok_or("time is too large")?;
        total = total.checked_add(segment).ok_or("time is too large")?;
        rest = &rest[suffix_len..];
    }

    Ok(total)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChannelRange {
    pub start: u16,
    pub end: u16,
}

impl Serialize for ChannelRange {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&format!("{}..{}", self.start, self.end))
    }
}

impl<'de> Deserialize<'de> for ChannelRange {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let raw = String::deserialize(deserializer)?;
        let Some((start, end)) = raw.split_once("..") else {
            return Err(de::Error::custom("channel range must look like `1..510`"));
        };
        let start = start
            .parse()
            .map_err(|_| de::Error::custom("range start must be an integer"))?;
        let end = end
            .parse()
            .map_err(|_| de::Error::custom("range end must be an integer"))?;
        if start > end {
            return Err(de::Error::custom("range start must be <= range end"));
        }
        Ok(Self { start, end })
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
#[serde(bound(
    serialize = "Project<M>: Serialize, Display<M>: Serialize, Layout<M>: Serialize, Patch<M>: Serialize, Sequence<M>: Serialize",
    deserialize = "Project<M>: Deserialize<'de>, Display<M>: Deserialize<'de>, Layout<M>: Deserialize<'de>, Patch<M>: Deserialize<'de>, Sequence<M>: Deserialize<'de>"
))]
pub enum DawnObject<M: ModelMode = Authored> {
    Project(Project<M>),
    Display(Display<M>),
    Controller(Controller),
    Layout(Layout<M>),
    Fixture(Fixture),
    Patch(Patch<M>),
    Sequence(Sequence<M>),
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ProjectPath(PathBuf);

impl ProjectPath {
    pub fn new(path: impl AsRef<Path>) -> Self {
        let path = path.as_ref();
        let absolute = if path.is_absolute() {
            path.to_path_buf()
        } else {
            std::env::current_dir()
                .map(|current_dir| current_dir.join(path))
                .unwrap_or_else(|_| path.to_path_buf())
        };
        Self(lexically_normalize_path(&absolute))
    }

    pub fn as_path(&self) -> &Path {
        &self.0
    }

    pub fn parent(&self) -> Option<Self> {
        self.0.parent().map(Self::new)
    }

    pub fn join(&self, path: impl AsRef<Path>) -> Self {
        Self::new(self.0.join(path))
    }

    pub fn to_slash_string(&self) -> String {
        path_to_slash_string(&self.0)
    }

    pub fn display(&self) -> std::path::Display<'_> {
        self.0.display()
    }
}

impl AsRef<Path> for ProjectPath {
    fn as_ref(&self) -> &Path {
        self.as_path()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DawnPath(PathBuf);

impl DawnPath {
    pub fn new(path: impl AsRef<Path>) -> Self {
        Self(path.as_ref().to_path_buf())
    }

    pub fn as_path(&self) -> &Path {
        &self.0
    }

    pub fn to_slash_string(&self) -> String {
        path_to_slash_string(&self.0)
    }
}

impl Serialize for DawnPath {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.to_slash_string())
    }
}

impl<'de> Deserialize<'de> for DawnPath {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        String::deserialize(deserializer).map(Self::new)
    }
}

macro_rules! string_ref {
    ($name:ident) => {
        #[derive(Debug, Clone, PartialEq, Eq, Hash, Deserialize, Serialize)]
        #[serde(transparent)]
        pub struct $name(String);

        impl $name {
            pub fn new(value: impl Into<String>) -> Self {
                Self(value.into())
            }

            pub fn as_str(&self) -> &str {
                &self.0
            }
        }
    };
}

string_ref!(ObjectName);
string_ref!(FixtureRef);
string_ref!(GroupRef);
string_ref!(ControllerRef);
string_ref!(SequenceEffectRef);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportRef {
    raw: String,
    path: DawnPath,
    object: Option<ObjectName>,
}

impl ImportRef {
    pub fn new(raw: impl Into<String>) -> Result<Self, String> {
        serde_yaml::from_value(serde_yaml::Value::String(raw.into()))
            .map_err(|error| error.to_string())
    }

    pub fn raw(&self) -> &str {
        &self.raw
    }

    pub fn path(&self) -> &DawnPath {
        &self.path
    }

    pub fn object(&self) -> Option<&ObjectName> {
        self.object.as_ref()
    }
}

impl<'de> Deserialize<'de> for ImportRef {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let raw = String::deserialize(deserializer)?;
        let (path, object) = match raw.split_once("::") {
            Some((path, object)) => {
                if object.is_empty() {
                    return Err(de::Error::custom("import object name must not be empty"));
                }
                (path, Some(ObjectName(object.to_string())))
            }
            None => (raw.as_str(), None),
        };
        if path.is_empty() {
            return Err(de::Error::custom("import path must not be empty"));
        }
        Ok(Self {
            raw: raw.clone(),
            path: DawnPath::new(path),
            object,
        })
    }
}

impl Serialize for ImportRef {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.raw)
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(untagged)]
pub enum InlineOrImport<T> {
    Inline(T),
    Import { import: ImportRef },
}

impl<T> InlineOrImport<T> {
    pub fn import_ref(&self) -> Option<&ImportRef> {
        match self {
            Self::Inline(_) => None,
            Self::Import { import } => Some(import),
        }
    }

    pub fn import_path(&self) -> Option<String> {
        self.import_ref()
            .map(|import| import.path().to_slash_string())
    }

    pub fn inline(&self) -> Option<&T> {
        match self {
            Self::Inline(value) => Some(value),
            Self::Import { .. } => None,
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
#[serde(bound(
    serialize = "M::ProjectDisplay: Serialize, M::ProjectSequence: Serialize",
    deserialize = "M::ProjectDisplay: Deserialize<'de>, M::ProjectSequence: Deserialize<'de>"
))]
pub struct Project<M: ModelMode = Authored> {
    pub name: String,
    pub display: M::ProjectDisplay,
    #[serde(default)]
    pub sequences: Vec<M::ProjectSequence>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
#[serde(bound(
    serialize = "M::DisplayController: Serialize, M::DisplayPatch: Serialize, M::DisplayLayout: Serialize",
    deserialize = "M::DisplayController: Deserialize<'de>, M::DisplayPatch: Deserialize<'de>, M::DisplayLayout: Deserialize<'de>"
))]
pub struct Display<M: ModelMode = Authored> {
    pub name: String,
    #[serde(default)]
    pub controllers: Vec<M::DisplayController>,
    pub patch: M::DisplayPatch,
    pub layout: M::DisplayLayout,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Controller {
    pub name: String,
    pub protocol: Protocol,
    #[serde(default)]
    pub universes: Vec<Universe>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Protocol {
    Artnet,
    Sacn,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Universe {
    pub id: u32,
    pub range: ChannelRange,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
#[serde(bound(
    serialize = "M::LayoutFixture: Serialize, Group<M>: Serialize",
    deserialize = "M::LayoutFixture: Deserialize<'de>, Group<M>: Deserialize<'de>"
))]
pub struct Layout<M: ModelMode = Authored> {
    pub name: String,
    pub units: DistanceUnit,
    #[serde(default)]
    pub fixtures: Vec<M::LayoutFixture>,
    #[serde(default)]
    pub groups: Vec<Group<M>>,
}

impl Layout<Resolved> {
    pub fn fixture(&self, index: FixtureIndex) -> Option<&FixturePlacement<Resolved>> {
        self.fixtures.get(index.0)
    }

    pub fn group_members(&self, index: GroupIndex) -> Option<&[FixtureIndex]> {
        self.groups
            .get(index.0)
            .map(|group| group.members.as_slice())
    }
}

impl Display<Resolved> {
    pub fn controller(&self, index: ControllerIndex) -> Option<&Controller> {
        self.controllers.get(index.0)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[cfg_attr(feature = "bindings", derive(specta::Type))]
#[serde(rename_all = "snake_case")]
pub enum DistanceUnit {
    Meters,
    Feet,
}

impl Default for DistanceUnit {
    fn default() -> Self {
        Self::Meters
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
#[serde(bound(
    serialize = "M::FixturePlacementFixture: Serialize",
    deserialize = "M::FixturePlacementFixture: Deserialize<'de>"
))]
pub struct FixturePlacement<M: ModelMode = Authored> {
    pub id: String,
    pub fixture: M::FixturePlacementFixture,
    pub transform: Transform,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Fixture {
    pub name: String,
    pub color_model: ColorModel,
    #[serde(default = "default_bulb_size")]
    pub bulb_size: f64,
    pub geometry: Geometry,
}

fn default_bulb_size() -> f64 {
    1.0
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[cfg_attr(feature = "bindings", derive(specta::Type))]
#[serde(rename_all = "snake_case")]
pub enum ColorModel {
    Rgb,
    Rgba,
    Rgbw,
    Rgbaw,
    White,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "bindings", derive(specta::Type))]
#[serde(deny_unknown_fields)]
pub struct Transform {
    pub position: Point3,
    #[serde(default)]
    pub rotation: Rotation3,
    #[serde(default)]
    pub scale: Scale3,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "bindings", derive(specta::Type))]
#[serde(deny_unknown_fields)]
pub struct Point3 {
    pub x: f64,
    pub y: f64,
    pub z: f64,
}

impl Default for Point3 {
    fn default() -> Self {
        Self {
            x: 0.0,
            y: 0.0,
            z: 0.0,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "bindings", derive(specta::Type))]
#[serde(deny_unknown_fields)]
pub struct Rotation3 {
    pub x: f64,
    pub y: f64,
    pub z: f64,
}

impl Default for Rotation3 {
    fn default() -> Self {
        Self {
            x: 0.0,
            y: 0.0,
            z: 0.0,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "bindings", derive(specta::Type))]
#[serde(deny_unknown_fields)]
pub struct Scale3 {
    pub x: f64,
    pub y: f64,
    pub z: f64,
}

impl Default for Scale3 {
    fn default() -> Self {
        Self {
            x: 1.0,
            y: 1.0,
            z: 1.0,
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[cfg_attr(feature = "bindings", derive(specta::Type))]
#[serde(
    tag = "type",
    rename_all = "snake_case",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum Geometry {
    Points {
        points: Vec<Point3>,
    },
    Lines {
        points: Vec<Point3>,
        pixels: u32,
    },
    Arc {
        center: Point3,
        radius: f64,
        start_degrees: f64,
        end_degrees: f64,
        pixels: u32,
    },
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
#[serde(bound(
    serialize = "M::GroupMember: Serialize",
    deserialize = "M::GroupMember: Deserialize<'de>"
))]
pub struct Group<M: ModelMode = Authored> {
    pub name: String,
    pub members: Vec<M::GroupMember>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
#[serde(bound(
    serialize = "Route<M>: Serialize",
    deserialize = "Route<M>: Deserialize<'de>"
))]
pub struct Patch<M: ModelMode = Authored> {
    #[serde(default)]
    pub routes: Vec<Route<M>>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
#[serde(bound(
    serialize = "M::RouteFixture: Serialize, M::RouteController: Serialize",
    deserialize = "M::RouteFixture: Deserialize<'de>, M::RouteController: Deserialize<'de>"
))]
pub struct Route<M: ModelMode = Authored> {
    pub fixture: M::RouteFixture,
    pub controller: M::RouteController,
    pub universe: u32,
    pub start: u32,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
#[serde(bound(
    serialize = "M::SequenceAudio: Serialize, SequenceEffect<M>: Serialize, AutomationClip<M>: Serialize",
    deserialize = "M::SequenceAudio: Deserialize<'de>, SequenceEffect<M>: Deserialize<'de>, AutomationClip<M>: Deserialize<'de>"
))]
pub struct Sequence<M: ModelMode = Authored> {
    pub duration: Time,
    pub frame_rate: u32,
    pub audio: M::SequenceAudio,
    #[serde(default)]
    pub effects: Vec<SequenceEffect<M>>,
    #[serde(default)]
    pub automation_clips: Vec<AutomationClip<M>>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(tag = "type", content = "name", rename_all = "snake_case")]
#[serde(bound(
    serialize = "M::EffectTargetGroup: Serialize, M::EffectTargetFixture: Serialize",
    deserialize = "M::EffectTargetGroup: Deserialize<'de>, M::EffectTargetFixture: Deserialize<'de>"
))]
pub enum EffectTarget<M: ModelMode = Authored> {
    Group(M::EffectTargetGroup),
    Fixture(M::EffectTargetFixture),
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
#[serde(bound(
    serialize = "EffectTarget<M>: Serialize, M::SequenceEffectScript: Serialize",
    deserialize = "EffectTarget<M>: Deserialize<'de>, M::SequenceEffectScript: Deserialize<'de>"
))]
pub struct SequenceEffect<M: ModelMode = Authored> {
    pub id: String,
    pub start: Time,
    pub duration: Time,
    pub target: EffectTarget<M>,
    #[serde(default)]
    pub params: IndexMap<String, EffectParam>,
    pub script: M::SequenceEffectScript,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(tag = "type", content = "value", rename_all = "snake_case")]
pub enum ScriptSource {
    Inline(String),
    External(DawnPath),
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(transparent)]
pub struct Flags {
    pub values: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(transparent)]
pub struct Color {
    pub value: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum Curve {
    Named { name: String },
    Points { points: Vec<CurvePoint> },
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CurvePoint {
    pub time: f64,
    pub value: f64,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum EffectParam {
    Integer { value: u64 },
    Float { value: f64 },
    Flags { value: Flags },
    Color { value: Color },
    Curve { curve: Curve },
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
#[serde(bound(
    serialize = "M::AutomationClipCurve: Serialize, M::AutomationClipTarget: Serialize",
    deserialize = "M::AutomationClipCurve: Deserialize<'de>, M::AutomationClipTarget: Deserialize<'de>"
))]
pub struct AutomationClip<M: ModelMode = Authored> {
    pub id: String,
    pub start: Time,
    pub duration: Time,
    pub curve: M::AutomationClipCurve,
    #[serde(default)]
    pub targets: Vec<M::AutomationClipTarget>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "bindings", derive(specta::Type))]
#[serde(rename_all = "snake_case")]
pub enum ObjectKind {
    Project,
    Display,
    Controller,
    Layout,
    Fixture,
    Patch,
    Sequence,
}

impl fmt::Display for ObjectKind {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Project => "project",
            Self::Display => "display",
            Self::Controller => "controller",
            Self::Layout => "layout",
            Self::Fixture => "fixture",
            Self::Patch => "patch",
            Self::Sequence => "sequence",
        })
    }
}

#[derive(Debug, Clone)]
pub struct ResolvedImport {
    pub source_path: ProjectPath,
    pub object: DawnObject<Authored>,
}

#[derive(Debug, Clone)]
pub enum LowerError {
    MissingProject {
        key: String,
    },
    WrongObjectKind {
        key: String,
        expected: ObjectKind,
        actual: ObjectKind,
    },
    WrongImportedObjectKind {
        import: String,
        expected: ObjectKind,
        actual: ObjectKind,
    },
    Import {
        import: String,
        message: String,
    },
    DuplicateFixtureId {
        id: String,
    },
    UnknownFixture {
        id: String,
    },
    DuplicateControllerName {
        name: String,
    },
    UnknownController {
        name: String,
    },
    DuplicateGroupName {
        name: String,
    },
    UnknownGroup {
        name: String,
    },
    DuplicateSequenceEffectId {
        id: String,
    },
    UnknownSequenceEffect {
        id: String,
    },
}

impl fmt::Display for LowerError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingProject { key } => {
                write!(formatter, "project object `{key}` was not found")
            }
            Self::WrongObjectKind {
                key,
                expected,
                actual,
            } => write!(
                formatter,
                "object `{key}` must be a {expected}, but found a {actual}"
            ),
            Self::WrongImportedObjectKind {
                import,
                expected,
                actual,
            } => write!(
                formatter,
                "import `{import}` must resolve to a {expected}, but found a {actual}"
            ),
            Self::Import { import, message } => {
                write!(formatter, "failed to resolve import `{import}`: {message}")
            }
            Self::DuplicateFixtureId { id } => write!(formatter, "duplicate fixture id `{id}`"),
            Self::UnknownFixture { id } => write!(formatter, "unknown fixture `{id}`"),
            Self::DuplicateControllerName { name } => {
                write!(formatter, "duplicate controller `{name}`")
            }
            Self::UnknownController { name } => write!(formatter, "unknown controller `{name}`"),
            Self::DuplicateGroupName { name } => write!(formatter, "duplicate group `{name}`"),
            Self::UnknownGroup { name } => write!(formatter, "unknown group `{name}`"),
            Self::DuplicateSequenceEffectId { id } => {
                write!(formatter, "duplicate sequence effect `{id}`")
            }
            Self::UnknownSequenceEffect { id } => {
                write!(formatter, "unknown sequence effect `{id}`")
            }
        }
    }
}

impl Error for LowerError {}

#[derive(Debug)]
pub enum LoadProjectError {
    Io {
        path: ProjectPath,
        source: std::io::Error,
    },
    Yaml {
        path: ProjectPath,
        source: serde_yaml::Error,
    },
    Lower(LowerError),
}

impl fmt::Display for LoadProjectError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io { path, source } => {
                write!(formatter, "failed to read `{}`: {source}", path.display())
            }
            Self::Yaml { path, source } => {
                write!(formatter, "failed to parse `{}`: {source}", path.display())
            }
            Self::Lower(error) => write!(formatter, "{error}"),
        }
    }
}

impl Error for LoadProjectError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Io { source, .. } => Some(source),
            Self::Yaml { source, .. } => Some(source),
            Self::Lower(source) => Some(source),
        }
    }
}

impl From<LowerError> for LoadProjectError {
    fn from(error: LowerError) -> Self {
        Self::Lower(error)
    }
}

#[derive(Debug, Clone)]
pub struct ProjectAnalysis {
    pub root_path: ProjectPath,
    pub project_key: String,
    pub files: IndexMap<ProjectPath, AnalyzedFile>,
    pub diagnostics: Vec<ProjectDiagnostic>,
    pub resolved: Option<ResolvedProject>,
}

impl ProjectAnalysis {
    pub fn has_errors(&self) -> bool {
        self.diagnostics
            .iter()
            .any(|diagnostic| diagnostic.severity == DiagnosticSeverity::Error)
    }
}

#[derive(Debug, Clone)]
pub struct AnalyzedFile {
    pub path: ProjectPath,
    pub text: Option<String>,
    pub file: Option<DawnFile>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectDiagnostic {
    pub path: ProjectPath,
    pub range: Option<TextRange>,
    pub severity: DiagnosticSeverity,
    pub code: DiagnosticCode,
    pub message: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "bindings", derive(specta::Type))]
pub enum DiagnosticSeverity {
    Error,
    Warning,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "bindings", derive(specta::Type))]
#[serde(rename_all = "snake_case")]
pub enum DiagnosticCode {
    Io,
    Yaml,
    Import,
    Lower,
    ProjectKey,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TextRange {
    pub start: TextPosition,
    pub end: TextPosition,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TextPosition {
    pub line: u32,
    pub character: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectOverlay {
    pub path: ProjectPath,
    pub content: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[cfg_attr(feature = "bindings", derive(specta::Type))]
#[serde(rename_all = "snake_case")]
pub enum DocumentViewId {
    Text,
    Layout,
    Fixture,
}

#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "bindings", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct DocumentDescriptor {
    pub path: String,
    pub objects: Vec<DocumentObjectDescriptor>,
    pub available_views: Vec<DocumentViewId>,
    pub default_object_keys: BTreeMap<DocumentViewId, String>,
}

#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "bindings", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct DocumentObjectDescriptor {
    pub key: String,
    pub kind: ObjectKind,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "bindings", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct LayoutDocument {
    pub path: String,
    pub object_key: String,
    pub name: String,
    pub units: DistanceUnit,
    pub fixtures: Vec<LayoutFixturePlacement>,
    pub groups: Vec<LayoutGroupDocument>,
    pub fixture_catalog: Vec<FixtureCatalogItem>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "bindings", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct LayoutFixturePlacement {
    pub id: String,
    pub fixture: LayoutFixtureRef,
    pub resolved_fixture: ResolvedLayoutFixture,
    pub transform: Transform,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "bindings", derive(specta::Type))]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum LayoutFixtureRef {
    Import {
        import: String,
        object_key: Option<String>,
        source_path: Option<String>,
    },
    Inline {
        name: String,
        color_model: ColorModel,
        bulb_size: f64,
        geometry: Geometry,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "bindings", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct LayoutGroupDocument {
    pub name: String,
    pub members: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "bindings", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct ResolvedLayoutFixture {
    pub name: String,
    pub color_model: ColorModel,
    pub bulb_size: f64,
    pub geometry: Geometry,
    pub geometry_summary: String,
    pub source_path: String,
    pub object_key: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "bindings", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct FixtureCatalogItem {
    pub object_key: String,
    pub source_path: String,
    pub import_string: String,
    pub display_name: String,
    pub color_model: ColorModel,
    pub bulb_size: f64,
    pub geometry: Geometry,
    pub geometry_summary: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "bindings", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct FixtureDocument {
    pub path: String,
    pub selected_object_key: Option<String>,
    pub fixtures: Vec<FixtureDefinitionDocument>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "bindings", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct FixtureDefinitionDocument {
    pub object_key: String,
    pub name: String,
    pub color_model: ColorModel,
    pub bulb_size: f64,
    pub geometry: Geometry,
    pub geometry_summary: String,
}

#[derive(Debug, Clone)]
pub struct DocumentEditOutcome<T> {
    pub serialized_content: String,
    pub analysis: ProjectAnalysis,
    pub refreshed_document: T,
}

#[derive(Debug, Clone)]
pub struct BlockedDocumentEdit {
    pub diagnostics: Vec<ProjectDiagnostic>,
    pub message: String,
}

#[derive(Debug, Clone)]
pub enum DocumentEditResult<T> {
    Applied(DocumentEditOutcome<T>),
    Blocked(BlockedDocumentEdit),
}

pub fn inspect_document(
    path: impl AsRef<Path>,
    overlays: Vec<ProjectOverlay>,
) -> Result<DocumentDescriptor, String> {
    let path = ProjectPath::new(path.as_ref());
    let text = read_text_with_overlays(&path, &overlays)?;
    let file: DawnFile = serde_yaml::from_str(&text).map_err(|error| error.to_string())?;
    let objects = file
        .iter()
        .map(|(key, object)| DocumentObjectDescriptor {
            key: key.clone(),
            kind: object.kind(),
        })
        .collect::<Vec<_>>();
    let mut available_views = vec![DocumentViewId::Text];
    let mut default_object_keys = BTreeMap::new();
    if let Some(key) = file.iter().find_map(|(key, object)| match object {
        DawnObject::Layout(_) => Some(key.clone()),
        _ => None,
    }) {
        available_views.push(DocumentViewId::Layout);
        default_object_keys.insert(DocumentViewId::Layout, key);
    }
    if let Some(key) = file.iter().find_map(|(key, object)| match object {
        DawnObject::Fixture(_) => Some(key.clone()),
        _ => None,
    }) {
        available_views.push(DocumentViewId::Fixture);
        default_object_keys.insert(DocumentViewId::Fixture, key);
    }

    Ok(DocumentDescriptor {
        path: path.to_slash_string(),
        objects,
        available_views,
        default_object_keys,
    })
}

pub fn get_fixture_document(
    path: impl AsRef<Path>,
    selected_object_key: Option<&str>,
    overlays: Vec<ProjectOverlay>,
) -> Result<FixtureDocument, String> {
    let path = ProjectPath::new(path.as_ref());
    let text = read_text_with_overlays(&path, &overlays)?;
    let file: DawnFile = serde_yaml::from_str(&text).map_err(|error| error.to_string())?;
    let fixtures = file
        .iter()
        .filter_map(|(key, object)| {
            let DawnObject::Fixture(fixture) = object else {
                return None;
            };
            Some(fixture_to_document(key, fixture))
        })
        .collect::<Vec<_>>();
    let selected_object_key = selected_object_key
        .filter(|key| fixtures.iter().any(|fixture| fixture.object_key == *key))
        .map(str::to_string)
        .or_else(|| fixtures.first().map(|fixture| fixture.object_key.clone()));

    Ok(FixtureDocument {
        path: path.to_slash_string(),
        selected_object_key,
        fixtures,
    })
}

pub fn get_layout_document(
    path: impl AsRef<Path>,
    object_key: &str,
    project_path: impl AsRef<Path>,
    overlays: Vec<ProjectOverlay>,
) -> Result<LayoutDocument, String> {
    let path = ProjectPath::new(path.as_ref());
    let analysis = analyze_project_with_overlays(project_path, None, overlays.clone());
    if let Some(diagnostic) = analysis.diagnostics.iter().find(|diagnostic| {
        diagnostic.severity == DiagnosticSeverity::Error
            && diagnostic.path == path
            && matches!(diagnostic.code, DiagnosticCode::Io | DiagnosticCode::Yaml)
    }) {
        return Err(format!(
            "could not load layout `{object_key}`: {}",
            diagnostic.message
        ));
    }

    let text = read_text_with_overlays(&path, &overlays)?;
    let file: DawnFile = serde_yaml::from_str(&text).map_err(|error| error.to_string())?;
    let object = file
        .get(object_key)
        .ok_or_else(|| format!("layout object `{object_key}` was not found"))?;
    let DawnObject::Layout(layout) = object else {
        return Err(format!("object `{object_key}` is not a layout"));
    };
    let catalog = fixture_catalog_from_analysis(&analysis, &path);
    let mut resolver = AnalysisImportResolver {
        files: &analysis.files,
    };
    let resolved_layout = lower_layout(layout, &path, &mut |source_path, import, expected| {
        resolver.resolve(source_path, import, expected)
    })
    .map_err(|error| format!("could not load layout `{object_key}`: {error}"))?;

    layout_to_document(&path, object_key, layout, &resolved_layout, &catalog)
}

pub fn apply_layout_document_edit(
    path: impl AsRef<Path>,
    object_key: &str,
    document: LayoutDocument,
    base_content: String,
    overlays: Vec<ProjectOverlay>,
    project_path: impl AsRef<Path>,
    allow_breaking_references: bool,
) -> Result<DocumentEditResult<LayoutDocument>, String> {
    let path = ProjectPath::new(path.as_ref());
    let project_path = ProjectPath::new(project_path.as_ref());
    let file: DawnFile = serde_yaml::from_str(&base_content).map_err(|error| error.to_string())?;
    let Some(DawnObject::Layout(current_layout)) = file.get(object_key) else {
        return Err(format!("layout object `{object_key}` was not found"));
    };
    let mut layout = document_to_layout(document)?;
    repair_layout_group_members(current_layout, &mut layout);
    validate_layout_identifiers(&layout)?;
    let object = DawnObject::Layout(layout);
    let serialized = replace_top_level_object(&base_content, object_key, &object)?;
    let next_overlays = overlay_after_save(path.clone(), serialized.clone(), overlays.clone());
    let analysis = analyze_project_with_overlays(project_path.as_path(), None, next_overlays);
    let introduced_errors = introduced_error_diagnostics(
        &analyze_project_with_overlays(
            project_path.as_path(),
            None,
            overlay_after_save(path.clone(), base_content, overlays),
        ),
        &analysis,
    );
    if !allow_breaking_references && !introduced_errors.is_empty() {
        return Ok(DocumentEditResult::Blocked(BlockedDocumentEdit {
            diagnostics: introduced_errors,
            message: "This edit introduces project reference errors.".to_string(),
        }));
    }

    let refreshed_document = get_layout_document(
        path.as_path(),
        object_key,
        analysis.root_path.as_path(),
        vec![ProjectOverlay {
            path: path.clone(),
            content: serialized.clone(),
        }],
    )?;
    Ok(DocumentEditResult::Applied(DocumentEditOutcome {
        serialized_content: serialized,
        analysis,
        refreshed_document,
    }))
}

pub fn apply_fixture_document_edit(
    path: impl AsRef<Path>,
    document: FixtureDocument,
    base_content: String,
    overlays: Vec<ProjectOverlay>,
    project_path: impl AsRef<Path>,
    allow_breaking_references: bool,
) -> Result<DocumentEditResult<FixtureDocument>, String> {
    let path = ProjectPath::new(path.as_ref());
    validate_fixture_document(&document)?;
    let file: DawnFile = serde_yaml::from_str(&base_content).map_err(|error| error.to_string())?;
    let mut replacements = BTreeMap::new();
    for (key, object) in &file {
        if matches!(object, DawnObject::Fixture(_)) {
            replacements.insert(key.clone(), None);
        }
    }
    for fixture in &document.fixtures {
        replacements.insert(
            fixture.object_key.clone(),
            Some(serialize_top_level_object(
                &fixture.object_key,
                &DawnObject::Fixture(document_to_fixture(fixture)),
            )?),
        );
    }
    let serialized = replace_top_level_objects(&base_content, replacements)?;
    let project_path = ProjectPath::new(project_path.as_ref());
    let next_overlays = overlay_after_save(path.clone(), serialized.clone(), overlays.clone());
    let analysis = analyze_project_with_overlays(project_path.as_path(), None, next_overlays);
    let introduced_errors = introduced_error_diagnostics(
        &analyze_project_with_overlays(
            project_path.as_path(),
            None,
            overlay_after_save(path.clone(), base_content, overlays),
        ),
        &analysis,
    );
    if !allow_breaking_references && !introduced_errors.is_empty() {
        return Ok(DocumentEditResult::Blocked(BlockedDocumentEdit {
            diagnostics: introduced_errors,
            message: "This edit introduces project reference errors.".to_string(),
        }));
    }

    let refreshed_document = get_fixture_document(
        path.as_path(),
        document.selected_object_key.as_deref(),
        vec![ProjectOverlay {
            path: path.clone(),
            content: serialized.clone(),
        }],
    )?;
    Ok(DocumentEditResult::Applied(DocumentEditOutcome {
        serialized_content: serialized,
        analysis,
        refreshed_document,
    }))
}

fn read_text_with_overlays(
    path: &ProjectPath,
    overlays: &[ProjectOverlay],
) -> Result<String, String> {
    overlays
        .iter()
        .find(|overlay| overlay.path == *path)
        .map(|overlay| overlay.content.clone())
        .map(Ok)
        .unwrap_or_else(|| fs::read_to_string(path.as_path()).map_err(|error| error.to_string()))
}

fn layout_to_document(
    path: &ProjectPath,
    object_key: &str,
    layout: &Layout<Authored>,
    resolved_layout: &Layout<Resolved>,
    catalog: &[FixtureCatalogItem],
) -> Result<LayoutDocument, String> {
    if layout.fixtures.len() != resolved_layout.fixtures.len() {
        return Err("resolved layout fixture count did not match authored layout".to_string());
    }

    Ok(LayoutDocument {
        path: path.to_slash_string(),
        object_key: object_key.to_string(),
        name: layout.name.clone(),
        units: layout.units,
        fixtures: layout
            .fixtures
            .iter()
            .zip(&resolved_layout.fixtures)
            .map(|(fixture, resolved)| placement_to_document(fixture, resolved, path))
            .collect(),
        groups: layout
            .groups
            .iter()
            .map(|group| LayoutGroupDocument {
                name: group.name.clone(),
                members: group
                    .members
                    .iter()
                    .map(|member| member.as_str().to_string())
                    .collect(),
            })
            .collect(),
        fixture_catalog: catalog.to_vec(),
    })
}

fn placement_to_document(
    placement: &FixturePlacement<Authored>,
    resolved: &FixturePlacement<Resolved>,
    source_path: &ProjectPath,
) -> LayoutFixturePlacement {
    let (fixture, resolved_source_path, resolved_object_key) = match &placement.fixture {
        InlineOrImport::Inline(fixture) => (
            LayoutFixtureRef::Inline {
                name: fixture.name.clone(),
                color_model: fixture.color_model,
                bulb_size: fixture.bulb_size,
                geometry: fixture.geometry.clone(),
            },
            source_path.to_slash_string(),
            None,
        ),
        InlineOrImport::Import { import } => {
            let resolved_path =
                resolve_import_file_path(source_path, import.path()).to_slash_string();
            (
                LayoutFixtureRef::Import {
                    import: import.raw().to_string(),
                    object_key: import.object().map(|object| object.as_str().to_string()),
                    source_path: Some(resolved_path.clone()),
                },
                resolved_path,
                import.object().map(|object| object.as_str().to_string()),
            )
        }
    };

    LayoutFixturePlacement {
        id: placement.id.clone(),
        fixture,
        resolved_fixture: ResolvedLayoutFixture {
            name: resolved.fixture.name.clone(),
            color_model: resolved.fixture.color_model,
            bulb_size: resolved.fixture.bulb_size,
            geometry: resolved.fixture.geometry.clone(),
            geometry_summary: geometry_summary(&resolved.fixture.geometry),
            source_path: resolved_source_path,
            object_key: resolved_object_key,
        },
        transform: placement.transform,
    }
}

fn fixture_catalog_from_analysis(
    analysis: &ProjectAnalysis,
    importing_source_path: &ProjectPath,
) -> Vec<FixtureCatalogItem> {
    let mut catalog = Vec::new();
    for (path, analyzed) in &analysis.files {
        let Some(file) = analyzed.file.as_ref() else {
            continue;
        };
        for (key, object) in file {
            let DawnObject::Fixture(fixture) = object else {
                continue;
            };
            let source_path = path.to_slash_string();
            let import_path = relative_import_path(importing_source_path, path);
            catalog.push(FixtureCatalogItem {
                object_key: key.clone(),
                source_path: source_path.clone(),
                import_string: format!("{import_path}::{key}"),
                display_name: fixture.name.clone(),
                color_model: fixture.color_model,
                bulb_size: fixture.bulb_size,
                geometry: fixture.geometry.clone(),
                geometry_summary: geometry_summary(&fixture.geometry),
            });
        }
    }
    catalog.sort_by(|left, right| {
        left.display_name
            .cmp(&right.display_name)
            .then_with(|| left.source_path.cmp(&right.source_path))
            .then_with(|| left.object_key.cmp(&right.object_key))
    });
    catalog
}

fn geometry_summary(geometry: &Geometry) -> String {
    match geometry {
        Geometry::Points { points } => format!("{} point{}", points.len(), plural(points.len())),
        Geometry::Lines { pixels, .. } => format!("lines, {pixels} pixels"),
        Geometry::Arc { pixels, .. } => format!("arc, {pixels} pixels"),
    }
}

fn plural(count: usize) -> &'static str {
    if count == 1 {
        ""
    } else {
        "s"
    }
}

fn document_to_layout(document: LayoutDocument) -> Result<Layout<Authored>, String> {
    Ok(Layout {
        name: document.name,
        units: document.units,
        fixtures: document
            .fixtures
            .into_iter()
            .map(document_to_placement)
            .collect::<Result<Vec<_>, _>>()?,
        groups: document
            .groups
            .into_iter()
            .map(|group| Group {
                name: group.name,
                members: group.members.into_iter().map(FixtureRef::new).collect(),
            })
            .collect(),
    })
}

fn document_to_placement(
    placement: LayoutFixturePlacement,
) -> Result<FixturePlacement<Authored>, String> {
    let fixture = match placement.fixture {
        LayoutFixtureRef::Import { import, .. } => InlineOrImport::Import {
            import: ImportRef::new(import)?,
        },
        LayoutFixtureRef::Inline {
            name,
            color_model,
            bulb_size,
            geometry,
        } => InlineOrImport::Inline(Fixture {
            name,
            color_model,
            bulb_size,
            geometry,
        }),
    };
    Ok(FixturePlacement {
        id: placement.id,
        fixture,
        transform: placement.transform,
    })
}

fn fixture_to_document(object_key: &str, fixture: &Fixture) -> FixtureDefinitionDocument {
    FixtureDefinitionDocument {
        object_key: object_key.to_string(),
        name: fixture.name.clone(),
        color_model: fixture.color_model,
        bulb_size: fixture.bulb_size,
        geometry: fixture.geometry.clone(),
        geometry_summary: geometry_summary(&fixture.geometry),
    }
}

fn document_to_fixture(document: &FixtureDefinitionDocument) -> Fixture {
    Fixture {
        name: document.name.clone(),
        color_model: document.color_model,
        bulb_size: document.bulb_size,
        geometry: document.geometry.clone(),
    }
}

fn validate_fixture_document(document: &FixtureDocument) -> Result<(), String> {
    let mut keys = HashSet::new();
    for fixture in &document.fixtures {
        validate_simple_identifier(&fixture.object_key, "fixture object key")?;
        if !keys.insert(fixture.object_key.as_str()) {
            return Err(format!(
                "duplicate fixture object key `{}`",
                fixture.object_key
            ));
        }
    }
    Ok(())
}

fn validate_layout_identifiers(layout: &Layout<Authored>) -> Result<(), String> {
    let mut ids = HashSet::new();
    for fixture in &layout.fixtures {
        validate_simple_identifier(&fixture.id, "fixture placement id")?;
        if !ids.insert(fixture.id.as_str()) {
            return Err(format!("duplicate fixture placement id `{}`", fixture.id));
        }
    }
    Ok(())
}

fn repair_layout_group_members(current: &Layout<Authored>, next: &mut Layout<Authored>) {
    let mut renamed_by_index = HashMap::new();
    for (current_fixture, next_fixture) in current.fixtures.iter().zip(&next.fixtures) {
        if current_fixture.id != next_fixture.id {
            renamed_by_index.insert(current_fixture.id.as_str(), next_fixture.id.as_str());
        }
    }
    let next_ids = next
        .fixtures
        .iter()
        .map(|fixture| fixture.id.as_str())
        .collect::<HashSet<_>>();

    for group in &mut next.groups {
        let mut seen = HashSet::new();
        group.members = group
            .members
            .iter()
            .filter_map(|member| {
                let current = member.as_str();
                let repaired = renamed_by_index.get(current).copied().unwrap_or(current);
                if next_ids.contains(repaired) && seen.insert(repaired.to_string()) {
                    Some(FixtureRef::new(repaired))
                } else {
                    None
                }
            })
            .collect();
    }
}

fn validate_simple_identifier(value: &str, label: &str) -> Result<(), String> {
    let mut chars = value.chars();
    let Some(first) = chars.next() else {
        return Err(format!("{label} cannot be empty"));
    };
    if !(first.is_ascii_alphabetic() || first == '_') {
        return Err(format!("{label} must start with a letter or underscore"));
    }
    if chars.any(|character| !(character.is_ascii_alphanumeric() || character == '_')) {
        return Err(format!(
            "{label} may only contain letters, numbers, and underscores"
        ));
    }
    Ok(())
}

fn replace_top_level_object(
    text: &str,
    object_key: &str,
    object: &DawnObject<Authored>,
) -> Result<String, String> {
    let mut replacements = BTreeMap::new();
    replacements.insert(
        object_key.to_string(),
        Some(serialize_top_level_object(object_key, object)?),
    );
    replace_top_level_objects(text, replacements)
}

fn serialize_top_level_object(
    object_key: &str,
    object: &DawnObject<Authored>,
) -> Result<String, String> {
    let mut file = DawnFile::new();
    file.insert(object_key.to_string(), object.clone());
    serde_yaml::to_string(&file).map_err(|error| error.to_string())
}

fn replace_top_level_objects(
    text: &str,
    mut replacements: BTreeMap<String, Option<String>>,
) -> Result<String, String> {
    let blocks = top_level_object_blocks(text);
    let mut output = String::with_capacity(text.len());
    let mut cursor = 0;
    for block in blocks {
        output.push_str(&text[cursor..block.start]);
        cursor = block.end;
        match replacements.remove(&block.key) {
            Some(Some(serialized)) => {
                output.push_str(serialized.trim_end_matches('\n'));
                output.push('\n');
            }
            Some(None) => {}
            None => output.push_str(&text[block.start..block.end]),
        }
    }
    output.push_str(&text[cursor..]);
    for serialized in replacements.into_values().flatten() {
        if !output.ends_with('\n') && !output.is_empty() {
            output.push('\n');
        }
        if !output.ends_with("\n\n") && !output.trim().is_empty() {
            output.push('\n');
        }
        output.push_str(serialized.trim_end_matches('\n'));
        output.push('\n');
    }
    Ok(output)
}

#[derive(Debug, Clone)]
struct TopLevelObjectBlock {
    key: String,
    start: usize,
    end: usize,
}

fn top_level_object_blocks(text: &str) -> Vec<TopLevelObjectBlock> {
    #[derive(Debug)]
    struct LineInfo {
        start: usize,
        key: Option<String>,
        comment_or_blank: bool,
    }

    let mut lines = Vec::new();
    let mut offset = 0;
    for line in text.split_inclusive('\n') {
        let line_without_newline = line.trim_end_matches(['\r', '\n']);
        lines.push(LineInfo {
            start: offset,
            key: top_level_key(line_without_newline),
            comment_or_blank: line_without_newline.trim().is_empty()
                || line_without_newline.starts_with('#'),
        });
        offset += line.len();
    }
    if offset < text.len() {
        let line = &text[offset..];
        lines.push(LineInfo {
            start: offset,
            key: top_level_key(line),
            comment_or_blank: line.trim().is_empty() || line.starts_with('#'),
        });
    }
    let keyed_lines = lines
        .iter()
        .enumerate()
        .filter_map(|(index, line)| {
            line.key
                .as_ref()
                .map(|key| (index, key.clone(), line.start))
        })
        .collect::<Vec<_>>();

    keyed_lines
        .iter()
        .enumerate()
        .map(|(index, (line_index, key, start))| {
            let end = keyed_lines
                .get(index + 1)
                .map(|(next_line_index, _, next_start)| {
                    let mut boundary = *next_start;
                    let mut candidate = *next_line_index;
                    while candidate > line_index + 1 && lines[candidate - 1].comment_or_blank {
                        candidate -= 1;
                        boundary = lines[candidate].start;
                    }
                    boundary
                })
                .unwrap_or(text.len());
            TopLevelObjectBlock {
                key: key.clone(),
                start: *start,
                end,
            }
        })
        .collect()
}

fn top_level_key(line: &str) -> Option<String> {
    if line.is_empty() || line.starts_with(char::is_whitespace) || line.starts_with('#') {
        return None;
    }
    let (key, rest) = line.split_once(':')?;
    if key.is_empty()
        || key == "---"
        || key.chars().any(|character| {
            character.is_whitespace() || matches!(character, '[' | ']' | '{' | '}' | ',')
        })
        || !(rest.is_empty() || rest.starts_with(char::is_whitespace))
    {
        return None;
    }
    Some(key.to_string())
}

fn introduced_error_diagnostics(
    before: &ProjectAnalysis,
    after: &ProjectAnalysis,
) -> Vec<ProjectDiagnostic> {
    after
        .diagnostics
        .iter()
        .filter(|diagnostic| {
            diagnostic.severity == DiagnosticSeverity::Error
                && !before.diagnostics.contains(diagnostic)
        })
        .cloned()
        .collect()
}

fn overlay_after_save(
    saved_path: ProjectPath,
    content: String,
    overlays: Vec<ProjectOverlay>,
) -> Vec<ProjectOverlay> {
    let mut next = overlays
        .into_iter()
        .filter(|overlay| overlay.path != saved_path)
        .collect::<Vec<_>>();
    next.push(ProjectOverlay {
        path: saved_path,
        content,
    });
    next
}

pub fn analyze_project(project_path: impl AsRef<Path>, project_key: &str) -> ProjectAnalysis {
    analyze_project_with_overlays(project_path, Some(project_key), Vec::new())
}

pub fn analyze_project_with_overlays(
    project_path: impl AsRef<Path>,
    project_key: Option<&str>,
    overlays: Vec<ProjectOverlay>,
) -> ProjectAnalysis {
    let root_path = ProjectPath::new(project_path.as_ref());
    let mut session = AnalysisSession::new(overlays);
    session.visit_file(root_path.clone());

    let inferred_project_key = if let Some(project_key) = project_key {
        Some(project_key.to_string())
    } else {
        infer_project_key(&root_path, &mut session)
    };

    let mut resolved = None;
    if !session.has_errors() {
        if let Some(root_file) = session
            .files
            .get(&root_path)
            .and_then(|analyzed| analyzed.file.as_ref())
        {
            if let Some(project_key) = inferred_project_key.as_deref() {
                let mut loader = AnalysisImportResolver {
                    files: &session.files,
                };
                match lower_project(
                    root_file,
                    project_key,
                    &root_path,
                    |source_path, import, expected| loader.resolve(source_path, import, expected),
                ) {
                    Ok(project) => resolved = Some(project),
                    Err(error) => {
                        let (path, range) = session.locate_lower_error(&root_path, &error);
                        session.diagnostics.push(ProjectDiagnostic {
                            path,
                            range,
                            severity: DiagnosticSeverity::Error,
                            code: DiagnosticCode::Lower,
                            message: error.to_string(),
                        });
                    }
                }
            }
        }
    }

    ProjectAnalysis {
        root_path,
        project_key: inferred_project_key.unwrap_or_default(),
        files: session.files,
        diagnostics: session.diagnostics,
        resolved,
    }
}

fn infer_project_key(root_path: &ProjectPath, session: &mut AnalysisSession) -> Option<String> {
    let Some(root_file) = session
        .files
        .get(root_path)
        .and_then(|analyzed| analyzed.file.as_ref())
    else {
        return None;
    };

    let project_keys = root_file
        .iter()
        .filter_map(|(key, object)| match object {
            DawnObject::Project(_) => Some(key.clone()),
            _ => None,
        })
        .collect::<Vec<_>>();

    match project_keys.as_slice() {
        [project_key] => Some(project_key.clone()),
        [] => {
            session.diagnostics.push(ProjectDiagnostic {
                path: root_path.clone(),
                range: None,
                severity: DiagnosticSeverity::Error,
                code: DiagnosticCode::ProjectKey,
                message: "root file must contain one project object, but found none".to_string(),
            });
            None
        }
        _ => {
            session.diagnostics.push(ProjectDiagnostic {
                path: root_path.clone(),
                range: None,
                severity: DiagnosticSeverity::Error,
                code: DiagnosticCode::ProjectKey,
                message: format!(
                    "root file must contain one project object, but found {}",
                    project_keys.len()
                ),
            });
            None
        }
    }
}

pub fn load_project(
    project_path: impl AsRef<Path>,
    project_key: &str,
) -> Result<ResolvedProject, LoadProjectError> {
    let project_path = ProjectPath::new(project_path.as_ref());
    let file = load_dawn_file(&project_path)?;
    let mut loader = FsImportLoader::default();

    lower_project(
        &file,
        project_key,
        &project_path,
        |source_path, import, expected| loader.resolve(source_path, import, expected),
    )
    .map_err(LoadProjectError::Lower)
}

pub fn lower_project(
    file: &DawnFile,
    project_key: &str,
    source_path: &ProjectPath,
    mut resolver: impl FnMut(&ProjectPath, &ImportRef, ObjectKind) -> Result<ResolvedImport, LowerError>,
) -> Result<ResolvedProject, LowerError> {
    let object = file
        .get(project_key)
        .ok_or_else(|| LowerError::MissingProject {
            key: project_key.to_string(),
        })?;
    let DawnObject::Project(project) = object else {
        return Err(LowerError::WrongObjectKind {
            key: project_key.to_string(),
            expected: ObjectKind::Project,
            actual: object.kind(),
        });
    };
    lower_project_object(project, source_path, &mut resolver)
}

impl<M: ModelMode> DawnObject<M> {
    pub fn kind(&self) -> ObjectKind {
        match self {
            Self::Project(_) => ObjectKind::Project,
            Self::Display(_) => ObjectKind::Display,
            Self::Controller(_) => ObjectKind::Controller,
            Self::Layout(_) => ObjectKind::Layout,
            Self::Fixture(_) => ObjectKind::Fixture,
            Self::Patch(_) => ObjectKind::Patch,
            Self::Sequence(_) => ObjectKind::Sequence,
        }
    }
}

fn lower_project_object(
    project: &Project<Authored>,
    source_path: &ProjectPath,
    resolver: &mut impl FnMut(
        &ProjectPath,
        &ImportRef,
        ObjectKind,
    ) -> Result<ResolvedImport, LowerError>,
) -> Result<Project<Resolved>, LowerError> {
    let (display, display_source) =
        resolve_display(&project.display, source_path, ObjectKind::Display, resolver)?;
    let (layout, layout_source) = resolve_layout(
        &display.layout,
        &display_source,
        ObjectKind::Layout,
        resolver,
    )?;

    let mut sequences = Vec::with_capacity(project.sequences.len());
    for sequence in &project.sequences {
        let (sequence, sequence_source) =
            resolve_sequence(sequence, source_path, ObjectKind::Sequence, resolver)?;
        sequences.push(lower_sequence(
            &sequence,
            &sequence_source,
            &layout,
            &layout_source,
            resolver,
        )?);
    }

    Ok(Project {
        name: project.name.clone(),
        display: lower_display(&display, &display_source, resolver)?,
        sequences,
    })
}

fn lower_display(
    display: &Display<Authored>,
    source_path: &ProjectPath,
    resolver: &mut impl FnMut(
        &ProjectPath,
        &ImportRef,
        ObjectKind,
    ) -> Result<ResolvedImport, LowerError>,
) -> Result<Display<Resolved>, LowerError> {
    let mut controllers = Vec::with_capacity(display.controllers.len());
    for controller in &display.controllers {
        let (controller, _) = resolve_controller(controller, source_path, resolver)?;
        controllers.push(controller);
    }
    let controller_indices = controller_indices(&controllers)?;

    let (layout, layout_source) =
        resolve_layout(&display.layout, source_path, ObjectKind::Layout, resolver)?;
    let layout = lower_layout(&layout, &layout_source, resolver)?;
    let fixture_indices = fixture_indices(&layout.fixtures)?;

    let (patch, _) = resolve_patch(&display.patch, source_path, ObjectKind::Patch, resolver)?;
    let patch = lower_patch(&patch, &fixture_indices, &controller_indices)?;

    Ok(Display {
        name: display.name.clone(),
        controllers,
        patch,
        layout,
    })
}

fn lower_layout(
    layout: &Layout<Authored>,
    source_path: &ProjectPath,
    resolver: &mut impl FnMut(
        &ProjectPath,
        &ImportRef,
        ObjectKind,
    ) -> Result<ResolvedImport, LowerError>,
) -> Result<Layout<Resolved>, LowerError> {
    let mut fixtures = Vec::with_capacity(layout.fixtures.len());
    for placement in &layout.fixtures {
        let (fixture, _) = resolve_fixture(&placement.fixture, source_path, resolver)?;
        fixtures.push(FixturePlacement {
            id: placement.id.clone(),
            fixture,
            transform: placement.transform,
        });
    }

    let fixture_indices = fixture_indices(&fixtures)?;
    let mut groups = Vec::with_capacity(layout.groups.len());
    let mut group_names = HashMap::with_capacity(layout.groups.len());
    for (group_index, group) in layout.groups.iter().enumerate() {
        if group_names
            .insert(group.name.clone(), GroupIndex(group_index))
            .is_some()
        {
            return Err(LowerError::DuplicateGroupName {
                name: group.name.clone(),
            });
        }

        let mut members = Vec::with_capacity(group.members.len());
        for member in &group.members {
            let id = member.as_str();
            let Some(index) = fixture_indices.get(id).copied() else {
                return Err(LowerError::UnknownFixture { id: id.to_string() });
            };
            members.push(index);
        }
        groups.push(Group {
            name: group.name.clone(),
            members,
        });
    }

    Ok(Layout {
        name: layout.name.clone(),
        units: layout.units,
        fixtures,
        groups,
    })
}

fn lower_patch(
    patch: &Patch<Authored>,
    fixtures: &HashMap<String, FixtureIndex>,
    controllers: &HashMap<String, ControllerIndex>,
) -> Result<Patch<Resolved>, LowerError> {
    let mut routes = Vec::with_capacity(patch.routes.len());
    for route in &patch.routes {
        let fixture = fixtures
            .get(route.fixture.as_str())
            .copied()
            .ok_or_else(|| LowerError::UnknownFixture {
                id: route.fixture.as_str().to_string(),
            })?;
        let controller = controllers
            .get(route.controller.as_str())
            .copied()
            .ok_or_else(|| LowerError::UnknownController {
                name: route.controller.as_str().to_string(),
            })?;
        routes.push(Route {
            fixture,
            controller,
            universe: route.universe,
            start: route.start,
        });
    }

    Ok(Patch { routes })
}

fn lower_sequence(
    sequence: &Sequence<Authored>,
    sequence_source_path: &ProjectPath,
    layout: &Layout<Authored>,
    layout_source_path: &ProjectPath,
    resolver: &mut impl FnMut(
        &ProjectPath,
        &ImportRef,
        ObjectKind,
    ) -> Result<ResolvedImport, LowerError>,
) -> Result<Sequence<Resolved>, LowerError> {
    let resolved_layout = lower_layout(layout, layout_source_path, resolver)?;
    let fixtures = fixture_indices(&resolved_layout.fixtures)?;
    let groups = group_indices(&resolved_layout.groups)?;

    let mut effect_indices = HashMap::with_capacity(sequence.effects.len());
    let mut effects = Vec::with_capacity(sequence.effects.len());
    for (effect_index, effect) in sequence.effects.iter().enumerate() {
        if effect_indices
            .insert(effect.id.clone(), SequenceEffectIndex(effect_index))
            .is_some()
        {
            return Err(LowerError::DuplicateSequenceEffectId {
                id: effect.id.clone(),
            });
        }
        effects.push(lower_sequence_effect(
            effect,
            &fixtures,
            &groups,
            sequence_source_path,
        )?);
    }

    let mut automation_clips = Vec::with_capacity(sequence.automation_clips.len());
    for clip in &sequence.automation_clips {
        let mut targets = Vec::with_capacity(clip.targets.len());
        for target in &clip.targets {
            let id = target.as_str();
            let Some(index) = effect_indices.get(id).copied() else {
                return Err(LowerError::UnknownSequenceEffect { id: id.to_string() });
            };
            targets.push(index);
        }
        automation_clips.push(AutomationClip {
            id: clip.id.clone(),
            start: clip.start.clone(),
            duration: clip.duration.clone(),
            curve: clip.curve.clone(),
            targets,
        });
    }

    Ok(Sequence {
        duration: sequence.duration.clone(),
        frame_rate: sequence.frame_rate,
        audio: sequence
            .audio
            .as_ref()
            .map(|audio| resolve_path(sequence_source_path, audio.path())),
        effects,
        automation_clips,
    })
}

fn lower_sequence_effect(
    effect: &SequenceEffect<Authored>,
    fixtures: &HashMap<String, FixtureIndex>,
    groups: &HashMap<String, GroupIndex>,
    source_path: &ProjectPath,
) -> Result<SequenceEffect<Resolved>, LowerError> {
    let target = match &effect.target {
        EffectTarget::Group(group) => {
            let name = group.as_str();
            let Some(index) = groups.get(name).copied() else {
                return Err(LowerError::UnknownGroup {
                    name: name.to_string(),
                });
            };
            EffectTarget::Group(index)
        }
        EffectTarget::Fixture(fixture) => {
            let id = fixture.as_str();
            let Some(index) = fixtures.get(id).copied() else {
                return Err(LowerError::UnknownFixture { id: id.to_string() });
            };
            EffectTarget::Fixture(index)
        }
    };

    Ok(SequenceEffect {
        id: effect.id.clone(),
        start: effect.start.clone(),
        duration: effect.duration.clone(),
        target,
        params: effect.params.clone(),
        script: match &effect.script {
            InlineOrImport::Inline(script) => ScriptSource::Inline(script.clone()),
            InlineOrImport::Import { import } => {
                ScriptSource::External(resolve_path(source_path, import.path()))
            }
        },
    })
}

fn fixture_indices(
    fixtures: &[FixturePlacement<Resolved>],
) -> Result<HashMap<String, FixtureIndex>, LowerError> {
    let mut indices = HashMap::with_capacity(fixtures.len());
    for (index, fixture) in fixtures.iter().enumerate() {
        if indices
            .insert(fixture.id.clone(), FixtureIndex(index))
            .is_some()
        {
            return Err(LowerError::DuplicateFixtureId {
                id: fixture.id.clone(),
            });
        }
    }
    Ok(indices)
}

fn controller_indices(
    controllers: &[Controller],
) -> Result<HashMap<String, ControllerIndex>, LowerError> {
    let mut indices = HashMap::with_capacity(controllers.len());
    for (index, controller) in controllers.iter().enumerate() {
        if indices
            .insert(controller.name.clone(), ControllerIndex(index))
            .is_some()
        {
            return Err(LowerError::DuplicateControllerName {
                name: controller.name.clone(),
            });
        }
    }
    Ok(indices)
}

fn group_indices(groups: &[Group<Resolved>]) -> Result<HashMap<String, GroupIndex>, LowerError> {
    let mut indices = HashMap::with_capacity(groups.len());
    for (index, group) in groups.iter().enumerate() {
        if indices
            .insert(group.name.clone(), GroupIndex(index))
            .is_some()
        {
            return Err(LowerError::DuplicateGroupName {
                name: group.name.clone(),
            });
        }
    }
    Ok(indices)
}

fn resolve_path(source_path: &ProjectPath, import_path: &DawnPath) -> DawnPath {
    if import_path.as_path().is_absolute() {
        return DawnPath::new(ProjectPath::new(import_path.as_path()).as_path());
    }

    DawnPath::new(resolve_import_file_path(source_path, import_path).as_path())
}

fn resolve_import(
    source_path: &ProjectPath,
    import: &ImportRef,
    expected: ObjectKind,
    resolver: &mut impl FnMut(
        &ProjectPath,
        &ImportRef,
        ObjectKind,
    ) -> Result<ResolvedImport, LowerError>,
) -> Result<ResolvedImport, LowerError> {
    let resolved = resolver(source_path, import, expected)?;
    if resolved.object.kind() != expected {
        return Err(LowerError::WrongImportedObjectKind {
            import: import.raw().to_string(),
            expected,
            actual: resolved.object.kind(),
        });
    }
    Ok(resolved)
}

#[derive(Default)]
struct AnalysisSession {
    files: IndexMap<ProjectPath, AnalyzedFile>,
    diagnostics: Vec<ProjectDiagnostic>,
    visiting: HashSet<ProjectPath>,
    overlays: HashMap<ProjectPath, String>,
}

impl AnalysisSession {
    fn new(overlays: Vec<ProjectOverlay>) -> Self {
        Self {
            overlays: overlays
                .into_iter()
                .map(|overlay| (overlay.path, overlay.content))
                .collect(),
            ..Self::default()
        }
    }

    fn has_errors(&self) -> bool {
        self.diagnostics
            .iter()
            .any(|diagnostic| diagnostic.severity == DiagnosticSeverity::Error)
    }

    fn visit_file(&mut self, path: ProjectPath) {
        if self.files.contains_key(&path) || !self.visiting.insert(path.clone()) {
            return;
        }

        let text = match self
            .overlays
            .get(&path)
            .cloned()
            .map(Ok)
            .unwrap_or_else(|| fs::read_to_string(path.as_path()))
        {
            Ok(text) => text,
            Err(source) => {
                self.diagnostics.push(ProjectDiagnostic {
                    path: path.clone(),
                    range: None,
                    severity: DiagnosticSeverity::Error,
                    code: DiagnosticCode::Io,
                    message: format!("failed to read `{}`: {source}", path.display()),
                });
                self.files.insert(
                    path.clone(),
                    AnalyzedFile {
                        path: path.clone(),
                        text: None,
                        file: None,
                    },
                );
                self.visiting.remove(&path);
                return;
            }
        };

        let file = match serde_yaml::from_str::<DawnFile>(&text) {
            Ok(file) => Some(file),
            Err(source) => {
                self.diagnostics.push(ProjectDiagnostic {
                    path: path.clone(),
                    range: yaml_error_range(&source),
                    severity: DiagnosticSeverity::Error,
                    code: DiagnosticCode::Yaml,
                    message: source.to_string(),
                });
                None
            }
        };

        let imports = file
            .as_ref()
            .map(collect_file_imports)
            .unwrap_or_else(Vec::new);
        self.files.insert(
            path.clone(),
            AnalyzedFile {
                path: path.clone(),
                text: Some(text.clone()),
                file,
            },
        );

        for import in imports {
            let import_path = resolve_import_file_path(&path, import.path());
            if !self.can_load_file(&import_path) {
                self.diagnostics.push(ProjectDiagnostic {
                    path: path.clone(),
                    range: import_range(&text, &import),
                    severity: DiagnosticSeverity::Error,
                    code: DiagnosticCode::Import,
                    message: format!(
                        "failed to read import `{}`: file `{}` was not found",
                        import.raw(),
                        import_path.display()
                    ),
                });
                continue;
            }

            self.visit_file(import_path);
        }

        self.visiting.remove(&path);
    }

    fn can_load_file(&self, path: &ProjectPath) -> bool {
        self.overlays.contains_key(path) || path.as_path().is_file()
    }

    fn locate_lower_error(
        &self,
        root_path: &ProjectPath,
        error: &LowerError,
    ) -> (ProjectPath, Option<TextRange>) {
        let token = match error {
            LowerError::MissingProject { key } => Some(key.as_str()),
            LowerError::WrongObjectKind { key, .. } => Some(key.as_str()),
            LowerError::WrongImportedObjectKind { import, .. } => Some(import.as_str()),
            LowerError::Import { import, .. } => Some(import.as_str()),
            LowerError::DuplicateFixtureId { id } => Some(id.as_str()),
            LowerError::UnknownFixture { id } => Some(id.as_str()),
            LowerError::DuplicateControllerName { name } => Some(name.as_str()),
            LowerError::UnknownController { name } => Some(name.as_str()),
            LowerError::DuplicateGroupName { name } => Some(name.as_str()),
            LowerError::UnknownGroup { name } => Some(name.as_str()),
            LowerError::DuplicateSequenceEffectId { id } => Some(id.as_str()),
            LowerError::UnknownSequenceEffect { id } => Some(id.as_str()),
        };

        if let Some(token) = token {
            if let Some((path, range)) = self.find_token(root_path, token) {
                return (path, Some(range));
            }
        }

        (root_path.clone(), None)
    }

    fn find_token(
        &self,
        preferred_path: &ProjectPath,
        token: &str,
    ) -> Option<(ProjectPath, TextRange)> {
        if let Some(file) = self.files.get(preferred_path) {
            if let Some(text) = file.text.as_deref() {
                if let Some(range) = find_text_range(text, token) {
                    return Some((preferred_path.clone(), range));
                }
            }
        }

        for (path, file) in &self.files {
            if path == preferred_path {
                continue;
            }
            if let Some(text) = file.text.as_deref() {
                if let Some(range) = find_text_range(text, token) {
                    return Some((path.clone(), range));
                }
            }
        }

        None
    }
}

struct AnalysisImportResolver<'a> {
    files: &'a IndexMap<ProjectPath, AnalyzedFile>,
}

impl AnalysisImportResolver<'_> {
    fn resolve(
        &mut self,
        source_path: &ProjectPath,
        import: &ImportRef,
        _expected: ObjectKind,
    ) -> Result<ResolvedImport, LowerError> {
        let import_path = resolve_import_file_path(source_path, import.path());
        let analyzed = self
            .files
            .get(&import_path)
            .ok_or_else(|| LowerError::Import {
                import: import.raw().to_string(),
                message: format!("file `{}` was not loaded", import_path.display()),
            })?;
        let file = analyzed.file.as_ref().ok_or_else(|| LowerError::Import {
            import: import.raw().to_string(),
            message: format!("file `{}` did not parse", import_path.display()),
        })?;
        let object = select_imported_object(file, import)?;

        Ok(ResolvedImport {
            source_path: import_path,
            object,
        })
    }
}

fn yaml_error_range(error: &serde_yaml::Error) -> Option<TextRange> {
    error.location().map(|location| {
        let line = location.line().saturating_sub(1) as u32;
        let character = location.column().saturating_sub(1) as u32;
        TextRange {
            start: TextPosition { line, character },
            end: TextPosition {
                line,
                character: character.saturating_add(1),
            },
        }
    })
}

fn find_text_range(text: &str, needle: &str) -> Option<TextRange> {
    for (line_index, line) in text.lines().enumerate() {
        if let Some(column) = line.find(needle) {
            return Some(TextRange {
                start: TextPosition {
                    line: line_index as u32,
                    character: column as u32,
                },
                end: TextPosition {
                    line: line_index as u32,
                    character: column.saturating_add(needle.len()) as u32,
                },
            });
        }
    }
    None
}

fn import_range(text: &str, import: &ImportRef) -> Option<TextRange> {
    find_text_range(text, import.raw())
        .or_else(|| find_text_range(text, &import.path().to_slash_string()))
}

fn collect_file_imports(file: &DawnFile) -> Vec<ImportRef> {
    let mut imports = Vec::new();
    for object in file.values() {
        collect_object_imports(object, &mut imports);
    }
    imports
}

fn collect_object_imports(object: &DawnObject<Authored>, imports: &mut Vec<ImportRef>) {
    match object {
        DawnObject::Project(project) => collect_project_imports(project, imports),
        DawnObject::Display(display) => collect_display_imports(display, imports),
        DawnObject::Controller(_) => {}
        DawnObject::Layout(layout) => collect_layout_imports(layout, imports),
        DawnObject::Fixture(_) => {}
        DawnObject::Patch(_) => {}
        DawnObject::Sequence(_) => {}
    }
}

fn collect_project_imports(project: &Project<Authored>, imports: &mut Vec<ImportRef>) {
    match &project.display {
        InlineOrImport::Inline(display) => collect_display_imports(display, imports),
        InlineOrImport::Import { import } => imports.push(import.clone()),
    }
    for sequence in &project.sequences {
        if let InlineOrImport::Import { import } = sequence {
            imports.push(import.clone());
        }
    }
}

fn collect_display_imports(display: &Display<Authored>, imports: &mut Vec<ImportRef>) {
    for controller in &display.controllers {
        if let InlineOrImport::Import { import } = controller {
            imports.push(import.clone());
        }
    }
    match &display.patch {
        InlineOrImport::Inline(_) => {}
        InlineOrImport::Import { import } => imports.push(import.clone()),
    }
    match &display.layout {
        InlineOrImport::Inline(layout) => collect_layout_imports(layout, imports),
        InlineOrImport::Import { import } => imports.push(import.clone()),
    }
}

fn collect_layout_imports(layout: &Layout<Authored>, imports: &mut Vec<ImportRef>) {
    for fixture in &layout.fixtures {
        if let InlineOrImport::Import { import } = &fixture.fixture {
            imports.push(import.clone());
        }
    }
}

#[derive(Default)]
struct FsImportLoader {
    files: HashMap<ProjectPath, DawnFile>,
}

impl FsImportLoader {
    fn resolve(
        &mut self,
        source_path: &ProjectPath,
        import: &ImportRef,
        _expected: ObjectKind,
    ) -> Result<ResolvedImport, LowerError> {
        let import_path = resolve_import_file_path(source_path, import.path());
        let file = self
            .load_cached(&import_path)
            .map_err(|error| LowerError::Import {
                import: import.raw().to_string(),
                message: error.to_string(),
            })?;
        let object = select_imported_object(file, import)?;

        Ok(ResolvedImport {
            source_path: import_path,
            object,
        })
    }

    fn load_cached(&mut self, path: &ProjectPath) -> Result<&DawnFile, LoadProjectError> {
        if !self.files.contains_key(path) {
            let file = load_dawn_file(path)?;
            self.files.insert(path.clone(), file);
        }
        Ok(self
            .files
            .get(path)
            .expect("file was inserted before lookup"))
    }
}

fn select_imported_object(
    file: &DawnFile,
    import: &ImportRef,
) -> Result<DawnObject<Authored>, LowerError> {
    if let Some(object) = import.object() {
        return file
            .get(object.as_str())
            .cloned()
            .ok_or_else(|| LowerError::Import {
                import: import.raw().to_string(),
                message: format!("object `{}` was not found", object.as_str()),
            });
    }

    if file.len() == 1 {
        return Ok(file
            .values()
            .next()
            .expect("file length was checked")
            .clone());
    }

    Err(LowerError::Import {
        import: import.raw().to_string(),
        message: "import must name an object when the target file has zero or multiple objects"
            .to_string(),
    })
}

fn load_dawn_file(path: &ProjectPath) -> Result<DawnFile, LoadProjectError> {
    let text = fs::read_to_string(path.as_path()).map_err(|source| LoadProjectError::Io {
        path: path.clone(),
        source,
    })?;
    serde_yaml::from_str(&text).map_err(|source| LoadProjectError::Yaml {
        path: path.clone(),
        source,
    })
}

fn resolve_import_file_path(source_path: &ProjectPath, import_path: &DawnPath) -> ProjectPath {
    if import_path.as_path().is_absolute() {
        return ProjectPath::new(import_path.as_path());
    }

    source_path
        .parent()
        .map(|parent| parent.join(import_path.as_path()))
        .unwrap_or_else(|| ProjectPath::new(import_path.as_path()))
}

fn relative_import_path(source_path: &ProjectPath, target_path: &ProjectPath) -> String {
    let Some(source_parent) = source_path.as_path().parent() else {
        return target_path.to_slash_string();
    };
    path_to_slash_string(&relative_path_between(source_parent, target_path.as_path()))
}

fn relative_path_between(from_dir: &Path, target_path: &Path) -> PathBuf {
    let from = lexically_normalize_path(from_dir);
    let target = lexically_normalize_path(target_path);
    let from_components = from.components().collect::<Vec<_>>();
    let target_components = target.components().collect::<Vec<_>>();

    let mut common = 0;
    while common < from_components.len()
        && common < target_components.len()
        && from_components[common] == target_components[common]
    {
        common += 1;
    }

    if common == 0 {
        return target;
    }

    let mut relative = PathBuf::new();
    for component in &from_components[common..] {
        if matches!(component, Component::Normal(_)) {
            relative.push("..");
        }
    }
    for component in &target_components[common..] {
        relative.push(component.as_os_str());
    }

    if relative.as_os_str().is_empty() {
        PathBuf::from(".")
    } else {
        relative
    }
}

fn lexically_normalize_path(path: &Path) -> PathBuf {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::Prefix(_) | Component::RootDir => normalized.push(component.as_os_str()),
            Component::CurDir => {}
            Component::ParentDir => {
                normalized.pop();
            }
            Component::Normal(part) => normalized.push(part),
        }
    }
    normalized
}

fn path_to_slash_string(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

fn resolve_display(
    value: &InlineOrImport<Display<Authored>>,
    source_path: &ProjectPath,
    expected: ObjectKind,
    resolver: &mut impl FnMut(
        &ProjectPath,
        &ImportRef,
        ObjectKind,
    ) -> Result<ResolvedImport, LowerError>,
) -> Result<(Display<Authored>, ProjectPath), LowerError> {
    match value {
        InlineOrImport::Inline(display) => Ok((display.clone(), source_path.clone())),
        InlineOrImport::Import { import } => {
            let resolved = resolve_import(source_path, import, expected, resolver)?;
            let DawnObject::Display(display) = resolved.object else {
                unreachable!("resolved import kind was checked");
            };
            Ok((display, resolved.source_path))
        }
    }
}

fn resolve_controller(
    value: &InlineOrImport<Controller>,
    source_path: &ProjectPath,
    resolver: &mut impl FnMut(
        &ProjectPath,
        &ImportRef,
        ObjectKind,
    ) -> Result<ResolvedImport, LowerError>,
) -> Result<(Controller, ProjectPath), LowerError> {
    match value {
        InlineOrImport::Inline(controller) => Ok((controller.clone(), source_path.clone())),
        InlineOrImport::Import { import } => {
            let resolved = resolve_import(source_path, import, ObjectKind::Controller, resolver)?;
            let DawnObject::Controller(controller) = resolved.object else {
                unreachable!("resolved import kind was checked");
            };
            Ok((controller, resolved.source_path))
        }
    }
}

fn resolve_layout(
    value: &InlineOrImport<Layout<Authored>>,
    source_path: &ProjectPath,
    expected: ObjectKind,
    resolver: &mut impl FnMut(
        &ProjectPath,
        &ImportRef,
        ObjectKind,
    ) -> Result<ResolvedImport, LowerError>,
) -> Result<(Layout<Authored>, ProjectPath), LowerError> {
    match value {
        InlineOrImport::Inline(layout) => Ok((layout.clone(), source_path.clone())),
        InlineOrImport::Import { import } => {
            let resolved = resolve_import(source_path, import, expected, resolver)?;
            let DawnObject::Layout(layout) = resolved.object else {
                unreachable!("resolved import kind was checked");
            };
            Ok((layout, resolved.source_path))
        }
    }
}

fn resolve_fixture(
    value: &InlineOrImport<Fixture>,
    source_path: &ProjectPath,
    resolver: &mut impl FnMut(
        &ProjectPath,
        &ImportRef,
        ObjectKind,
    ) -> Result<ResolvedImport, LowerError>,
) -> Result<(Fixture, ProjectPath), LowerError> {
    match value {
        InlineOrImport::Inline(fixture) => Ok((fixture.clone(), source_path.clone())),
        InlineOrImport::Import { import } => {
            let resolved = resolve_import(source_path, import, ObjectKind::Fixture, resolver)?;
            let DawnObject::Fixture(fixture) = resolved.object else {
                unreachable!("resolved import kind was checked");
            };
            Ok((fixture, resolved.source_path))
        }
    }
}

fn resolve_patch(
    value: &InlineOrImport<Patch<Authored>>,
    source_path: &ProjectPath,
    expected: ObjectKind,
    resolver: &mut impl FnMut(
        &ProjectPath,
        &ImportRef,
        ObjectKind,
    ) -> Result<ResolvedImport, LowerError>,
) -> Result<(Patch<Authored>, ProjectPath), LowerError> {
    match value {
        InlineOrImport::Inline(patch) => Ok((patch.clone(), source_path.clone())),
        InlineOrImport::Import { import } => {
            let resolved = resolve_import(source_path, import, expected, resolver)?;
            let DawnObject::Patch(patch) = resolved.object else {
                unreachable!("resolved import kind was checked");
            };
            Ok((patch, resolved.source_path))
        }
    }
}

fn resolve_sequence(
    value: &InlineOrImport<Sequence<Authored>>,
    source_path: &ProjectPath,
    expected: ObjectKind,
    resolver: &mut impl FnMut(
        &ProjectPath,
        &ImportRef,
        ObjectKind,
    ) -> Result<ResolvedImport, LowerError>,
) -> Result<(Sequence<Authored>, ProjectPath), LowerError> {
    match value {
        InlineOrImport::Inline(sequence) => Ok((sequence.clone(), source_path.clone())),
        InlineOrImport::Import { import } => {
            let resolved = resolve_import(source_path, import, expected, resolver)?;
            let DawnObject::Sequence(sequence) = resolved.object else {
                unreachable!("resolved import kind was checked");
            };
            Ok((sequence, resolved.source_path))
        }
    }
}
