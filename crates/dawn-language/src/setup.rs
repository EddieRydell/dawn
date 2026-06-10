use indexmap::IndexMap;
use std::net::IpAddr;

use crate::values::{DistanceSpan, Point3, Rotation3, Scale3};

#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct DisplayId(pub String);

#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct ControllerId(pub String);

#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct ControllerDefinitionId(pub String);

#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct LayoutId(pub String);

#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct PatchId(pub String);

#[derive(Clone, Debug, PartialEq)]
pub struct Display {
    pub id: DisplayId,
    pub layout: LayoutId,
    pub patch: PatchId,
    pub controllers: Vec<ControllerId>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ControllerDefinition {
    pub protocol: Protocol,
    pub address: Option<ControllerAddress>,
    pub outputs: Vec<ControllerOutput>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Protocol {
    E131,
    Artnet,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ControllerAddress {
    pub ip: IpAddr,
    pub port: u16,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ControllerOutput {
    pub channel_order: RgbChannelOrder,
    pub pixels: usize,
    pub first_universe: u32,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RgbChannelOrder {
    Rgb,
    Rbg,
    Grb,
    Gbr,
    Brg,
    Bgr,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Patch {
    pub id: PatchId,
    pub routes: Vec<PatchRoute>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct PatchRoute {
    pub fixture: FixtureInstanceId,
    pub fixture_pixels: PixelRange,
    pub controller: ControllerId,
    pub output: ControllerOutputIndex,
    pub start_channel_offset: u32,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PixelRange {
    pub start: u32,
    pub count: u32,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ControllerOutputIndex(pub u32);

#[derive(Clone, Debug, PartialEq)]
pub struct Layout {
    pub id: LayoutId,
    pub target_order: Vec<LayoutTarget>,
    pub fixtures: Vec<FixtureInst>,
    pub groups: Vec<FixtureGroup>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum LayoutTarget {
    Fixture(FixtureInstanceId),
    Group(FixtureGroupId),
}

#[derive(Clone, Debug, PartialEq)]
pub struct FixtureGroup {
    pub id: FixtureGroupId,
    pub name: String,
    pub fixtures: Vec<FixtureInstanceId>,
}

#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct FixtureGroupId(pub u32);

#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct FixtureInstanceId(pub u32);

#[derive(Clone, Debug, PartialEq)]
pub struct FixtureInst {
    pub id: FixtureInstanceId,
    pub name: String,
    pub definition: FixtureDefinitionId,
    pub position: Point3,
    pub rotation: Rotation3,
    pub scale: Scale3,
}

#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct FixtureDefinitionId(pub String);

#[derive(Clone, Debug, PartialEq)]
pub struct FixtureDefinition {
    pub bulb_radius: DistanceSpan,
    pub geometry: Geometry,
}

#[derive(Clone, Debug, PartialEq)]
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

#[derive(Clone, Debug, Default, PartialEq)]
pub struct FixtureDefinitionStore {
    pub definitions: IndexMap<FixtureDefinitionId, FixtureDefinition>,
}

impl FixtureDefinitionStore {
    pub fn get(&self, key: &FixtureDefinitionId) -> Option<&FixtureDefinition> {
        self.definitions.get(key)
    }

    pub fn insert(
        &mut self,
        key: FixtureDefinitionId,
        definition: FixtureDefinition,
    ) -> Option<FixtureDefinition> {
        self.definitions.insert(key, definition)
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct ControllerDefinitionStore {
    pub definitions: IndexMap<ControllerDefinitionId, ControllerDefinition>,
}

impl ControllerDefinitionStore {
    pub fn get(&self, key: &ControllerDefinitionId) -> Option<&ControllerDefinition> {
        self.definitions.get(key)
    }

    pub fn insert(
        &mut self,
        key: ControllerDefinitionId,
        definition: ControllerDefinition,
    ) -> Option<ControllerDefinition> {
        self.definitions.insert(key, definition)
    }
}
