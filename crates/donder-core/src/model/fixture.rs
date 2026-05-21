use std::collections::HashSet;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// Newtype for fixture identity. Prevents mixing up fixture IDs with other integers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS, JsonSchema)]
#[serde(transparent)]
#[ts(export)]
pub struct FixtureId(pub u32);

/// Newtype for group identity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS, JsonSchema)]
#[serde(transparent)]
#[ts(export)]
pub struct GroupId(pub u32);

/// Newtype for controller identity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS, JsonSchema)]
#[serde(transparent)]
#[ts(export)]
pub struct ControllerId(pub u32);

// ── Color & Channel Models ──────────────────────────────────────────

/// How a fixture's channels map to color data.
/// Extensible to cover all common LED and conventional fixture types.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS, JsonSchema)]
#[ts(export)]
pub enum ColorModel {
    /// Single intensity channel (dimmers, single-color LEDs).
    Single,
    /// 3 channels for color. Order specified by `ChannelOrder`.
    Rgb,
    /// 4 channels: RGB + dedicated white.
    Rgbw,
}

impl ColorModel {
    /// Number of DMX channels consumed per pixel for this color model.
    pub const fn channels_per_pixel(self) -> u16 {
        match self {
            ColorModel::Single => 1,
            ColorModel::Rgb => 3,
            ColorModel::Rgbw => 4,
        }
    }
}

/// Channel byte ordering within a pixel. Different protocols/chips use different orders.
/// WS2811 defaults to GRB, WS2812 uses GRB, SK6812 uses GRBW, etc.
///
/// No `Default` impl — callers must specify the correct order for their hardware.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS, JsonSchema)]
#[ts(export)]
pub enum ChannelOrder {
    Rgb,
    Grb,
    Brg,
    Rbg,
    Gbr,
    Bgr,
}

// ── DMX Addressing ──────────────────────────────────────────────────

/// DMX universe number (0-indexed internally, shown as 1-indexed to users).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS, JsonSchema)]
#[serde(transparent)]
#[ts(export)]
pub struct Universe(pub u16);

impl Universe {
    pub const fn display_number(self) -> u16 {
        self.0.saturating_add(1)
    }

    pub fn protocol_number(self) -> Result<u16, &'static str> {
        self.0
            .checked_add(1)
            .ok_or("Internal universe 65535 cannot be represented as an E1.31 universe")
    }

    pub fn from_protocol_number(universe: u16) -> Result<Self, &'static str> {
        if universe == 0 {
            return Err("E1.31 universe 0 is invalid");
        }
        Ok(Self(universe - 1))
    }
}

/// DMX channel address within a universe. Valid range: 1..=512.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, TS, JsonSchema)]
#[serde(transparent)]
#[ts(export)]
pub struct DmxAddress(u16);

impl<'de> Deserialize<'de> for DmxAddress {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let addr = u16::deserialize(deserializer)?;
        DmxAddress::new(addr).ok_or_else(|| {
            serde::de::Error::custom(format!("DMX address {addr} out of valid range 1..=512"))
        })
    }
}

impl DmxAddress {
    /// Create a DMX address. Returns None if out of valid range (1-512).
    pub fn new(addr: u16) -> Option<Self> {
        if (1..=512).contains(&addr) {
            Some(Self(addr))
        } else {
            None
        }
    }

    pub fn get(self) -> u16 {
        self.0
    }
}

// ── Output / Patching ───────────────────────────────────────────────

/// How a fixture's pixel data gets mapped to a physical output.
/// This is the "patch" - it connects logical fixtures to physical channels.
/// Kept as a separate concern from the fixture itself so the same fixture
/// definition can be re-patched to different controllers/universes.
#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
#[ts(export)]
pub struct Patch {
    pub fixture_id: FixtureId,
    #[serde(default)]
    pub fixture_channel_start: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub channel_count: Option<u32>,
    pub output: OutputMapping,
}

impl Patch {
    pub fn fixture_channel_start(&self) -> u32 {
        self.fixture_channel_start
    }

    pub fn resolved_channel_count(&self, fixture: &FixtureDef) -> u32 {
        self.channel_count.unwrap_or_else(|| {
            fixture
                .total_channels()
                .saturating_sub(self.fixture_channel_start)
        })
    }

    pub fn fixture_channel_end_exclusive(&self, fixture: &FixtureDef) -> u32 {
        self.fixture_channel_start()
            .saturating_add(self.resolved_channel_count(fixture))
    }
}

/// Where a fixture's channel data is sent.
#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
#[ts(export)]
pub enum OutputMapping {
    /// Standard DMX (E1.31/sACN, ArtNet, or serial DMX).
    Dmx {
        universe: Universe,
        start_address: DmxAddress,
        channel_order: ChannelOrder,
    },
    /// Future: direct pixel protocol output (e.g. WS2811 via a pixel controller).
    /// The controller handles the protocol; we just need to know which output port.
    PixelPort {
        controller_id: ControllerId,
        port: u16,
        channel_order: ChannelOrder,
    },
    /// Generic controller output numbering when the source data does not expose DMX addressing.
    ControllerOutput {
        controller_id: ControllerId,
        start_output: u16,
    },
}

// ── Controller ──────────────────────────────────────────────────────

/// How a controller communicates with the sequencer.
#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
#[ts(export)]
pub enum ControllerProtocol {
    /// E1.31 (Streaming ACN) over network.
    E131 {
        unicast_address: Option<String>,
        #[serde(default)]
        universes: Vec<Universe>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        universe_sizes: Vec<E131UniverseSize>,
    },
    /// ArtNet over network.
    ArtNet { address: Option<String> },
    /// Serial (USB) for direct pixel output.
    Serial { port: String, baud_rate: u32 },
    /// Imported controller whose transport/output protocol could not be identified.
    Unknown,
}

/// Optional E1.31 universe payload size. Some controllers are configured for
/// 510-channel pixel boundaries instead of full 512-slot DMX universes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS, JsonSchema)]
#[ts(export)]
pub struct E131UniverseSize {
    pub universe: Universe,
    pub size: u16,
}

/// A physical controller that drives one or more outputs.
/// Examples: Falcon F16V4, ESPixelStick, Kulp K32, etc.
#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
#[ts(export)]
pub struct Controller {
    pub id: ControllerId,
    pub name: String,
    pub protocol: ControllerProtocol,
}

// ── Pixel & Bulb Types ──────────────────────────────────────────────

/// Whether a fixture uses individually-addressable (smart) or ganged (dumb) pixels.
fn default_pixel_type() -> PixelType {
    PixelType::Smart
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS, JsonSchema)]
#[ts(export)]
pub enum PixelType {
    Smart,
    Dumb,
}

/// Physical bulb shape, affects display size in the preview renderer.
fn default_bulb_shape() -> BulbShape {
    BulbShape::LED
}

fn default_requires_layout() -> bool {
    true
}

fn default_requires_patch() -> bool {
    true
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS, JsonSchema)]
#[ts(export)]
pub enum BulbShape {
    LED,
    C9,
    C7,
    Mini,
    Flood,
    Icicle,
    Globe,
    Snowflake,
}

impl BulbShape {
    /// Default display radius multiplier for this bulb shape.
    pub fn default_display_radius(self) -> f32 {
        match self {
            BulbShape::Mini => 0.8,
            BulbShape::LED => 1.0,
            BulbShape::Icicle => 1.2,
            BulbShape::C7 => 1.5,
            BulbShape::Globe => 1.8,
            BulbShape::C9 => 2.0,
            BulbShape::Snowflake => 2.5,
            BulbShape::Flood => 3.0,
        }
    }
}

// ── Fixtures ────────────────────────────────────────────────────────

/// A fixture definition. Represents a logical light or string of lights.
/// This is purely about *what* the light is, not *how* it's connected.
/// Connection info lives in `Patch` and `Controller`.
#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
#[ts(export)]
pub struct FixtureDef {
    pub id: FixtureId,
    pub name: String,
    pub color_model: ColorModel,
    /// Number of individually addressable pixels. 1 for simple fixtures.
    pub pixel_count: u32,
    #[serde(default = "default_pixel_type")]
    pub pixel_type: PixelType,
    #[serde(default = "default_bulb_shape")]
    pub bulb_shape: BulbShape,
    #[serde(default = "default_requires_layout")]
    pub requires_layout: bool,
    #[serde(default = "default_requires_patch")]
    pub requires_patch: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub display_radius_override: Option<f32>,
    pub channel_order: ChannelOrder,
}

impl FixtureDef {
    /// Total DMX channels this fixture consumes.
    pub fn total_channels(&self) -> u32 {
        self.pixel_count * u32::from(self.color_model.channels_per_pixel())
    }

    /// Effective display radius multiplier (override or bulb shape default).
    pub fn display_radius(&self) -> f32 {
        self.display_radius_override
            .unwrap_or_else(|| self.bulb_shape.default_display_radius())
    }
}

// ── Groups & Targeting ──────────────────────────────────────────────

/// A member of a group: either a direct fixture or a nested sub-group.
#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
#[ts(export)]
pub enum GroupMember {
    Fixture(FixtureId),
    Group(GroupId),
}

/// A named group of fixtures for targeting effects. Supports hierarchical nesting.
#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
#[ts(export)]
pub struct FixtureGroup {
    pub id: GroupId,
    pub name: String,
    pub members: Vec<GroupMember>,
}

impl FixtureGroup {
    /// Recursively resolve all fixture IDs in this group, with cycle detection.
    pub fn resolve_fixture_ids(&self, all_groups: &[FixtureGroup]) -> Vec<FixtureId> {
        let mut result = Vec::new();
        let mut visited = HashSet::new();
        visited.insert(self.id);
        Self::resolve_recursive(&self.members, all_groups, &mut visited, &mut result);
        result
    }

    fn resolve_recursive(
        members: &[GroupMember],
        all_groups: &[FixtureGroup],
        visited: &mut HashSet<GroupId>,
        result: &mut Vec<FixtureId>,
    ) {
        for member in members {
            match member {
                GroupMember::Fixture(id) => result.push(*id),
                GroupMember::Group(gid) => {
                    if visited.insert(*gid) {
                        if let Some(group) = all_groups.iter().find(|g| g.id == *gid) {
                            Self::resolve_recursive(&group.members, all_groups, visited, result);
                        }
                    }
                }
            }
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]
mod tests {
    use super::*;

    fn group(id: u32, members: Vec<GroupMember>) -> FixtureGroup {
        FixtureGroup {
            id: GroupId(id),
            name: format!("Group {id}"),
            members,
        }
    }

    #[test]
    fn flat_group_resolves_to_its_fixtures() {
        let g = group(
            1,
            vec![
                GroupMember::Fixture(FixtureId(10)),
                GroupMember::Fixture(FixtureId(20)),
            ],
        );
        let all = vec![g.clone()];
        let ids = g.resolve_fixture_ids(&all);
        assert_eq!(ids, vec![FixtureId(10), FixtureId(20)]);
    }

    #[test]
    fn nested_group_resolves_recursively() {
        let inner = group(2, vec![GroupMember::Fixture(FixtureId(30))]);
        let outer = group(
            1,
            vec![
                GroupMember::Fixture(FixtureId(10)),
                GroupMember::Group(GroupId(2)),
            ],
        );
        let all = vec![outer.clone(), inner];
        let ids = outer.resolve_fixture_ids(&all);
        assert_eq!(ids, vec![FixtureId(10), FixtureId(30)]);
    }

    #[test]
    fn circular_reference_terminates() {
        let a = group(1, vec![GroupMember::Group(GroupId(2))]);
        let b = group(2, vec![GroupMember::Group(GroupId(1))]);
        let all = vec![a.clone(), b];
        // Should not hang — cycle detection stops it
        let ids = a.resolve_fixture_ids(&all);
        assert!(ids.is_empty());
    }

    #[test]
    fn self_referencing_group_terminates() {
        let g = group(
            1,
            vec![
                GroupMember::Fixture(FixtureId(10)),
                GroupMember::Group(GroupId(1)),
            ],
        );
        let all = vec![g.clone()];
        let ids = g.resolve_fixture_ids(&all);
        assert_eq!(ids, vec![FixtureId(10)]);
    }

    #[test]
    fn empty_group_resolves_to_empty() {
        let g = group(1, vec![]);
        let all = vec![g.clone()];
        let ids = g.resolve_fixture_ids(&all);
        assert!(ids.is_empty());
    }

    #[test]
    fn missing_sub_group_is_skipped() {
        let g = group(
            1,
            vec![
                GroupMember::Fixture(FixtureId(10)),
                GroupMember::Group(GroupId(999)), // doesn't exist
            ],
        );
        let all = vec![g.clone()];
        let ids = g.resolve_fixture_ids(&all);
        assert_eq!(ids, vec![FixtureId(10)]);
    }

    // ── DmxAddress boundary tests ────────────────────────────────

    #[test]
    fn dmx_address_zero_is_none() {
        assert!(DmxAddress::new(0).is_none());
    }

    #[test]
    fn dmx_address_one_is_valid() {
        let addr = DmxAddress::new(1).expect("1 is valid");
        assert_eq!(addr.get(), 1);
    }

    #[test]
    fn dmx_address_512_is_valid() {
        let addr = DmxAddress::new(512).expect("512 is valid");
        assert_eq!(addr.get(), 512);
    }

    #[test]
    fn dmx_address_513_is_none() {
        assert!(DmxAddress::new(513).is_none());
    }

    #[test]
    fn dmx_address_max_is_none() {
        assert!(DmxAddress::new(u16::MAX).is_none());
    }

    #[test]
    fn dmx_address_deserialize_invalid_errors() {
        let result: Result<DmxAddress, _> = serde_json::from_str("0");
        assert!(result.is_err());
        let result: Result<DmxAddress, _> = serde_json::from_str("513");
        assert!(result.is_err());
    }

    #[test]
    fn dmx_address_roundtrip() {
        let addr = DmxAddress::new(256).expect("valid");
        let json = serde_json::to_string(&addr).expect("serialize");
        assert_eq!(json, "256");
        let back: DmxAddress = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back.get(), 256);
    }

    #[test]
    fn protocol_universe_zero_is_rejected() {
        assert!(Universe::from_protocol_number(0).is_err());
        assert_eq!(
            Universe::from_protocol_number(1).expect("valid"),
            Universe(0)
        );
    }

    // ── Deeply nested groups ─────────────────────────────────────

    #[test]
    fn deeply_nested_groups_resolve() {
        let g3 = group(3, vec![GroupMember::Fixture(FixtureId(100))]);
        let g2 = group(2, vec![GroupMember::Group(GroupId(3))]);
        let g1 = group(1, vec![GroupMember::Group(GroupId(2))]);
        let all = vec![g1.clone(), g2, g3];
        let ids = g1.resolve_fixture_ids(&all);
        assert_eq!(ids, vec![FixtureId(100)]);
    }
}
