use std::collections::HashMap;
use std::fmt;
use std::net::SocketAddr;

use dawn_project::{
    ColorModel, Controller, ControllerOutput, DawnProject, Display, FixturePlacement, Layout,
    Patch, Protocol, Resolved, ResolvedInlineOrRef, RgbChannelOrder,
};

use crate::document::geometry_render_plan;
use crate::output_runtime::{OutputGeometryModel, RenderedOutputFrame};

pub const DMX_UNIVERSE_CHANNELS: usize = 512;
const RGB_CHANNELS_PER_PIXEL: usize = 3;
const E131_SOURCE_NAME: &str = "Dawn";
const E131_CID: [u8; 16] = *b"DawnLiveOutput01";
const ACN_PACKET_IDENTIFIER: &[u8; 12] = b"ASC-E1.17\0\0\0";

#[derive(Debug, Clone)]
pub struct ControllerOutputPlan {
    universes: Vec<ControllerUniversePlan>,
}

impl ControllerOutputPlan {
    pub fn universes(&self) -> &[ControllerUniversePlan] {
        &self.universes
    }

    pub fn active_universe_count(&self) -> usize {
        self.universes.len()
    }

    pub fn frame_buffers(&self, frame: &RenderedOutputFrame) -> Vec<ControllerUniverseFrame> {
        let mut outputs = self.blackout_buffers();
        for (universe_plan, output) in self.universes.iter().zip(outputs.iter_mut()) {
            for route in &universe_plan.routes {
                for offset in 0..route.channel_count {
                    let source_channel = route.fixture_channel_offset + offset;
                    let output_channel = route.start_channel + offset;
                    let Some(value) = ordered_frame_channel(
                        &frame.rgb,
                        route.rgb_offset,
                        source_channel,
                        route.channel_order,
                    ) else {
                        continue;
                    };
                    output.data[output_channel] = value;
                }
            }
        }
        outputs
    }

    pub fn frame_channel_bytes(&self, frame: &RenderedOutputFrame) -> Vec<u8> {
        self.frame_buffers(frame)
            .into_iter()
            .flat_map(|buffer| buffer.data)
            .collect()
    }

    pub fn channel_count(&self) -> usize {
        self.universes
            .iter()
            .map(|universe| universe.channel_count)
            .sum()
    }

    pub fn blackout_buffers(&self) -> Vec<ControllerUniverseFrame> {
        self.universes
            .iter()
            .map(|universe| ControllerUniverseFrame {
                destination: universe.destination,
                universe: universe.universe,
                data: vec![0; universe.channel_count],
            })
            .collect()
    }
}

#[derive(Debug, Clone)]
pub struct ControllerUniversePlan {
    pub controller_name: String,
    pub destination: Option<SocketAddr>,
    pub universe: u16,
    channel_count: usize,
    routes: Vec<ControllerRoutePlan>,
}

#[derive(Debug, Clone)]
struct ControllerRoutePlan {
    rgb_offset: usize,
    channel_order: RgbChannelOrder,
    fixture_channel_offset: usize,
    start_channel: usize,
    channel_count: usize,
}

#[derive(Debug, Clone)]
pub struct ControllerUniverseFrame {
    pub destination: Option<SocketAddr>,
    pub universe: u16,
    pub data: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ControllerOutputError {
    ProjectNotResolved,
    UnsupportedProtocol {
        controller: String,
        protocol: Protocol,
    },
    MissingDestination {
        controller: String,
    },
    MissingUniverse {
        controller: String,
        universe: u32,
    },
    InvalidUniverseId {
        controller: String,
        universe: u32,
    },
    InvalidUniverseRange {
        controller: String,
        universe: u32,
        start: u16,
        end: u16,
    },
    UnsupportedColorModel {
        fixture: String,
        color_model: ColorModel,
    },
    RouteOutsideUniverseRange {
        fixture: String,
        controller: String,
        universe: u32,
        start: u32,
        end: u32,
        range_start: u16,
        range_end: u16,
    },
    RouteOverlap {
        fixture: String,
        controller: String,
        universe: u32,
        channel: u32,
    },
    RouteTargetsLinearController {
        fixture: String,
        controller: String,
    },
    MissingGroup {
        controller: String,
        group: String,
    },
    LinearGroupCountMismatch {
        controller: String,
        group: String,
        expected: usize,
        actual: usize,
    },
    LinearFixturePixelCountMismatch {
        fixture: String,
        expected: usize,
        actual: usize,
    },
    InvalidLinearOutputConfig {
        controller: String,
        message: &'static str,
    },
}

impl fmt::Display for ControllerOutputError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ProjectNotResolved => write!(formatter, "project must load before live output is available"),
            Self::UnsupportedProtocol { controller, protocol } => write!(
                formatter,
                "controller `{controller}` uses unsupported live-output protocol `{protocol:?}`"
            ),
            Self::MissingDestination { controller } => {
                write!(formatter, "controller `{controller}` is missing a destination")
            }
            Self::MissingUniverse { controller, universe } => write!(
                formatter,
                "controller `{controller}` does not declare universe `{universe}`"
            ),
            Self::InvalidUniverseId { controller, universe } => write!(
                formatter,
                "controller `{controller}` universe `{universe}` is outside the E1.31 universe range"
            ),
            Self::InvalidUniverseRange { controller, universe, start, end } => write!(
                formatter,
                "controller `{controller}` universe `{universe}` range `{start}..{end}` is outside DMX channels 1..512"
            ),
            Self::UnsupportedColorModel { fixture, color_model } => write!(
                formatter,
                "fixture `{fixture}` uses unsupported live-output color model `{color_model:?}`"
            ),
            Self::RouteOutsideUniverseRange {
                fixture,
                controller,
                universe,
                start,
                end,
                range_start,
                range_end,
            } => write!(
                formatter,
                "fixture `{fixture}` route on `{controller}` universe `{universe}` uses channels `{start}..{end}`, outside declared range `{range_start}..{range_end}`"
            ),
            Self::RouteOverlap { fixture, controller, universe, channel } => write!(
                formatter,
                "fixture `{fixture}` overlaps another route on `{controller}` universe `{universe}` at channel `{channel}`"
            ),
            Self::RouteTargetsLinearController { fixture, controller } => write!(
                formatter,
                "fixture `{fixture}` route targets linear RGB controller `{controller}`"
            ),
            Self::MissingGroup { controller, group } => {
                write!(formatter, "controller `{controller}` references unknown group `{group}`")
            }
            Self::LinearGroupCountMismatch { controller, group, expected, actual } => write!(
                formatter,
                "controller `{controller}` linear RGB group `{group}` has {actual} fixtures, expected {expected}"
            ),
            Self::LinearFixturePixelCountMismatch { fixture, expected, actual } => write!(
                formatter,
                "fixture `{fixture}` has {actual} pixels, expected {expected} for linear RGB output"
            ),
            Self::InvalidLinearOutputConfig { controller, message } => write!(
                formatter,
                "controller `{controller}` has invalid linear RGB output config: {message}"
            ),
        }
    }
}

impl std::error::Error for ControllerOutputError {}

pub fn build_output_plan(
    project: &DawnProject,
    geometry: &OutputGeometryModel,
) -> Result<ControllerOutputPlan, ControllerOutputError> {
    build_output_plan_for(project, geometry, ControllerOutputPurpose::Live)
}

pub fn build_fseq_output_plan(
    project: &DawnProject,
    geometry: &OutputGeometryModel,
) -> Result<ControllerOutputPlan, ControllerOutputError> {
    build_output_plan_for(project, geometry, ControllerOutputPurpose::Fseq)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ControllerOutputPurpose {
    Live,
    Fseq,
}

fn build_output_plan_for(
    project: &DawnProject,
    geometry: &OutputGeometryModel,
    purpose: ControllerOutputPurpose,
) -> Result<ControllerOutputPlan, ControllerOutputError> {
    let display = active_display(project)?;
    let layout = resolved_layout(project, &display.layout)?;
    let patch = resolved_patch(project, &display.patch)?;
    let mut universes = HashMap::new();
    let mut occupancy = HashMap::new();

    for controller_ref in &display.controllers {
        let (controller_name, controller) = resolved_controller(project, controller_ref)?;
        if let ControllerOutput::LinearRgb { .. } = &controller.output {
            add_linear_rgb_controller(
                project,
                layout,
                controller_name,
                controller,
                &mut universes,
                geometry,
                purpose,
            )?;
        }
    }

    for route in &patch.routes {
        let controller_name = route.controller.key.name.as_str();
        let controller = project
            .stores
            .controllers
            .get(&route.controller.key)
            .map(|controller| &controller.value)
            .ok_or(ControllerOutputError::ProjectNotResolved)?;
        let fixture_index = layout
            .fixtures
            .iter()
            .position(|fixture| fixture.id == route.fixture)
            .ok_or(ControllerOutputError::ProjectNotResolved)?;
        let fixture = &layout.fixtures[fixture_index];
        let ControllerOutput::PatchedDmx {
            channel_order,
            universes: declared_universes,
        } = &controller.output
        else {
            return Err(ControllerOutputError::RouteTargetsLinearController {
                fixture: fixture_name(fixture),
                controller: controller_name.to_string(),
            });
        };
        if purpose == ControllerOutputPurpose::Live {
            validate_live_controller(controller_name, controller)?;
        }
        let destination = controller
            .destination
            .map(|destination| destination.socket_addr());
        let universe_index =
            controller_universe_index(controller_name, declared_universes, route.universe)?;
        let fixture_definition = fixture_fixture(project, fixture)?;
        if fixture_definition.color_model != ColorModel::Rgb {
            return Err(ControllerOutputError::UnsupportedColorModel {
                fixture: fixture_name(fixture),
                color_model: fixture_definition.color_model,
            });
        }
        let channel_count = fixture_pixel_count(project, fixture)? * RGB_CHANNELS_PER_PIXEL;
        add_route_segments(RouteSegmentInput {
            universes: &mut universes,
            occupancy: &mut occupancy,
            controller_key: controller_name.to_string(),
            controller_name,
            destination,
            declared_universes,
            start_universe_index: universe_index,
            start: route.start,
            rgb_offset: fixture_rgb_offset(geometry, fixture_index)?,
            fixture_name: &fixture_name(fixture),
            channel_order: *channel_order,
            channel_count,
        })?;
    }

    let mut universes = universes.into_values().collect::<Vec<_>>();
    universes.sort_by(|left, right| {
        left.controller_name
            .cmp(&right.controller_name)
            .then(left.universe.cmp(&right.universe))
    });
    Ok(ControllerOutputPlan {
        universes: universes
            .into_iter()
            .map(|universe| ControllerUniversePlan {
                controller_name: universe.controller_name,
                destination: universe.destination,
                universe: universe.universe,
                channel_count: universe.channel_count,
                routes: universe.routes,
            })
            .collect(),
    })
}

fn add_linear_rgb_controller(
    project: &DawnProject,
    layout: &Layout<Resolved>,
    controller_name: &str,
    controller: &Controller,
    universes: &mut HashMap<UniverseKey, ControllerUniverseBuilder>,
    geometry: &OutputGeometryModel,
    purpose: ControllerOutputPurpose,
) -> Result<(), ControllerOutputError> {
    let ControllerOutput::LinearRgb {
        channel_order,
        group,
        output_count,
        pixels_per_output,
        first_universe,
        slots_per_universe,
    } = &controller.output
    else {
        return Ok(());
    };
    validate_linear_config(
        controller_name,
        *output_count,
        *pixels_per_output,
        *slots_per_universe,
    )?;
    if purpose == ControllerOutputPurpose::Live {
        validate_live_controller(controller_name, controller)?;
    }
    let destination = controller
        .destination
        .map(|destination| destination.socket_addr());
    let group = layout
        .groups
        .iter()
        .find(|candidate| candidate.id == *group)
        .ok_or_else(|| ControllerOutputError::MissingGroup {
            controller: controller_name.to_string(),
            group: group.to_string(),
        })?;
    if group.members.len() != *output_count {
        return Err(ControllerOutputError::LinearGroupCountMismatch {
            controller: controller_name.to_string(),
            group: group.id.to_string(),
            expected: *output_count,
            actual: group.members.len(),
        });
    }
    let fixture_channels = pixels_per_output
        .checked_mul(RGB_CHANNELS_PER_PIXEL)
        .ok_or_else(|| ControllerOutputError::InvalidLinearOutputConfig {
            controller: controller_name.to_string(),
            message: "pixels_per_output is too large",
        })?;
    let total_slots = output_count.checked_mul(fixture_channels).ok_or_else(|| {
        ControllerOutputError::InvalidLinearOutputConfig {
            controller: controller_name.to_string(),
            message: "output_count is too large",
        }
    })?;
    let universe_count = total_slots.div_ceil(*slots_per_universe);
    let last_universe = first_universe
        .checked_add(universe_count.saturating_sub(1) as u32)
        .ok_or_else(|| ControllerOutputError::InvalidLinearOutputConfig {
            controller: controller_name.to_string(),
            message: "derived universe range is too large",
        })?;
    if *first_universe == 0 || last_universe > u16::MAX as u32 {
        return Err(ControllerOutputError::InvalidUniverseId {
            controller: controller_name.to_string(),
            universe: last_universe,
        });
    }

    for (output_index, fixture_id) in group.members.iter().copied().enumerate() {
        let fixture_index = layout
            .fixtures
            .iter()
            .position(|fixture| fixture.id == fixture_id)
            .ok_or(ControllerOutputError::ProjectNotResolved)?;
        let fixture = &layout.fixtures[fixture_index];
        let fixture_definition = fixture_fixture(project, fixture)?;
        if fixture_definition.color_model != ColorModel::Rgb {
            return Err(ControllerOutputError::UnsupportedColorModel {
                fixture: fixture_name(fixture),
                color_model: fixture_definition.color_model,
            });
        }
        let actual_pixels = fixture_pixel_count(project, fixture)?;
        if actual_pixels != *pixels_per_output {
            return Err(ControllerOutputError::LinearFixturePixelCountMismatch {
                fixture: fixture_name(fixture),
                expected: *pixels_per_output,
                actual: actual_pixels,
            });
        }

        let mut fixture_offset = 0usize;
        let mut stream_offset = output_index * fixture_channels;
        let mut remaining = fixture_channels;
        while remaining > 0 {
            let universe_offset = stream_offset / *slots_per_universe;
            let universe_start = universe_offset * *slots_per_universe;
            let universe_slots = (*slots_per_universe).min(total_slots - universe_start);
            let start_channel = stream_offset - universe_start;
            let segment_channels = remaining.min(universe_slots - start_channel);
            let universe = first_universe + universe_offset as u32;
            let key = UniverseKey {
                controller: controller_name.to_string(),
                universe,
            };
            universes
                .entry(key)
                .or_insert_with(|| ControllerUniverseBuilder {
                    controller_name: controller_name.to_string(),
                    destination,
                    universe: universe as u16,
                    channel_count: universe_slots,
                    routes: Vec::new(),
                })
                .routes
                .push(ControllerRoutePlan {
                    rgb_offset: fixture_rgb_offset(geometry, fixture_index)?,
                    channel_order: *channel_order,
                    fixture_channel_offset: fixture_offset,
                    start_channel,
                    channel_count: segment_channels,
                });

            fixture_offset += segment_channels;
            stream_offset += segment_channels;
            remaining -= segment_channels;
        }
    }
    Ok(())
}

fn validate_linear_config(
    controller_name: &str,
    output_count: usize,
    pixels_per_output: usize,
    slots_per_universe: usize,
) -> Result<(), ControllerOutputError> {
    if output_count == 0 {
        return Err(ControllerOutputError::InvalidLinearOutputConfig {
            controller: controller_name.to_string(),
            message: "output_count must be greater than zero",
        });
    }
    if pixels_per_output == 0 {
        return Err(ControllerOutputError::InvalidLinearOutputConfig {
            controller: controller_name.to_string(),
            message: "pixels_per_output must be greater than zero",
        });
    }
    if slots_per_universe == 0 || slots_per_universe > DMX_UNIVERSE_CHANNELS {
        return Err(ControllerOutputError::InvalidLinearOutputConfig {
            controller: controller_name.to_string(),
            message: "slots_per_universe must be in 1..512",
        });
    }
    Ok(())
}

fn validate_live_controller(
    controller_name: &str,
    controller: &Controller,
) -> Result<(), ControllerOutputError> {
    if controller.protocol != Protocol::Sacn {
        return Err(ControllerOutputError::UnsupportedProtocol {
            controller: controller_name.to_string(),
            protocol: controller.protocol,
        });
    }
    if controller.destination.is_none() {
        return Err(ControllerOutputError::MissingDestination {
            controller: controller_name.to_string(),
        });
    }
    Ok(())
}

struct RouteSegmentInput<'a> {
    universes: &'a mut HashMap<UniverseKey, ControllerUniverseBuilder>,
    occupancy: &'a mut HashMap<UniverseKey, Vec<bool>>,
    controller_key: String,
    controller_name: &'a str,
    destination: Option<SocketAddr>,
    declared_universes: &'a [dawn_project::Universe],
    start_universe_index: usize,
    start: u32,
    rgb_offset: usize,
    fixture_name: &'a str,
    channel_order: RgbChannelOrder,
    channel_count: usize,
}

fn add_route_segments(input: RouteSegmentInput<'_>) -> Result<(), ControllerOutputError> {
    let mut universe_index = input.start_universe_index;
    let mut channel = input.start as usize;
    let mut fixture_channel_offset = 0usize;
    let mut remaining = input.channel_count;
    while remaining > 0 {
        let Some(universe) = input.declared_universes.get(universe_index) else {
            let last_universe = input
                .declared_universes
                .last()
                .map(|universe| universe.id)
                .unwrap_or(0);
            return Err(ControllerOutputError::RouteOutsideUniverseRange {
                fixture: input.fixture_name.to_string(),
                controller: input.controller_name.to_string(),
                universe: last_universe,
                start: input.start,
                end: (input.start as usize + input.channel_count - 1).min(u32::MAX as usize) as u32,
                range_start: 1,
                range_end: 0,
            });
        };
        validate_universe(input.controller_name, universe)?;
        if channel < usize::from(universe.range.start) || channel > usize::from(universe.range.end)
        {
            return Err(ControllerOutputError::RouteOutsideUniverseRange {
                fixture: input.fixture_name.to_string(),
                controller: input.controller_name.to_string(),
                universe: universe.id,
                start: input.start,
                end: (input.start as usize + input.channel_count - 1).min(u32::MAX as usize) as u32,
                range_start: universe.range.start,
                range_end: universe.range.end,
            });
        }
        let available = usize::from(universe.range.end) - channel + 1;
        let segment_channels = remaining.min(available);
        let key = UniverseKey {
            controller: input.controller_key.clone(),
            universe: universe.id,
        };
        let channel_count = universe_channel_count(universe);
        let used = input
            .occupancy
            .entry(key.clone())
            .or_insert_with(|| vec![false; channel_count]);
        let start_channel = channel - usize::from(universe.range.start);
        for offset in 0..segment_channels {
            if used[start_channel + offset] {
                return Err(ControllerOutputError::RouteOverlap {
                    fixture: input.fixture_name.to_string(),
                    controller: input.controller_name.to_string(),
                    universe: universe.id,
                    channel: (channel + offset).min(u32::MAX as usize) as u32,
                });
            }
            used[start_channel + offset] = true;
        }

        input
            .universes
            .entry(key)
            .or_insert_with(|| ControllerUniverseBuilder {
                controller_name: input.controller_name.to_string(),
                destination: input.destination,
                universe: universe.id as u16,
                channel_count,
                routes: Vec::new(),
            })
            .routes
            .push(ControllerRoutePlan {
                rgb_offset: input.rgb_offset,
                channel_order: input.channel_order,
                fixture_channel_offset,
                start_channel,
                channel_count: segment_channels,
            });

        remaining -= segment_channels;
        fixture_channel_offset += segment_channels;
        universe_index += 1;
        channel = input
            .declared_universes
            .get(universe_index)
            .map(|universe| usize::from(universe.range.start))
            .unwrap_or(1);
    }
    Ok(())
}

fn controller_universe_index(
    controller_name: &str,
    universes: &[dawn_project::Universe],
    universe_id: u32,
) -> Result<usize, ControllerOutputError> {
    for (index, universe) in universes.iter().enumerate() {
        if universe.id == universe_id {
            validate_universe(controller_name, universe)?;
            return Ok(index);
        }
    }
    Err(ControllerOutputError::MissingUniverse {
        controller: controller_name.to_string(),
        universe: universe_id,
    })
}

fn validate_universe(
    controller_name: &str,
    universe: &dawn_project::Universe,
) -> Result<(), ControllerOutputError> {
    if universe.id == 0 || universe.id > u16::MAX as u32 {
        return Err(ControllerOutputError::InvalidUniverseId {
            controller: controller_name.to_string(),
            universe: universe.id,
        });
    }
    if universe.range.start == 0
        || universe.range.end == 0
        || universe.range.start > universe.range.end
        || usize::from(universe.range.end) > DMX_UNIVERSE_CHANNELS
    {
        return Err(ControllerOutputError::InvalidUniverseRange {
            controller: controller_name.to_string(),
            universe: universe.id,
            start: universe.range.start,
            end: universe.range.end,
        });
    }
    Ok(())
}

fn universe_channel_count(universe: &dawn_project::Universe) -> usize {
    usize::from(universe.range.end - universe.range.start + 1)
}

fn active_display(project: &DawnProject) -> Result<&Display<Resolved>, ControllerOutputError> {
    match &project.display {
        ResolvedInlineOrRef::Inline(display) => Ok(display),
        ResolvedInlineOrRef::Ref(reference) => project
            .stores
            .displays
            .get(&reference.key)
            .map(|display| &display.value)
            .ok_or(ControllerOutputError::ProjectNotResolved),
    }
}

fn resolved_layout<'a>(
    project: &'a DawnProject,
    layout: &'a ResolvedInlineOrRef<Layout<Resolved>, dawn_project::LayoutDefinitionKey>,
) -> Result<&'a Layout<Resolved>, ControllerOutputError> {
    match layout {
        ResolvedInlineOrRef::Inline(layout) => Ok(layout),
        ResolvedInlineOrRef::Ref(reference) => project
            .stores
            .layouts
            .get(&reference.key)
            .map(|layout| &layout.value)
            .ok_or(ControllerOutputError::ProjectNotResolved),
    }
}

fn resolved_patch<'a>(
    project: &'a DawnProject,
    patch: &'a ResolvedInlineOrRef<Patch<Resolved>, dawn_project::PatchDefinitionKey>,
) -> Result<&'a Patch<Resolved>, ControllerOutputError> {
    match patch {
        ResolvedInlineOrRef::Inline(patch) => Ok(patch),
        ResolvedInlineOrRef::Ref(reference) => project
            .stores
            .patches
            .get(&reference.key)
            .map(|patch| &patch.value)
            .ok_or(ControllerOutputError::ProjectNotResolved),
    }
}

fn resolved_controller<'a>(
    project: &'a DawnProject,
    controller: &'a ResolvedInlineOrRef<Controller, dawn_project::ControllerDefinitionKey>,
) -> Result<(&'a str, &'a Controller), ControllerOutputError> {
    match controller {
        ResolvedInlineOrRef::Inline(controller) => Ok(("inline", controller)),
        ResolvedInlineOrRef::Ref(reference) => project
            .stores
            .controllers
            .get(&reference.key)
            .map(|controller| (reference.key.name.as_str(), &controller.value))
            .ok_or(ControllerOutputError::ProjectNotResolved),
    }
}

fn fixture_name(fixture: &FixturePlacement<Resolved>) -> String {
    fixture
        .name
        .clone()
        .unwrap_or_else(|| fixture.id.to_string())
}

fn fixture_fixture<'a>(
    project: &'a DawnProject,
    fixture: &'a FixturePlacement<Resolved>,
) -> Result<&'a dawn_project::Fixture, ControllerOutputError> {
    match &fixture.fixture {
        ResolvedInlineOrRef::Inline(fixture) => Ok(fixture),
        ResolvedInlineOrRef::Ref(reference) => project
            .stores
            .fixture_definitions
            .get(&reference.key)
            .map(|fixture| &fixture.value)
            .ok_or(ControllerOutputError::ProjectNotResolved),
    }
}

fn fixture_pixel_count(
    project: &DawnProject,
    fixture: &FixturePlacement<Resolved>,
) -> Result<usize, ControllerOutputError> {
    Ok(geometry_render_plan(fixture_fixture(project, fixture)?)
        .emitters
        .len())
}

fn fixture_rgb_offset(
    geometry: &OutputGeometryModel,
    fixture_index: usize,
) -> Result<usize, ControllerOutputError> {
    let pixel_offset = geometry
        .fixtures
        .iter()
        .take(fixture_index)
        .map(|fixture| fixture.pixels.len())
        .sum::<usize>();
    pixel_offset
        .checked_mul(RGB_CHANNELS_PER_PIXEL)
        .ok_or(ControllerOutputError::ProjectNotResolved)
}

fn ordered_frame_channel(
    rgb: &[u8],
    fixture_rgb_offset: usize,
    fixture_channel_offset: usize,
    channel_order: RgbChannelOrder,
) -> Option<u8> {
    let pixel_offset = fixture_channel_offset / RGB_CHANNELS_PER_PIXEL;
    let channel = fixture_channel_offset % RGB_CHANNELS_PER_PIXEL;
    let source_pixel_offset = fixture_rgb_offset.checked_add(pixel_offset.checked_mul(3)?)?;
    let ordered_channel = match channel_order {
        RgbChannelOrder::Rgb => channel,
        RgbChannelOrder::Rbg => [0, 2, 1][channel],
        RgbChannelOrder::Grb => [1, 0, 2][channel],
        RgbChannelOrder::Gbr => [1, 2, 0][channel],
        RgbChannelOrder::Brg => [2, 0, 1][channel],
        RgbChannelOrder::Bgr => [2, 1, 0][channel],
    };
    rgb.get(source_pixel_offset + ordered_channel).copied()
}

pub fn encode_e131_data_packet(frame: &ControllerUniverseFrame, sequence: u8) -> Vec<u8> {
    let property_value_count = frame.data.len() + 1;
    let packet_len = 126 + frame.data.len();
    let mut packet = vec![0; packet_len];
    packet[0..2].copy_from_slice(&0x0010u16.to_be_bytes());
    packet[2..4].copy_from_slice(&0u16.to_be_bytes());
    packet[4..16].copy_from_slice(ACN_PACKET_IDENTIFIER);
    write_flags_and_length(&mut packet, 16, 109 + property_value_count);
    packet[18..22].copy_from_slice(&0x0000_0004u32.to_be_bytes());
    packet[22..38].copy_from_slice(&E131_CID);

    write_flags_and_length(&mut packet, 38, 87 + property_value_count);
    packet[40..44].copy_from_slice(&0x0000_0002u32.to_be_bytes());
    write_source_name(&mut packet[44..108]);
    packet[108] = 100;
    packet[109..111].copy_from_slice(&0u16.to_be_bytes());
    packet[111] = sequence;
    packet[112] = 0;
    packet[113..115].copy_from_slice(&frame.universe.to_be_bytes());

    write_flags_and_length(&mut packet, 115, 10 + property_value_count);
    packet[117] = 0x02;
    packet[118] = 0xa1;
    packet[119..121].copy_from_slice(&0u16.to_be_bytes());
    packet[121..123].copy_from_slice(&1u16.to_be_bytes());
    packet[123..125].copy_from_slice(&(property_value_count as u16).to_be_bytes());
    packet[125] = 0;
    packet[126..].copy_from_slice(&frame.data);
    packet
}

fn write_flags_and_length(packet: &mut [u8], offset: usize, length: usize) {
    let value = 0x7000u16 | (length as u16 & 0x0fff);
    packet[offset..offset + 2].copy_from_slice(&value.to_be_bytes());
}

fn write_source_name(target: &mut [u8]) {
    let bytes = E131_SOURCE_NAME.as_bytes();
    let count = bytes.len().min(target.len());
    target[..count].copy_from_slice(&bytes[..count]);
}

#[derive(Debug, Clone)]
struct ControllerUniverseBuilder {
    controller_name: String,
    destination: Option<SocketAddr>,
    universe: u16,
    channel_count: usize,
    routes: Vec<ControllerRoutePlan>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct UniverseKey {
    controller: String,
    universe: u32,
}
