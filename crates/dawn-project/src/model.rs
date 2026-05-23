use std::fmt;

use indexmap::IndexMap;
use serde::de::{self, Visitor};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

pub type DawnFile = IndexMap<String, DawnObject>;

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
pub enum DawnObject {
    Project(Project),
    Display(Display),
    Controller(Controller),
    Layout(Layout),
    Fixture(Fixture),
    Patch(Patch),
    Sequence(Sequence),
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(transparent)]
pub struct DawnPath(String);

impl DawnPath {
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
pub struct Project {
    pub name: String,
    pub display: InlineOrImport<Display>,
    #[serde(default)]
    pub sequences: Vec<InlineOrImport<Sequence>>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Display {
    pub name: String,
    #[serde(default)]
    pub controllers: Vec<InlineOrImport<Controller>>,
    pub patch: InlineOrImport<Patch>,
    pub layout: InlineOrImport<Layout>,
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
pub struct Layout {
    pub name: String,
    pub units: DistanceUnit,
    #[serde(default)]
    pub fixtures: Vec<FixturePlacement>,
    #[serde(default)]
    pub groups: Vec<Group>,
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
pub struct FixturePlacement {
    pub id: String,
    pub fixture: InlineOrImport<Fixture>,
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
pub struct Group {
    pub name: String,
    pub members: Vec<FixtureRef>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Patch {
    #[serde(default)]
    pub routes: Vec<Route>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Route {
    pub fixture: FixtureRef,
    pub controller: ControllerRef,
    pub universe: u32,
    pub start: u32,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Sequence {
    pub duration: Time,
    pub frame_rate: u32,
    pub audio: Option<ImportRef>,
    #[serde(default)]
    pub effects: Vec<SequenceEffect>,
    #[serde(default)]
    pub automation_clips: Vec<AutomationClip>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(tag = "type", content = "name", rename_all = "snake_case")]
pub enum EffectTarget {
    Group(GroupRef),
    Fixture(FixtureRef),
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SequenceEffect {
    pub id: String,
    pub start: Time,
    pub duration: Time,
    pub target: EffectTarget,
    #[serde(default)]
    pub params: IndexMap<String, EffectParam>,
    pub script: InlineOrImport<String>,
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
pub struct AutomationClip {
    pub id: String,
    pub start: Time,
    pub duration: Time,
    pub curve: InlineOrImport<Curve>,
    #[serde(default)]
    pub targets: Vec<SequenceEffectRef>,
}
