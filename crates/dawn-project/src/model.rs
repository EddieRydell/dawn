use std::collections::{HashMap, HashSet};
use std::error::Error;
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};

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

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(transparent)]
pub struct DawnPath(String);

impl DawnPath {
    pub fn new(path: impl Into<String>) -> Self {
        Self(path.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

macro_rules! string_ref {
    ($name:ident) => {
        #[derive(Debug, Clone, PartialEq, Eq, Hash, Deserialize, Serialize)]
        #[serde(transparent)]
        pub struct $name(String);

        impl $name {
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
            path: DawnPath(path.to_string()),
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

    pub fn import_path(&self) -> Option<&str> {
        self.import_ref().map(|import| import.path().as_str())
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
    pub geometry: Geometry,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ColorModel {
    Rgb,
    Rgba,
    Rgbw,
    Rgbaw,
    White,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Transform {
    pub position: Point3,
    #[serde(default)]
    pub rotation: Rotation3,
    #[serde(default)]
    pub scale: Scale3,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
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
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum Geometry {
    Points {
        points: Vec<Point3>,
    },
    Line {
        from: Point3,
        to: Point3,
        pixels: u32,
    },
    Lines {
        points: Vec<Point3>,
        lines: Vec<LineSegment>,
    },
    Arc {
        center: Point3,
        radius: f64,
        start_degrees: f64,
        end_degrees: f64,
        pixels: u32,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct LineSegment {
    pub from: usize,
    pub to: usize,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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
    pub source_path: DawnPath,
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
        path: PathBuf,
        source: std::io::Error,
    },
    Yaml {
        path: PathBuf,
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
    pub root_path: PathBuf,
    pub project_key: String,
    pub files: IndexMap<PathBuf, AnalyzedFile>,
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
    pub path: PathBuf,
    pub text: Option<String>,
    pub file: Option<DawnFile>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectDiagnostic {
    pub path: PathBuf,
    pub range: Option<TextRange>,
    pub severity: DiagnosticSeverity,
    pub code: DiagnosticCode,
    pub message: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiagnosticSeverity {
    Error,
    Warning,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiagnosticCode {
    Io,
    Yaml,
    Import,
    Lower,
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

pub fn analyze_project(project_path: impl AsRef<Path>, project_key: &str) -> ProjectAnalysis {
    let root_path = absolutize_path(project_path.as_ref());
    let mut session = AnalysisSession::default();
    session.visit_file(root_path.clone());

    let mut resolved = None;
    if !session.has_errors() {
        if let Some(root_file) = session
            .files
            .get(&root_path)
            .and_then(|analyzed| analyzed.file.as_ref())
        {
            let source_path = DawnPath::new(path_to_string(&root_path));
            let mut loader = AnalysisImportResolver {
                files: &session.files,
            };
            match lower_project(
                root_file,
                project_key,
                &source_path,
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

    ProjectAnalysis {
        root_path,
        project_key: project_key.to_string(),
        files: session.files,
        diagnostics: session.diagnostics,
        resolved,
    }
}

pub fn load_project(
    project_path: impl AsRef<Path>,
    project_key: &str,
) -> Result<ResolvedProject, LoadProjectError> {
    let project_path = absolutize_path(project_path.as_ref());
    let file = load_dawn_file(&project_path)?;
    let source_path = DawnPath::new(path_to_string(&project_path));
    let mut loader = FsImportLoader::default();

    lower_project(
        &file,
        project_key,
        &source_path,
        |source_path, import, expected| loader.resolve(source_path, import, expected),
    )
    .map_err(LoadProjectError::Lower)
}

pub fn lower_project(
    file: &DawnFile,
    project_key: &str,
    source_path: &DawnPath,
    mut resolver: impl FnMut(&DawnPath, &ImportRef, ObjectKind) -> Result<ResolvedImport, LowerError>,
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
    source_path: &DawnPath,
    resolver: &mut impl FnMut(&DawnPath, &ImportRef, ObjectKind) -> Result<ResolvedImport, LowerError>,
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
    source_path: &DawnPath,
    resolver: &mut impl FnMut(&DawnPath, &ImportRef, ObjectKind) -> Result<ResolvedImport, LowerError>,
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
    source_path: &DawnPath,
    resolver: &mut impl FnMut(&DawnPath, &ImportRef, ObjectKind) -> Result<ResolvedImport, LowerError>,
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
    sequence_source_path: &DawnPath,
    layout: &Layout<Authored>,
    layout_source_path: &DawnPath,
    resolver: &mut impl FnMut(&DawnPath, &ImportRef, ObjectKind) -> Result<ResolvedImport, LowerError>,
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
    source_path: &DawnPath,
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

fn resolve_path(source_path: &DawnPath, import_path: &DawnPath) -> DawnPath {
    let path = import_path.as_str();
    if path.starts_with('/') || path.contains(':') {
        return DawnPath(path.to_string());
    }

    let source = source_path.as_str().replace('\\', "/");
    let Some((directory, _)) = source.rsplit_once('/') else {
        return DawnPath(path.to_string());
    };
    DawnPath(normalize_path(&format!("{directory}/{path}")))
}

fn normalize_path(path: &str) -> String {
    let mut parts = Vec::new();
    for part in path.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            _ => parts.push(part),
        }
    }
    parts.join("/")
}

fn resolve_import(
    source_path: &DawnPath,
    import: &ImportRef,
    expected: ObjectKind,
    resolver: &mut impl FnMut(&DawnPath, &ImportRef, ObjectKind) -> Result<ResolvedImport, LowerError>,
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
    files: IndexMap<PathBuf, AnalyzedFile>,
    diagnostics: Vec<ProjectDiagnostic>,
    visiting: HashSet<PathBuf>,
}

impl AnalysisSession {
    fn has_errors(&self) -> bool {
        self.diagnostics
            .iter()
            .any(|diagnostic| diagnostic.severity == DiagnosticSeverity::Error)
    }

    fn visit_file(&mut self, path: PathBuf) {
        let path = absolutize_path(&path);
        if self.files.contains_key(&path) || !self.visiting.insert(path.clone()) {
            return;
        }

        let text = match fs::read_to_string(&path) {
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
                text: Some(text),
                file,
            },
        );

        for import in imports {
            self.visit_file(resolve_import_file_path(
                &DawnPath::new(path_to_string(&path)),
                import.path(),
            ));
        }

        self.visiting.remove(&path);
    }

    fn locate_lower_error(
        &self,
        root_path: &Path,
        error: &LowerError,
    ) -> (PathBuf, Option<TextRange>) {
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

        (root_path.to_path_buf(), None)
    }

    fn find_token(&self, preferred_path: &Path, token: &str) -> Option<(PathBuf, TextRange)> {
        if let Some(file) = self.files.get(preferred_path) {
            if let Some(text) = file.text.as_deref() {
                if let Some(range) = find_text_range(text, token) {
                    return Some((preferred_path.to_path_buf(), range));
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
    files: &'a IndexMap<PathBuf, AnalyzedFile>,
}

impl AnalysisImportResolver<'_> {
    fn resolve(
        &mut self,
        source_path: &DawnPath,
        import: &ImportRef,
        _expected: ObjectKind,
    ) -> Result<ResolvedImport, LowerError> {
        let import_path = absolutize_path(&resolve_import_file_path(source_path, import.path()));
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
            source_path: DawnPath::new(path_to_string(&import_path)),
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
    files: HashMap<PathBuf, DawnFile>,
}

impl FsImportLoader {
    fn resolve(
        &mut self,
        source_path: &DawnPath,
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
            source_path: DawnPath::new(path_to_string(&import_path)),
            object,
        })
    }

    fn load_cached(&mut self, path: &Path) -> Result<&DawnFile, LoadProjectError> {
        let path = absolutize_path(path);
        if !self.files.contains_key(&path) {
            let file = load_dawn_file(&path)?;
            self.files.insert(path.clone(), file);
        }
        Ok(self
            .files
            .get(&path)
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

fn load_dawn_file(path: &Path) -> Result<DawnFile, LoadProjectError> {
    let text = fs::read_to_string(path).map_err(|source| LoadProjectError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    serde_yaml::from_str(&text).map_err(|source| LoadProjectError::Yaml {
        path: path.to_path_buf(),
        source,
    })
}

fn resolve_import_file_path(source_path: &DawnPath, import_path: &DawnPath) -> PathBuf {
    let import_path = PathBuf::from(import_path.as_str());
    if import_path.is_absolute() {
        return import_path;
    }

    PathBuf::from(source_path.as_str())
        .parent()
        .map(|parent| parent.join(&import_path))
        .unwrap_or(import_path)
}

fn absolutize_path(path: &Path) -> PathBuf {
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()
            .map(|current_dir| current_dir.join(path))
            .unwrap_or_else(|_| path.to_path_buf())
    }
}

fn path_to_string(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

fn resolve_display(
    value: &InlineOrImport<Display<Authored>>,
    source_path: &DawnPath,
    expected: ObjectKind,
    resolver: &mut impl FnMut(&DawnPath, &ImportRef, ObjectKind) -> Result<ResolvedImport, LowerError>,
) -> Result<(Display<Authored>, DawnPath), LowerError> {
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
    source_path: &DawnPath,
    resolver: &mut impl FnMut(&DawnPath, &ImportRef, ObjectKind) -> Result<ResolvedImport, LowerError>,
) -> Result<(Controller, DawnPath), LowerError> {
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
    source_path: &DawnPath,
    expected: ObjectKind,
    resolver: &mut impl FnMut(&DawnPath, &ImportRef, ObjectKind) -> Result<ResolvedImport, LowerError>,
) -> Result<(Layout<Authored>, DawnPath), LowerError> {
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
    source_path: &DawnPath,
    resolver: &mut impl FnMut(&DawnPath, &ImportRef, ObjectKind) -> Result<ResolvedImport, LowerError>,
) -> Result<(Fixture, DawnPath), LowerError> {
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
    source_path: &DawnPath,
    expected: ObjectKind,
    resolver: &mut impl FnMut(&DawnPath, &ImportRef, ObjectKind) -> Result<ResolvedImport, LowerError>,
) -> Result<(Patch<Authored>, DawnPath), LowerError> {
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
    source_path: &DawnPath,
    expected: ObjectKind,
    resolver: &mut impl FnMut(&DawnPath, &ImportRef, ObjectKind) -> Result<ResolvedImport, LowerError>,
) -> Result<(Sequence<Authored>, DawnPath), LowerError> {
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
