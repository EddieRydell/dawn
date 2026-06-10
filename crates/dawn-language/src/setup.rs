use camino::Utf8PathBuf;
use indexmap::IndexMap;
use std::net::IpAddr;

use crate::values::{DistanceSpan, Point3, Rotation3, Scale3};

pub struct Setup {
    pub controllers: Vec<ControllerInst>,
    pub patch: Patch,
    pub layout: Layout,
}

pub struct ControllerInst {
    pub key: ControllerInstKey,
    pub definition: ControllerDefinitionKey,
    pub address: Option<ControllerAddress>,
}

#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct ControllerInstKey {
    pub key: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct ControllerDefinitionKey {
    pub source_path: Utf8PathBuf,
    pub controller_name: String,
}

pub struct ControllerDefinition {
    pub key: ControllerDefinitionKey,
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

pub struct Patch {
    pub routes: Vec<PatchRoute>,
}

pub struct PatchRoute {
    pub fixture: FixtureInstKey,
    pub fixture_pixels: PixelRange,
    pub controller: ControllerInstKey,
    pub output: ControllerOutputIndex,
    pub start_channel: u32,
}

pub struct PixelRange {
    pub start: u32,
    pub count: u32,
}

pub struct ControllerOutputIndex(pub u32);

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
pub struct ControllerDefinitionStore {
    pub definitions: IndexMap<ControllerDefinitionKey, ControllerDefinition>,
}

impl ControllerDefinitionStore {
    pub fn get(&self, key: &ControllerDefinitionKey) -> Option<&ControllerDefinition> {
        self.definitions.get(key)
    }

    pub fn insert(&mut self, definition: ControllerDefinition) -> Option<ControllerDefinition> {
        self.definitions.insert(definition.key.clone(), definition)
    }
}
