use std::fmt;

use indexmap::IndexMap;
use serde::de::{self, Visitor};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::path::{ImportPath, ProjectPath};

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
    type EffectParamCurve: fmt::Debug + Clone + Serialize + for<'de> Deserialize<'de>;
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
    type EffectParamCurve = InlineOrImport<Curve>;
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
    type SequenceAudio = Option<ProjectPath>;
    type EffectTargetGroup = GroupIndex;
    type EffectTargetFixture = FixtureIndex;
    type SequenceEffectScript = ScriptSource;
    type EffectParamCurve = Curve;
    type AutomationClipCurve = Curve;
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
    Curve(Curve),
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
    path: ImportPath,
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

    pub fn path(&self) -> &ImportPath {
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
            path: ImportPath::new(path),
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
    DEFAULT_BULB_SIZE
}

pub const DEFAULT_BULB_SIZE: f64 = 1.0;
pub const MIN_BULB_SIZE: f64 = 0.05;
pub const BULB_SIZE_UNIT_RADIUS: f64 = 0.035;

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
    serialize = "EffectTarget<M>: Serialize, M::SequenceEffectScript: Serialize, EffectParam<M>: Serialize",
    deserialize = "EffectTarget<M>: Deserialize<'de>, M::SequenceEffectScript: Deserialize<'de>, EffectParam<M>: Deserialize<'de>"
))]
pub struct SequenceEffect<M: ModelMode = Authored> {
    pub id: String,
    pub start: Time,
    pub duration: Time,
    pub target: EffectTarget<M>,
    #[serde(default)]
    pub params: IndexMap<String, EffectParam<M>>,
    pub script: M::SequenceEffectScript,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(tag = "type", content = "value", rename_all = "snake_case")]
pub enum ScriptSource {
    Inline(String),
    External(ProjectPath),
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
#[serde(deny_unknown_fields)]
pub struct Curve {
    pub points: Vec<CurvePoint>,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CurvePoint {
    pub time: f64,
    pub value: f64,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
#[serde(bound(
    serialize = "M::EffectParamCurve: Serialize",
    deserialize = "M::EffectParamCurve: Deserialize<'de>"
))]
pub enum EffectParam<M: ModelMode = Authored> {
    Integer { value: u64 },
    Float { value: f64 },
    Flags { value: Flags },
    Color { value: Color },
    Curve { curve: M::EffectParamCurve },
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
#[serde(rename_all = "snake_case")]
pub enum ObjectKind {
    Project,
    Display,
    Controller,
    Layout,
    Fixture,
    Patch,
    Sequence,
    Curve,
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
            Self::Curve => "curve",
        })
    }
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
            Self::Curve(_) => ObjectKind::Curve,
        }
    }
}
