use camino::Utf8PathBuf;
use indexmap::IndexMap;
use std::net::IpAddr;

pub struct Setup {
    pub controllers: Vec<Controller>,
    pub patch: Patch,
    pub layout: Layout,
}

pub struct Controller {
    pub key: String,
    pub address: Option<ControllerAddress>,
    pub protocol: Protocol,
    pub outputs: Vec<ControllerOutput>,
}

pub enum Protocol {
    E131,
}

pub struct ControllerAddress {
    pub ip: IpAddr,
    pub port: u16,
}

pub struct ControllerOutput {
    pub channel_order: RgbChannelOrder,
    pub pixels: usize,
    pub first_universe: u32,
}

pub enum RgbChannelOrder {
    Rgb,
    Rbg,
    Grb,
    Gbr,
    Brg,
    Bgr,
}

pub struct Patch {}

pub struct Layout {
    pub fixtures: Vec<FixtureInst>,
    pub groups: Vec<FixtureGroup>,
}

pub struct FixtureGroup {
    pub name: String,
    pub fixtures: Vec<FixtureInstKey>,
}

pub struct FixtureInstKey {
    pub key: String,
}

pub struct FixtureInst {
    pub key: FixtureInstKey,
    pub definition: FixtureDefinitionKey,
    pub position: Point3,
    pub rotation: Rotation3,
    pub scale: Scale3,
}

#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct FixtureDefinitionKey {
    pub source_path: Utf8PathBuf,
    pub fixture_name: String,
}

pub struct FixtureDefinition {
    pub key: FixtureDefinitionKey,
    pub bulb_radius: DistanceSpan,
    pub geometry: Geometry,
}

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

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Distance {
    pub micrometers: i64,
}

impl Distance {
    pub const ZERO: Self = Self { micrometers: 0 };
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DistanceSpan {
    pub micrometers: u64,
}

impl DistanceSpan {
    pub const ZERO: Self = Self { micrometers: 0 };
}

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

#[derive(Clone, Copy, Debug, PartialEq)]
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

#[derive(Clone, Copy, Debug, PartialEq)]
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

#[derive(Default)]
pub struct FixtureDefinitionStore {
    pub definitions: IndexMap<FixtureDefinitionKey, FixtureDefinition>,
}

impl FixtureDefinitionStore {
    pub fn get(&self, key: &FixtureDefinitionKey) -> Option<&FixtureDefinition> {
        self.definitions.get(key)
    }

    pub fn insert(&mut self, definition: FixtureDefinition) -> Option<FixtureDefinition> {
        self.definitions.insert(definition.key.clone(), definition)
    }
}

#[derive(Default)]
pub struct ControllerDefinitionStore {}
