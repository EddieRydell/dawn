use std::fmt;
use std::marker::PhantomData;
use std::net::{IpAddr, SocketAddr};
use std::str::FromStr;

use indexmap::IndexMap;
use serde::de::{self, Visitor};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::effect_script::{CompiledEffect, EffectParamSchema, EffectScriptKind, EffectVisibility};
use crate::path::{PathStringExt, Utf8PathBuf};

#[derive(Debug, Clone, Copy, Deserialize, Serialize)]
pub enum Authored {}

#[derive(Debug, Clone, Copy, Deserialize, Serialize)]
pub enum Resolved {}

pub type DawnProject = Project<Resolved>;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Deserialize, Serialize)]
#[serde(transparent)]
pub struct SequenceEffectId(pub u32);

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Deserialize, Serialize)]
#[serde(transparent)]
pub struct GroupInstantiationId(pub u32);

impl fmt::Display for GroupInstantiationId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}", self.0)
    }
}

#[derive(Debug, Clone)]
pub enum ResolvedProvenance {
    Inline,
    Named {
        path: Utf8PathBuf,
        symbol: String,
    },
    ExternalEffect {
        path: Utf8PathBuf,
        effect_name: String,
    },
}

#[derive(Debug, Clone)]
pub struct ResolvedObject<T> {
    pub value: T,
    pub provenance: ResolvedProvenance,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DefinitionKey<Name> {
    pub path: Utf8PathBuf,
    pub name: String,
    #[serde(skip)]
    marker: PhantomData<Name>,
}

impl<Name> DefinitionKey<Name> {
    pub fn new(path: Utf8PathBuf, name: impl Into<String>) -> Self {
        Self {
            path,
            name: name.into(),
            marker: PhantomData,
        }
    }

    pub fn display_key(&self) -> String {
        format!("{}#{}", self.path.to_slash_string(), self.name)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Deserialize, Serialize)]
pub enum ProjectDefinitionName {}
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Deserialize, Serialize)]
pub enum DisplayDefinitionName {}
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Deserialize, Serialize)]
pub enum SequenceDefinitionName {}
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Deserialize, Serialize)]
pub enum ControllerDefinitionName {}
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Deserialize, Serialize)]
pub enum PatchDefinitionName {}
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Deserialize, Serialize)]
pub enum LayoutDefinitionName {}
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Deserialize, Serialize)]
pub enum FixtureDefinitionName {}
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Deserialize, Serialize)]
pub enum CurveDefinitionName {}
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Deserialize, Serialize)]
pub enum EffectDefinitionName {}

pub type ProjectDefinitionKey = DefinitionKey<ProjectDefinitionName>;
pub type DisplayDefinitionKey = DefinitionKey<DisplayDefinitionName>;
pub type SequenceDefinitionKey = DefinitionKey<SequenceDefinitionName>;
pub type ControllerDefinitionKey = DefinitionKey<ControllerDefinitionName>;
pub type PatchDefinitionKey = DefinitionKey<PatchDefinitionName>;
pub type LayoutDefinitionKey = DefinitionKey<LayoutDefinitionName>;
pub type FixtureDefinitionKey = DefinitionKey<FixtureDefinitionName>;
pub type CurveDefinitionKey = DefinitionKey<CurveDefinitionName>;
pub type EffectDefinitionKey = DefinitionKey<EffectDefinitionName>;

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ResolvedSymbolRef<K> {
    pub key: K,
    pub reference: SymbolRef,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(untagged)]
pub enum ResolvedInlineOrRef<T, K> {
    Ref(ResolvedSymbolRef<K>),
    Inline(T),
}

impl<T, K> ResolvedInlineOrRef<T, K> {
    pub fn value(&self) -> Option<&T> {
        match self {
            Self::Inline(value) => Some(value),
            Self::Ref(_) => None,
        }
    }

    pub fn symbol_ref(&self) -> Option<&ResolvedSymbolRef<K>> {
        match self {
            Self::Ref(reference) => Some(reference),
            Self::Inline(_) => None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct EffectDefinition<M: ModelMode = Authored> {
    pub source: M::EffectDefinitionSource,
    pub schema: Vec<EffectParamSchema>,
    pub kind: EffectScriptKind,
    pub visibility: EffectVisibility,
    pub compiled: M::EffectDefinitionCompiled,
}

#[derive(Debug, Clone, Default)]
pub struct NoCompiledEffect;

#[derive(Debug, Clone)]
pub struct AuthoredEffectDefinitionSource {
    pub text: String,
}

#[derive(Debug, Clone)]
pub struct ResolvedEffectDefinitionSource {
    pub path: Utf8PathBuf,
    pub effect_name: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ResolvedAssetPath {
    pub path: Utf8PathBuf,
    pub source: AssetPath,
}

#[derive(Debug, Clone)]
pub enum ResolvedSourceFile {
    Dawn {
        imports: Vec<DawnImport>,
        objects: IndexMap<String, ResolvedSourceObject>,
    },
    Effect {
        text: String,
    },
}

#[derive(Debug, Clone)]
pub enum ResolvedSourceObject {
    Project(ProjectDefinitionKey),
    Display(DisplayDefinitionKey),
    Controller(ControllerDefinitionKey),
    Layout(LayoutDefinitionKey),
    Fixture(FixtureDefinitionKey),
    Patch(PatchDefinitionKey),
    Sequence(SequenceDefinitionKey),
    Curve(CurveDefinitionKey),
    Unused(DawnObject<Authored>),
}

#[derive(Debug, Clone, Default)]
pub struct NoProjectStores;

#[derive(Debug, Clone, Default)]
pub struct ResolvedStores {
    pub root_project: Option<ProjectDefinitionKey>,
    pub source_files: IndexMap<Utf8PathBuf, ResolvedSourceFile>,
    pub displays: IndexMap<DisplayDefinitionKey, ResolvedObject<Display<Resolved>>>,
    pub sequences: IndexMap<SequenceDefinitionKey, ResolvedObject<Sequence<Resolved>>>,
    pub controllers: IndexMap<ControllerDefinitionKey, ResolvedObject<Controller>>,
    pub patches: IndexMap<PatchDefinitionKey, ResolvedObject<Patch<Resolved>>>,
    pub layouts: IndexMap<LayoutDefinitionKey, ResolvedObject<Layout<Resolved>>>,
    pub fixture_definitions: IndexMap<FixtureDefinitionKey, ResolvedObject<Fixture>>,
    pub curves: IndexMap<CurveDefinitionKey, ResolvedObject<Curve>>,
    pub effect_definitions:
        IndexMap<EffectDefinitionKey, ResolvedObject<EffectDefinition<Resolved>>>,
}

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
    type ProjectStores: fmt::Debug + Clone + Default;
    type ProjectDisplay: fmt::Debug + Clone + Serialize + for<'de> Deserialize<'de>;
    type ProjectSequence: fmt::Debug + Clone + Serialize + for<'de> Deserialize<'de>;
    type DisplayController: fmt::Debug + Clone + Serialize + for<'de> Deserialize<'de>;
    type DisplayPatch: fmt::Debug + Clone + Serialize + for<'de> Deserialize<'de>;
    type DisplayLayout: fmt::Debug + Clone + Serialize + for<'de> Deserialize<'de>;
    type LayoutFixture: fmt::Debug + Clone + Serialize + for<'de> Deserialize<'de>;
    type LayoutGroup: fmt::Debug + Clone + Serialize + for<'de> Deserialize<'de>;
    type FixturePlacementFixture: fmt::Debug + Clone + Serialize + for<'de> Deserialize<'de>;
    type GroupMember: fmt::Debug + Clone + Serialize + for<'de> Deserialize<'de>;
    type PatchRoute: fmt::Debug + Clone + Serialize + for<'de> Deserialize<'de>;
    type RouteFixture: fmt::Debug + Clone + Serialize + for<'de> Deserialize<'de>;
    type RouteController: fmt::Debug + Clone + Serialize + for<'de> Deserialize<'de>;
    type SequenceAudio: fmt::Debug + Clone + Serialize + for<'de> Deserialize<'de>;
    type EffectTargetGroup: fmt::Debug + Clone + Serialize + for<'de> Deserialize<'de>;
    type EffectTargetFixture: fmt::Debug + Clone + Serialize + for<'de> Deserialize<'de>;
    type SequenceEffectScript: fmt::Debug + Clone + Serialize + for<'de> Deserialize<'de>;
    type EffectParamCurve: fmt::Debug + Clone + Serialize + for<'de> Deserialize<'de>;
    type AutomationClipCurve: fmt::Debug + Clone + Serialize + for<'de> Deserialize<'de>;
    type AutomationClipTarget: fmt::Debug + Clone + Serialize + for<'de> Deserialize<'de>;
    type SequenceEffectId: fmt::Debug + Clone + Serialize + for<'de> Deserialize<'de>;
    type EffectDefinitionSource: fmt::Debug + Clone;
    type EffectDefinitionCompiled: fmt::Debug + Clone;
}

impl ModelMode for Authored {
    type ProjectStores = NoProjectStores;
    type ProjectDisplay = InlineOrRef<Display<Authored>>;
    type ProjectSequence = InlineOrRef<Sequence<Authored>>;
    type DisplayController = InlineOrRef<Controller>;
    type DisplayPatch = InlineOrRef<Patch<Authored>>;
    type DisplayLayout = InlineOrRef<Layout<Authored>>;
    type LayoutFixture = FixturePlacement<Authored>;
    type LayoutGroup = Group<Authored>;
    type FixturePlacementFixture = InlineOrRef<Fixture>;
    type GroupMember = FixtureId;
    type PatchRoute = Route<Authored>;
    type RouteFixture = FixtureId;
    type RouteController = SymbolRef;
    type SequenceAudio = Option<AssetPath>;
    type EffectTargetGroup = GroupInstantiationId;
    type EffectTargetFixture = FixtureId;
    type SequenceEffectScript = SymbolRef;
    type EffectParamCurve = InlineOrRef<Curve>;
    type AutomationClipCurve = InlineOrRef<Curve>;
    type AutomationClipTarget = u32;
    type SequenceEffectId = u32;
    type EffectDefinitionSource = AuthoredEffectDefinitionSource;
    type EffectDefinitionCompiled = NoCompiledEffect;
}

impl ModelMode for Resolved {
    type ProjectStores = ResolvedStores;
    type ProjectDisplay = ResolvedInlineOrRef<Display<Resolved>, DisplayDefinitionKey>;
    type ProjectSequence = ResolvedInlineOrRef<Sequence<Resolved>, SequenceDefinitionKey>;
    type DisplayController = ResolvedInlineOrRef<Controller, ControllerDefinitionKey>;
    type DisplayPatch = ResolvedInlineOrRef<Patch<Resolved>, PatchDefinitionKey>;
    type DisplayLayout = ResolvedInlineOrRef<Layout<Resolved>, LayoutDefinitionKey>;
    type LayoutFixture = FixturePlacement<Resolved>;
    type LayoutGroup = Group<Resolved>;
    type FixturePlacementFixture = ResolvedInlineOrRef<Fixture, FixtureDefinitionKey>;
    type GroupMember = FixtureId;
    type PatchRoute = Route<Resolved>;
    type RouteFixture = FixtureId;
    type RouteController = ResolvedSymbolRef<ControllerDefinitionKey>;
    type SequenceAudio = Option<ResolvedAssetPath>;
    type EffectTargetGroup = GroupInstantiationId;
    type EffectTargetFixture = FixtureId;
    type SequenceEffectScript = ResolvedSymbolRef<EffectDefinitionKey>;
    type EffectParamCurve = ResolvedInlineOrRef<Curve, CurveDefinitionKey>;
    type AutomationClipCurve = ResolvedInlineOrRef<Curve, CurveDefinitionKey>;
    type AutomationClipTarget = SequenceEffectId;
    type SequenceEffectId = SequenceEffectId;
    type EffectDefinitionSource = ResolvedEffectDefinitionSource;
    type EffectDefinitionCompiled = CompiledEffect;
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct Time {
    nanoseconds: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct TimeSpan {
    nanoseconds: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct Distance {
    micrometers: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct DistanceSpan {
    micrometers: u64,
}

impl Time {
    pub const ZERO: Self = Self { nanoseconds: 0 };

    pub fn from_nanoseconds(nanoseconds: u64) -> Self {
        Self { nanoseconds }
    }

    pub fn try_from_seconds_f64_rounded(seconds: f64) -> Result<Self, &'static str> {
        seconds_to_nanoseconds_rounded(seconds).map(Self::from_nanoseconds)
    }

    pub fn as_seconds_f64(self) -> f64 {
        self.nanoseconds as f64 / NANOS_PER_SECOND as f64
    }

    pub fn as_nanoseconds(self) -> u64 {
        self.nanoseconds
    }
}

impl TimeSpan {
    pub const ZERO: Self = Self { nanoseconds: 0 };

    pub fn from_nanoseconds(nanoseconds: u64) -> Self {
        Self { nanoseconds }
    }

    pub fn try_from_seconds_f64_rounded(seconds: f64) -> Result<Self, &'static str> {
        seconds_to_nanoseconds_rounded(seconds).map(Self::from_nanoseconds)
    }

    pub fn as_seconds_f64(self) -> f64 {
        self.nanoseconds as f64 / NANOS_PER_SECOND as f64
    }

    pub fn as_nanoseconds(self) -> u64 {
        self.nanoseconds
    }
}

impl Distance {
    pub const ZERO: Self = Self { micrometers: 0 };

    pub fn from_micrometers(micrometers: i64) -> Self {
        Self { micrometers }
    }

    pub fn try_from_meters_f64_truncated(meters: f64) -> Result<Self, &'static str> {
        meters_to_micrometers_truncated(meters).map(Self::from_micrometers)
    }

    pub fn try_from_meters_f64_rounded(meters: f64) -> Result<Self, &'static str> {
        meters_to_micrometers_rounded(meters).map(Self::from_micrometers)
    }

    pub fn as_meters_f64(self) -> f64 {
        self.micrometers as f64 / MICROMETERS_PER_METER as f64
    }

    pub fn as_micrometers(self) -> i64 {
        self.micrometers
    }
}

impl DistanceSpan {
    pub const ZERO: Self = Self { micrometers: 0 };

    pub fn from_micrometers(micrometers: u64) -> Self {
        Self { micrometers }
    }

    pub fn try_from_meters_f64_truncated(meters: f64) -> Result<Self, &'static str> {
        let micrometers = meters_to_micrometers_truncated(meters)?;
        if micrometers < 0 {
            return Err("distance span meters must be non-negative");
        }
        Ok(Self::from_micrometers(micrometers as u64))
    }

    pub fn try_from_meters_f64_rounded(meters: f64) -> Result<Self, &'static str> {
        let micrometers = meters_to_micrometers_rounded(meters)?;
        if micrometers < 0 {
            return Err("distance span meters must be non-negative");
        }
        Ok(Self::from_micrometers(micrometers as u64))
    }

    pub fn as_meters_f64(self) -> f64 {
        self.micrometers as f64 / MICROMETERS_PER_METER as f64
    }

    pub fn as_micrometers(self) -> u64 {
        self.micrometers
    }
}

impl Serialize for Time {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&format_nanoseconds_as_seconds(self.nanoseconds))
    }
}

impl Serialize for TimeSpan {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&format_nanoseconds_as_seconds(self.nanoseconds))
    }
}

impl Serialize for Distance {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serialize_distance_meters(self.micrometers, serializer)
    }
}

impl Serialize for DistanceSpan {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serialize_distance_meters(self.micrometers as i64, serializer)
    }
}

impl<'de> Deserialize<'de> for Time {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        deserialize_time_like(deserializer, "time").map(Self::from_nanoseconds)
    }
}

impl<'de> Deserialize<'de> for TimeSpan {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        deserialize_time_like(deserializer, "duration").map(Self::from_nanoseconds)
    }
}

impl<'de> Deserialize<'de> for Distance {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        deserialize_distance_like(deserializer, "distance").map(Self::from_micrometers)
    }
}

impl<'de> Deserialize<'de> for DistanceSpan {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let micrometers = deserialize_distance_like(deserializer, "distance span")?;
        if micrometers < 0 {
            return Err(de::Error::custom(
                "distance span meters must be non-negative",
            ));
        }
        Ok(Self::from_micrometers(micrometers as u64))
    }
}

fn deserialize_time_like<'de, D>(deserializer: D, label: &'static str) -> Result<u64, D::Error>
where
    D: Deserializer<'de>,
{
    struct TimeVisitor {
        label: &'static str,
    }

    impl Visitor<'_> for TimeVisitor {
        type Value = u64;

        fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
            write!(
                formatter,
                "a suffixed {} like 1m10s, 1.5s, 12ms, 25us, or 100ns",
                self.label
            )
        }

        fn visit_u64<E>(self, _: u64) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            Err(E::custom(format!(
                "{} must use an `ns`, `us`, `ms`, `s`, or `m` suffix",
                self.label
            )))
        }

        fn visit_str<E>(self, value: &str) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            parse_time_nanoseconds(value).map_err(E::custom)
        }
    }

    deserializer.deserialize_any(TimeVisitor { label })
}

fn deserialize_distance_like<'de, D>(deserializer: D, label: &'static str) -> Result<i64, D::Error>
where
    D: Deserializer<'de>,
{
    struct DistanceVisitor {
        label: &'static str,
    }

    impl Visitor<'_> for DistanceVisitor {
        type Value = i64;

        fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
            write!(
                formatter,
                "a decimal meter number or a metric distance string using um, mm, cm, or m"
            )
        }

        fn visit_i64<E>(self, value: i64) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            value
                .checked_mul(MICROMETERS_PER_METER)
                .ok_or_else(|| E::custom(format!("{} is too large", self.label)))
        }

        fn visit_u64<E>(self, value: u64) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            let micrometers = value
                .checked_mul(MICROMETERS_PER_METER as u64)
                .ok_or_else(|| E::custom(format!("{} is too large", self.label)))?;
            i64::try_from(micrometers)
                .map_err(|_| E::custom(format!("{} is too large", self.label)))
        }

        fn visit_f64<E>(self, value: f64) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            meters_to_micrometers_truncated(value).map_err(E::custom)
        }

        fn visit_str<E>(self, value: &str) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            parse_metric_distance_micrometers(value).map_err(E::custom)
        }
    }

    deserializer.deserialize_any(DistanceVisitor { label })
}

const NANOS_PER_MICROSECOND: u64 = 1_000;
const NANOS_PER_MILLISECOND: u64 = 1_000_000;
pub const NANOS_PER_SECOND: u64 = 1_000_000_000;
const NANOS_PER_MINUTE: u64 = 60 * NANOS_PER_SECOND;
pub const MICROMETERS_PER_METER: i64 = 1_000_000;
const MICROMETERS_PER_CENTIMETER: i64 = 10_000;
const MICROMETERS_PER_MILLIMETER: i64 = 1_000;

fn parse_time_nanoseconds(value: &str) -> Result<u64, &'static str> {
    if value.is_empty() {
        return Err("time must not be empty");
    }

    let mut rest = value;
    let mut total = 0u64;
    while !rest.is_empty() {
        let amount_len = rest
            .find(|character: char| !character.is_ascii_digit() && character != '.')
            .unwrap_or(rest.len());
        if amount_len == 0 {
            return Err("time segment must start with a number");
        }

        let amount = &rest[..amount_len];
        rest = &rest[amount_len..];

        let (multiplier, suffix_len) = if rest.starts_with("ns") {
            (1, 2)
        } else if rest.starts_with("us") {
            (NANOS_PER_MICROSECOND, 2)
        } else if rest.starts_with("ms") {
            (NANOS_PER_MILLISECOND, 2)
        } else if rest.starts_with('s') {
            (NANOS_PER_SECOND, 1)
        } else if rest.starts_with('m') {
            (NANOS_PER_MINUTE, 1)
        } else {
            return Err("time segment must use `ns`, `us`, `ms`, `s`, or `m`");
        };

        let segment = parse_decimal_amount_as_nanoseconds(amount, multiplier)?;
        total = total.checked_add(segment).ok_or("time is too large")?;
        rest = &rest[suffix_len..];
    }

    Ok(total)
}

fn parse_metric_distance_micrometers(value: &str) -> Result<i64, &'static str> {
    if value.is_empty() {
        return Err("distance must not be empty");
    }
    let (amount, multiplier) = if let Some(amount) = value.strip_suffix("um") {
        (amount, 1)
    } else if let Some(amount) = value.strip_suffix("mm") {
        (amount, MICROMETERS_PER_MILLIMETER)
    } else if let Some(amount) = value.strip_suffix("cm") {
        (amount, MICROMETERS_PER_CENTIMETER)
    } else if let Some(amount) = value.strip_suffix('m') {
        (amount, MICROMETERS_PER_METER)
    } else {
        (value, MICROMETERS_PER_METER)
    };
    parse_decimal_amount_as_micrometers(amount, multiplier)
}

fn parse_decimal_amount_as_nanoseconds(amount: &str, multiplier: u64) -> Result<u64, &'static str> {
    let Some((whole, fractional)) = amount.split_once('.') else {
        return amount
            .parse::<u64>()
            .map_err(|_| "time amount must be a number")?
            .checked_mul(multiplier)
            .ok_or("time is too large");
    };

    if whole.is_empty() || fractional.is_empty() || fractional.contains('.') {
        return Err("time amount must be a number");
    }
    if !whole.chars().all(|character| character.is_ascii_digit())
        || !fractional
            .chars()
            .all(|character| character.is_ascii_digit())
    {
        return Err("time amount must be a number");
    }

    let whole_nanoseconds = whole
        .parse::<u64>()
        .map_err(|_| "time amount must be a number")?
        .checked_mul(multiplier)
        .ok_or("time is too large")?;
    let scale = 10u64
        .checked_pow(fractional.len() as u32)
        .ok_or("time precision is too fine")?;
    let fractional_amount = fractional
        .parse::<u64>()
        .map_err(|_| "time amount must be a number")?;
    let scaled = fractional_amount
        .checked_mul(multiplier)
        .ok_or("time is too large")?;
    if scaled % scale != 0 {
        return Err("time precision is finer than one nanosecond");
    }
    whole_nanoseconds
        .checked_add(scaled / scale)
        .ok_or("time is too large")
}

fn parse_decimal_amount_as_micrometers(
    mut amount: &str,
    multiplier: i64,
) -> Result<i64, &'static str> {
    let sign = if let Some(rest) = amount.strip_prefix('-') {
        amount = rest;
        -1
    } else {
        1
    };

    let Some((whole, fractional)) = amount.split_once('.') else {
        let whole = amount
            .parse::<i64>()
            .map_err(|_| "distance amount must be a number")?;
        return whole
            .checked_mul(multiplier)
            .and_then(|value| value.checked_mul(sign))
            .ok_or("distance is too large");
    };

    if whole.is_empty() || fractional.is_empty() || fractional.contains('.') {
        return Err("distance amount must be a number");
    }
    if !whole.chars().all(|character| character.is_ascii_digit())
        || !fractional
            .chars()
            .all(|character| character.is_ascii_digit())
    {
        return Err("distance amount must be a number");
    }

    let whole_micrometers = whole
        .parse::<i64>()
        .map_err(|_| "distance amount must be a number")?
        .checked_mul(multiplier)
        .ok_or("distance is too large")?;
    let scale = 10i64
        .checked_pow(fractional.len() as u32)
        .ok_or("distance precision is too fine")?;
    let fractional_amount = fractional
        .parse::<i64>()
        .map_err(|_| "distance amount must be a number")?;
    let fractional_micrometers = fractional_amount
        .checked_mul(multiplier)
        .ok_or("distance is too large")?
        / scale;
    whole_micrometers
        .checked_add(fractional_micrometers)
        .and_then(|value| value.checked_mul(sign))
        .ok_or("distance is too large")
}

fn format_nanoseconds_as_seconds(nanoseconds: u64) -> String {
    let seconds = nanoseconds / NANOS_PER_SECOND;
    let fractional = nanoseconds % NANOS_PER_SECOND;
    if fractional == 0 {
        return format!("{seconds}s");
    }
    let mut fractional_text = format!("{fractional:09}");
    while fractional_text.ends_with('0') {
        fractional_text.pop();
    }
    format!("{seconds}.{fractional_text}s")
}

fn serialize_distance_meters<S>(micrometers: i64, serializer: S) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    if micrometers % MICROMETERS_PER_METER == 0 {
        serializer.serialize_i64(micrometers / MICROMETERS_PER_METER)
    } else {
        serializer.serialize_f64(micrometers as f64 / MICROMETERS_PER_METER as f64)
    }
}

fn seconds_to_nanoseconds_rounded(seconds: f64) -> Result<u64, &'static str> {
    if !seconds.is_finite() {
        return Err("time seconds must be finite");
    }
    if seconds < 0.0 {
        return Err("time seconds must be non-negative");
    }
    let nanoseconds = seconds * NANOS_PER_SECOND as f64;
    if nanoseconds > u64::MAX as f64 {
        return Err("time is too large");
    }
    Ok(nanoseconds.round() as u64)
}

fn meters_to_micrometers_truncated(meters: f64) -> Result<i64, &'static str> {
    if !meters.is_finite() {
        return Err("distance meters must be finite");
    }
    let micrometers = meters * MICROMETERS_PER_METER as f64;
    if micrometers < i64::MIN as f64 || micrometers > i64::MAX as f64 {
        return Err("distance is too large");
    }
    Ok(micrometers.trunc() as i64)
}

fn meters_to_micrometers_rounded(meters: f64) -> Result<i64, &'static str> {
    if !meters.is_finite() {
        return Err("distance meters must be finite");
    }
    let micrometers = meters * MICROMETERS_PER_METER as f64;
    if micrometers < i64::MIN as f64 || micrometers > i64::MAX as f64 {
        return Err("distance is too large");
    }
    Ok(micrometers.round() as i64)
}

#[cfg(test)]
mod time_tests {
    use super::{Distance, DistanceSpan, Time, TimeSpan, NANOS_PER_SECOND};

    fn parse_time(value: &str) -> Result<Time, String> {
        serde_yaml::from_str(value).map_err(|error| error.to_string())
    }

    #[test]
    fn parses_exact_time_units() {
        assert_eq!(parse_time("1ns").unwrap().nanoseconds, 1);
        assert_eq!(parse_time("1us").unwrap().nanoseconds, 1_000);
        assert_eq!(parse_time("1ms").unwrap().nanoseconds, 1_000_000);
        assert_eq!(parse_time("1s").unwrap().nanoseconds, NANOS_PER_SECOND);
        assert_eq!(parse_time("1m").unwrap().nanoseconds, 60 * NANOS_PER_SECOND);
        assert_eq!(parse_time("1m1.5s").unwrap().nanoseconds, 61_500_000_000);
    }

    #[test]
    fn parses_decimal_seconds_without_float_rounding() {
        assert_eq!(parse_time("0.016666667s").unwrap().nanoseconds, 16_666_667);
        assert_eq!(parse_time("1.5s").unwrap().nanoseconds, 1_500_000_000);
        assert_eq!(parse_time("0.001ms").unwrap().nanoseconds, 1_000);
    }

    #[test]
    fn serializes_canonical_decimal_seconds() {
        assert_eq!(serde_yaml::to_string(&Time::ZERO).unwrap().trim(), "0s");
        assert_eq!(
            serde_yaml::to_string(&Time::from_nanoseconds(1_500_000_000))
                .unwrap()
                .trim(),
            "1.5s"
        );
        assert_eq!(
            serde_yaml::to_string(&TimeSpan::from_nanoseconds(16_666_667))
                .unwrap()
                .trim(),
            "0.016666667s"
        );
    }

    #[test]
    fn rejects_sub_nanosecond_precision_and_overflow() {
        assert!(parse_time("0.1ns").is_err());
        assert!(parse_time("18446744073709551616ns").is_err());
        assert!(parse_time("1xs").is_err());
    }

    #[test]
    fn parses_metric_distances_and_spans() {
        assert_eq!(
            serde_yaml::from_str::<Distance>("1")
                .unwrap()
                .as_micrometers(),
            1_000_000
        );
        assert_eq!(
            serde_yaml::from_str::<Distance>("1.25m")
                .unwrap()
                .as_micrometers(),
            1_250_000
        );
        assert_eq!(
            serde_yaml::from_str::<Distance>("12.3cm")
                .unwrap()
                .as_micrometers(),
            123_000
        );
        assert_eq!(
            serde_yaml::from_str::<Distance>("2.5mm")
                .unwrap()
                .as_micrometers(),
            2_500
        );
        assert_eq!(
            serde_yaml::from_str::<Distance>("7um")
                .unwrap()
                .as_micrometers(),
            7
        );
        assert_eq!(
            serde_yaml::from_str::<DistanceSpan>("0.001")
                .unwrap()
                .as_micrometers(),
            1_000
        );
    }

    #[test]
    fn serializes_canonical_decimal_meters() {
        assert_eq!(
            serde_yaml::to_string(&Distance::from_micrometers(1_000_000))
                .unwrap()
                .trim(),
            "1"
        );
        assert_eq!(
            serde_yaml::to_string(&Distance::from_micrometers(1_200_000))
                .unwrap()
                .trim(),
            "1.2"
        );
        assert_eq!(
            serde_yaml::to_string(&DistanceSpan::from_micrometers(1))
                .unwrap()
                .trim(),
            "1e-6"
        );
    }

    #[test]
    fn signed_distances_allow_negatives_but_spans_reject_them() {
        assert_eq!(
            serde_yaml::from_str::<Distance>("-0.5")
                .unwrap()
                .as_micrometers(),
            -500_000
        );
        assert!(serde_yaml::from_str::<DistanceSpan>("-0.5").is_err());
    }

    #[test]
    fn distance_api_rejects_non_finite_and_truncates_sub_micrometer() {
        assert!(Distance::try_from_meters_f64_truncated(f64::NAN).is_err());
        assert!(DistanceSpan::try_from_meters_f64_truncated(f64::INFINITY).is_err());
        assert_eq!(
            Distance::try_from_meters_f64_truncated(0.0000009)
                .unwrap()
                .as_micrometers(),
            0
        );
        assert_eq!(
            Distance::try_from_meters_f64_truncated(-0.0000009)
                .unwrap()
                .as_micrometers(),
            0
        );
    }
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
    #[serde(skip)]
    Effect(EffectDefinition<M>),
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
    pub display: M::ProjectDisplay,
    #[serde(default)]
    pub sequences: Vec<M::ProjectSequence>,
    #[serde(skip)]
    pub stores: M::ProjectStores,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
#[serde(bound(
    serialize = "M::DisplayController: Serialize, M::DisplayPatch: Serialize, M::DisplayLayout: Serialize",
    deserialize = "M::DisplayController: Deserialize<'de>, M::DisplayPatch: Deserialize<'de>, M::DisplayLayout: Deserialize<'de>"
))]
pub struct Display<M: ModelMode = Authored> {
    #[serde(default)]
    pub controllers: Vec<M::DisplayController>,
    pub patch: M::DisplayPatch,
    pub layout: M::DisplayLayout,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Controller {
    pub protocol: Protocol,
    #[serde(default)]
    pub destination: Option<ControllerDestination>,
    pub output: ControllerOutput,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ControllerDestination {
    pub address: IpAddr,
    pub port: u16,
}

impl ControllerDestination {
    pub fn socket_addr(self) -> SocketAddr {
        SocketAddr::new(self.address, self.port)
    }
}

impl Serialize for ControllerDestination {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.socket_addr().to_string())
    }
}

impl<'de> Deserialize<'de> for ControllerDestination {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let raw = String::deserialize(deserializer)?;
        let socket_addr = SocketAddr::from_str(&raw).map_err(|_| {
            de::Error::custom(
                "controller destination must be an IP endpoint like `192.168.1.50:5568`",
            )
        })?;
        Ok(Self {
            address: socket_addr.ip(),
            port: socket_addr.port(),
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Protocol {
    Artnet,
    Sacn,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RgbChannelOrder {
    Rgb,
    Rbg,
    Grb,
    Gbr,
    Brg,
    Bgr,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum ControllerOutput {
    PatchedDmx {
        channel_order: RgbChannelOrder,
        universes: Vec<Universe>,
    },
    LinearRgb {
        channel_order: RgbChannelOrder,
        group: GroupInstantiationId,
        output_count: usize,
        pixels_per_output: usize,
        first_universe: u32,
        slots_per_universe: usize,
    },
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
    serialize = "M::LayoutFixture: Serialize, M::LayoutGroup: Serialize",
    deserialize = "M::LayoutFixture: Deserialize<'de>, M::LayoutGroup: Deserialize<'de>"
))]
pub struct Layout<M: ModelMode = Authored> {
    #[serde(default)]
    pub target_order: Vec<LayoutTargetRef>,
    #[serde(default)]
    pub fixtures: Vec<M::LayoutFixture>,
    #[serde(default)]
    pub groups: Vec<M::LayoutGroup>,
}

impl Layout<Resolved> {
    pub fn fixture(&self, index: FixtureIndex) -> Option<&FixturePlacement<Resolved>> {
        self.fixtures.get(index.0)
    }
}

impl Display<Resolved> {
    pub fn controller(&self, index: ControllerIndex) -> Option<&Controller> {
        self.controllers
            .get(index.0)
            .and_then(ResolvedInlineOrRef::value)
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
#[serde(bound(
    serialize = "M::FixturePlacementFixture: Serialize",
    deserialize = "M::FixturePlacementFixture: Deserialize<'de>"
))]
pub struct FixturePlacement<M: ModelMode = Authored> {
    pub id: FixtureId,
    #[serde(default)]
    pub name: Option<String>,
    pub fixture: M::FixturePlacementFixture,
    pub transform: Transform,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Fixture {
    pub color_model: ColorModel,
    #[serde(default = "default_bulb_diameter")]
    pub bulb_diameter: DistanceSpan,
    pub geometry: Geometry,
}

fn default_bulb_diameter() -> DistanceSpan {
    DEFAULT_BULB_DIAMETER
}

pub const DEFAULT_BULB_DIAMETER: DistanceSpan = DistanceSpan {
    micrometers: 10_000,
};

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
    pub x: Distance,
    pub y: Distance,
    pub z: Distance,
}

impl Default for Point3 {
    fn default() -> Self {
        Self {
            x: Distance::ZERO,
            y: Distance::ZERO,
            z: Distance::ZERO,
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
        radius: DistanceSpan,
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
    pub id: GroupInstantiationId,
    #[serde(default)]
    pub name: Option<String>,
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
    pub id: u32,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
#[serde(bound(
    serialize = "M::PatchRoute: Serialize",
    deserialize = "M::PatchRoute: Deserialize<'de>"
))]
pub struct Patch<M: ModelMode = Authored> {
    #[serde(default)]
    pub routes: Vec<M::PatchRoute>,
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
    pub duration: TimeSpan,
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
    Group { id: M::EffectTargetGroup },
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
    serialize = "M::SequenceEffectId: Serialize, EffectTarget<M>: Serialize, M::SequenceEffectScript: Serialize, EffectParam<M>: Serialize",
    deserialize = "M::SequenceEffectId: Deserialize<'de>, EffectTarget<M>: Deserialize<'de>, M::SequenceEffectScript: Deserialize<'de>, EffectParam<M>: Deserialize<'de>"
))]
pub struct SequenceEffect<M: ModelMode = Authored> {
    pub id: M::SequenceEffectId,
    pub start: Time,
    pub duration: TimeSpan,
    pub target: EffectTarget<M>,
    pub scope: SequenceEffectScope,
    #[serde(default)]
    pub params: IndexMap<String, EffectParam<M>>,
    pub script: M::SequenceEffectScript,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ArrayElementType {
    Int,
    Float,
    Bool,
    Color,
    CurveFloat,
    CurveColor,
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
        curve: CurveUse<M>,
    },
    Array {
        element_type: ArrayElementType,
        values: Vec<EffectParamArrayValue<M>>,
    },
    Marks {
        key: String,
    },
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(untagged)]
#[serde(bound(
    serialize = "M::EffectParamCurve: Serialize",
    deserialize = "M::EffectParamCurve: Deserialize<'de>"
))]
pub enum EffectParamArrayValue<M: ModelMode = Authored> {
    Integer(u64),
    Float(f64),
    Boolean(bool),
    Color(Color),
    Curve(CurveUse<M>),
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
#[serde(bound(
    serialize = "M::EffectParamCurve: Serialize",
    deserialize = "M::EffectParamCurve: Deserialize<'de>"
))]
pub struct CurveUse<M: ModelMode = Authored> {
    pub id: u32,
    pub curve: M::EffectParamCurve,
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
    pub duration: TimeSpan,
    pub curve: CurveUse<M>,
    #[serde(default)]
    pub targets: Vec<M::AutomationClipTarget>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
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
    Effect,
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
            Self::Effect => "effect",
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
            Self::Effect(_) => ObjectKind::Effect,
        }
    }
}
