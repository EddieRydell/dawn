use std::fmt;

use indexmap::IndexMap;
use serde::de::{self, Visitor};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::path::Utf8PathBuf;

#[derive(Debug, Clone, Copy, Deserialize, Serialize)]
pub enum Authored {}

#[derive(Debug, Clone, Copy, Deserialize, Serialize)]
pub enum Resolved {}

pub type AuthoredProject = Project<Authored>;
pub type ResolvedProject = Project<Resolved>;

#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct DawnImport {
    pub from: Utf8PathBuf,
    #[serde(rename = "as")]
    pub alias: String,
}

#[derive(Debug, Clone, Default)]
pub struct DawnFile {
    pub imports: Vec<DawnImport>,
    pub objects: IndexMap<String, DawnObject<Authored>>,
}

impl DawnFile {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn len(&self) -> usize {
        self.objects.len()
    }

    pub fn is_empty(&self) -> bool {
        self.objects.is_empty()
    }

    pub fn get(&self, key: &str) -> Option<&DawnObject<Authored>> {
        self.objects.get(key)
    }

    pub fn insert(
        &mut self,
        key: String,
        value: DawnObject<Authored>,
    ) -> Option<DawnObject<Authored>> {
        self.objects.insert(key, value)
    }

    pub fn iter(&self) -> indexmap::map::Iter<'_, String, DawnObject<Authored>> {
        self.objects.iter()
    }

    pub fn values(&self) -> indexmap::map::Values<'_, String, DawnObject<Authored>> {
        self.objects.values()
    }
}

impl<'a> IntoIterator for &'a DawnFile {
    type Item = (&'a String, &'a DawnObject<Authored>);
    type IntoIter = indexmap::map::Iter<'a, String, DawnObject<Authored>>;

    fn into_iter(self) -> Self::IntoIter {
        self.objects.iter()
    }
}

impl<'de> Deserialize<'de> for DawnFile {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let mut raw = IndexMap::<String, serde_yaml::Value>::deserialize(deserializer)?;
        let imports = match raw.shift_remove("imports") {
            Some(value) => serde_yaml::from_value::<Vec<DawnImport>>(value)
                .map_err(|error| de::Error::custom(error.to_string()))?,
            None => Vec::new(),
        };
        let mut objects = IndexMap::with_capacity(raw.len());
        for (key, value) in raw {
            let object = serde_yaml::from_value::<DawnObject<Authored>>(value)
                .map_err(|error| de::Error::custom(format!("{key}: {error}")))?;
            objects.insert(key, object);
        }
        Ok(Self { imports, objects })
    }
}

impl Serialize for DawnFile {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut raw = IndexMap::<String, serde_yaml::Value>::new();
        if !self.imports.is_empty() {
            raw.insert(
                "imports".to_string(),
                serde_yaml::to_value(&self.imports).map_err(serde::ser::Error::custom)?,
            );
        }
        for (key, object) in &self.objects {
            raw.insert(
                key.clone(),
                serde_yaml::to_value(object).map_err(serde::ser::Error::custom)?,
            );
        }
        raw.serialize(serializer)
    }
}

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
    type ProjectDisplay = InlineOrRef<Display<Authored>>;
    type ProjectSequence = InlineOrRef<Sequence<Authored>>;
    type DisplayController = InlineOrRef<Controller>;
    type DisplayPatch = InlineOrRef<Patch<Authored>>;
    type DisplayLayout = InlineOrRef<Layout<Authored>>;
    type LayoutFixture = FixturePlacement<Authored>;
    type FixturePlacementFixture = InlineOrRef<Fixture>;
    type GroupMember = FixtureId;
    type RouteFixture = FixtureId;
    type RouteController = ControllerRef;
    type SequenceAudio = Option<AssetPath>;
    type EffectTargetGroup = GroupRef;
    type EffectTargetFixture = FixtureId;
    type SequenceEffectScript = InlineScriptOrRef;
    type EffectParamCurve = InlineOrRef<Curve>;
    type AutomationClipCurve = InlineOrRef<Curve>;
    type AutomationClipTarget = u32;
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
    type SequenceAudio = Option<Utf8PathBuf>;
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Deserialize, Serialize)]
#[serde(transparent)]
pub struct FixtureId(pub u32);

impl fmt::Display for FixtureId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}", self.0)
    }
}

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
string_ref!(GroupRef);
string_ref!(ControllerRef);

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct SymbolRef {
    raw: String,
    alias: Option<String>,
    name: ObjectName,
}

impl SymbolRef {
    pub fn new(raw: impl Into<String>) -> Result<Self, String> {
        serde_yaml::from_value(serde_yaml::Value::String(raw.into()))
            .map_err(|error| error.to_string())
    }

    pub fn raw(&self) -> &str {
        &self.raw
    }

    pub fn alias(&self) -> Option<&str> {
        self.alias.as_deref()
    }

    pub fn name(&self) -> &ObjectName {
        &self.name
    }
}

impl<'de> Deserialize<'de> for SymbolRef {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let raw = String::deserialize(deserializer)?;
        let (alias, name) = match raw.split_once('.') {
            Some((alias, name)) => {
                validate_identifier(alias, "reference alias").map_err(de::Error::custom)?;
                validate_identifier(name, "reference name").map_err(de::Error::custom)?;
                (Some(alias.to_string()), name.to_string())
            }
            None => {
                validate_identifier(&raw, "reference name").map_err(de::Error::custom)?;
                (None, raw.clone())
            }
        };
        Ok(Self {
            raw: raw.clone(),
            alias,
            name: ObjectName(name),
        })
    }
}

impl Serialize for SymbolRef {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.raw)
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(untagged)]
pub enum InlineOrRef<T> {
    Ref(SymbolRef),
    Inline(T),
}

impl<T> InlineOrRef<T> {
    pub fn symbol_ref(&self) -> Option<&SymbolRef> {
        match self {
            Self::Ref(reference) => Some(reference),
            Self::Inline(_) => None,
        }
    }

    pub fn inline(&self) -> Option<&T> {
        match self {
            Self::Inline(value) => Some(value),
            Self::Ref(_) => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InlineScriptOrRef {
    Inline { inline: String },
    Ref(SymbolRef),
}

impl<'de> Deserialize<'de> for InlineScriptOrRef {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = serde_yaml::Value::deserialize(deserializer)?;
        match value {
            serde_yaml::Value::String(raw) => match SymbolRef::new(raw.clone()) {
                Ok(reference) => Ok(Self::Ref(reference)),
                Err(_) => Ok(Self::Inline { inline: raw }),
            },
            other => {
                #[derive(Deserialize)]
                #[serde(deny_unknown_fields)]
                struct Inline {
                    inline: String,
                }
                let inline = Inline::deserialize(other)
                    .map_err(|error| de::Error::custom(error.to_string()))?;
                Ok(Self::Inline {
                    inline: inline.inline,
                })
            }
        }
    }
}

impl Serialize for InlineScriptOrRef {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match self {
            Self::Inline { inline } => {
                #[derive(Serialize)]
                struct Inline<'a> {
                    inline: &'a str,
                }
                Inline { inline }.serialize(serializer)
            }
            Self::Ref(reference) => reference.serialize(serializer),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AssetPath {
    raw: String,
    path: Utf8PathBuf,
}

impl AssetPath {
    pub fn new(raw: impl Into<String>) -> Result<Self, String> {
        serde_yaml::from_value(serde_yaml::Value::String(raw.into()))
            .map_err(|error| error.to_string())
    }

    pub fn raw(&self) -> &str {
        &self.raw
    }

    pub fn path(&self) -> &Utf8PathBuf {
        &self.path
    }
}

impl<'de> Deserialize<'de> for AssetPath {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let raw = String::deserialize(deserializer)?;
        if raw.trim().is_empty() {
            return Err(de::Error::custom("asset path must not be empty"));
        }
        Ok(Self {
            path: Utf8PathBuf::from(raw.as_str()),
            raw,
        })
    }
}

impl Serialize for AssetPath {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.raw)
    }
}

pub fn validate_identifier(value: &str, label: &str) -> Result<(), String> {
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Deserialize, Serialize)]
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
    pub target_order: Vec<LayoutTargetRef>,
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

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DistanceUnit {
    #[default]
    Meters,
    Feet,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
#[serde(bound(
    serialize = "M::FixturePlacementFixture: Serialize",
    deserialize = "M::FixturePlacementFixture: Deserialize<'de>"
))]
pub struct FixturePlacement<M: ModelMode = Authored> {
    pub id: FixtureId,
    pub name: String,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Deserialize, Serialize)]
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum LayoutTargetKind {
    Group,
    Fixture,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct LayoutTargetRef {
    #[serde(rename = "type")]
    pub kind: LayoutTargetKind,
    pub name: String,
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
    serialize = "M::SequenceAudio: Serialize, SequenceMarkCollection: Serialize, SequenceEffect<M>: Serialize, AutomationClip<M>: Serialize",
    deserialize = "M::SequenceAudio: Deserialize<'de>, SequenceMarkCollection: Deserialize<'de>, SequenceEffect<M>: Deserialize<'de>, AutomationClip<M>: Deserialize<'de>"
))]
pub struct Sequence<M: ModelMode = Authored> {
    pub duration: Time,
    pub frame_rate: u32,
    pub audio: M::SequenceAudio,
    #[serde(default)]
    pub mark_collections: Vec<SequenceMarkCollection>,
    #[serde(default)]
    pub effects: Vec<SequenceEffect<M>>,
    #[serde(default)]
    pub automation_clips: Vec<AutomationClip<M>>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SequenceMarkCollection {
    pub key: String,
    pub name: String,
    pub color: String,
    #[serde(default)]
    pub marks: Vec<Time>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
#[serde(bound(
    serialize = "M::EffectTargetGroup: Serialize, M::EffectTargetFixture: Serialize",
    deserialize = "M::EffectTargetGroup: Deserialize<'de>, M::EffectTargetFixture: Deserialize<'de>"
))]
pub enum EffectTarget<M: ModelMode = Authored> {
    Group { name: M::EffectTargetGroup },
    Fixture { id: M::EffectTargetFixture },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SequenceEffectScope {
    PerFixture,
    WholeTarget,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
#[serde(bound(
    serialize = "EffectTarget<M>: Serialize, M::SequenceEffectScript: Serialize, EffectParam<M>: Serialize",
    deserialize = "EffectTarget<M>: Deserialize<'de>, M::SequenceEffectScript: Deserialize<'de>, EffectParam<M>: Deserialize<'de>"
))]
pub struct SequenceEffect<M: ModelMode = Authored> {
    pub id: u32,
    pub start: Time,
    pub duration: Time,
    pub target: EffectTarget<M>,
    pub scope: SequenceEffectScope,
    #[serde(default)]
    pub params: IndexMap<String, EffectParam<M>>,
    pub script: M::SequenceEffectScript,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(tag = "type", content = "value", rename_all = "snake_case")]
pub enum ScriptSource {
    Inline(String),
    External(Utf8PathBuf),
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(transparent)]
pub struct Flags {
    pub values: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Color {
    pub red: u8,
    pub green: u8,
    pub blue: u8,
}

impl Color {
    pub fn new(red: u8, green: u8, blue: u8) -> Self {
        Self { red, green, blue }
    }

    pub fn parse(value: &str) -> Result<Self, String> {
        let raw = value
            .strip_prefix('#')
            .ok_or_else(|| "color literal must start with `#`".to_string())?;
        if raw.len() != 6 || !raw.chars().all(|character| character.is_ascii_hexdigit()) {
            return Err("color literal must look like `#rrggbb`".to_string());
        }
        let red = u8::from_str_radix(&raw[0..2], 16)
            .map_err(|_| "red channel must be hexadecimal".to_string())?;
        let green = u8::from_str_radix(&raw[2..4], 16)
            .map_err(|_| "green channel must be hexadecimal".to_string())?;
        let blue = u8::from_str_radix(&raw[4..6], 16)
            .map_err(|_| "blue channel must be hexadecimal".to_string())?;
        Ok(Self { red, green, blue })
    }

    pub fn to_hex(self) -> String {
        format!("#{:02x}{:02x}{:02x}", self.red, self.green, self.blue)
    }

    pub fn scale(self, factor: f64) -> Self {
        Self {
            red: scale_channel(self.red, factor),
            green: scale_channel(self.green, factor),
            blue: scale_channel(self.blue, factor),
        }
    }

    pub fn mix(self, other: Self, amount: f64) -> Self {
        let amount = amount.clamp(0.0, 1.0);
        Self {
            red: lerp_channel(self.red, other.red, amount),
            green: lerp_channel(self.green, other.green, amount),
            blue: lerp_channel(self.blue, other.blue, amount),
        }
    }
}

impl Serialize for Color {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.to_hex())
    }
}

impl<'de> Deserialize<'de> for Color {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let raw = String::deserialize(deserializer)?;
        Self::parse(&raw).map_err(de::Error::custom)
    }
}

fn scale_channel(channel: u8, factor: f64) -> u8 {
    ((channel as f64) * factor).round().clamp(0.0, 255.0) as u8
}

fn lerp_channel(left: u8, right: u8, amount: f64) -> u8 {
    ((left as f64) + ((right as f64) - (left as f64)) * amount)
        .round()
        .clamp(0.0, 255.0) as u8
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CurveValueType {
    Float,
    Color,
}

#[derive(Debug, Clone, PartialEq)]
pub enum CurveValue {
    Float(f64),
    Color(Color),
}

impl CurveValue {
    pub fn value_type(&self) -> CurveValueType {
        match self {
            Self::Float(_) => CurveValueType::Float,
            Self::Color(_) => CurveValueType::Color,
        }
    }
}

impl Serialize for CurveValue {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match self {
            Self::Float(value) => serializer.serialize_f64(*value),
            Self::Color(value) => value.serialize(serializer),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Curve {
    pub value_type: CurveValueType,
    pub points: Vec<CurvePoint>,
}

impl Curve {
    pub fn evaluate(&self, time: f64) -> Option<CurveValue> {
        let first = self.points.first()?;
        let last = self.points.last()?;
        if time <= first.time {
            return Some(first.value.clone());
        }
        if time >= last.time {
            return Some(last.value.clone());
        }
        for pair in self.points.windows(2) {
            let left = &pair[0];
            let right = &pair[1];
            if time >= left.time && time <= right.time {
                let span = right.time - left.time;
                let amount = if span.abs() < f64::EPSILON {
                    0.0
                } else {
                    (time - left.time) / span
                };
                return Some(match (&left.value, &right.value) {
                    (CurveValue::Float(left), CurveValue::Float(right)) => {
                        CurveValue::Float(left + (right - left) * amount)
                    }
                    (CurveValue::Color(left), CurveValue::Color(right)) => {
                        CurveValue::Color(left.mix(*right, amount))
                    }
                    _ => unreachable!("curve point value types are validated during parsing"),
                });
            }
        }
        Some(last.value.clone())
    }

    pub fn evaluate_float(&self, time: f64) -> Option<f64> {
        match self.evaluate(time)? {
            CurveValue::Float(value) => Some(value),
            CurveValue::Color(_) => None,
        }
    }

    pub fn evaluate_color(&self, time: f64) -> Option<Color> {
        match self.evaluate(time)? {
            CurveValue::Float(_) => None,
            CurveValue::Color(value) => Some(value),
        }
    }
}

impl<'de> Deserialize<'de> for Curve {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct RawCurve {
            value_type: CurveValueType,
            points: Vec<RawCurvePoint>,
        }

        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct RawCurvePoint {
            time: f64,
            value: serde_yaml::Value,
        }

        let raw = RawCurve::deserialize(deserializer)?;
        let mut points = Vec::with_capacity(raw.points.len());
        for point in raw.points {
            let value = match raw.value_type {
                CurveValueType::Float => {
                    point.value.as_f64().map(CurveValue::Float).ok_or_else(|| {
                        de::Error::custom("float curve points must use numeric values")
                    })?
                }
                CurveValueType::Color => {
                    let Some(raw_color) = point.value.as_str() else {
                        return Err(de::Error::custom(
                            "color curve points must use `#rrggbb` string values",
                        ));
                    };
                    CurveValue::Color(Color::parse(raw_color).map_err(de::Error::custom)?)
                }
            };
            points.push(CurvePoint {
                time: point.time,
                value,
            });
        }
        Ok(Self {
            value_type: raw.value_type,
            points,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CurvePoint {
    pub time: f64,
    pub value: CurveValue,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
#[serde(bound(
    serialize = "M::EffectParamCurve: Serialize",
    deserialize = "M::EffectParamCurve: Deserialize<'de>"
))]
pub enum EffectParam<M: ModelMode = Authored> {
    Integer {
        value: u64,
    },
    Float {
        value: f64,
    },
    #[serde(rename = "bool")]
    Boolean {
        value: bool,
    },
    Enum {
        value: String,
    },
    Flags {
        value: Flags,
    },
    Color {
        value: Color,
    },
    Curve {
        curve: M::EffectParamCurve,
    },
    Marks {
        key: String,
    },
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
#[serde(bound(
    serialize = "M::AutomationClipCurve: Serialize, M::AutomationClipTarget: Serialize",
    deserialize = "M::AutomationClipCurve: Deserialize<'de>, M::AutomationClipTarget: Deserialize<'de>"
))]
pub struct AutomationClip<M: ModelMode = Authored> {
    pub id: u32,
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
