use std::collections::HashMap;
use std::fmt;
use std::net::SocketAddr;

use dawn_project::analysis::ProjectAnalysis;
use dawn_project::model::{
    ColorModel, Controller, ControllerIndex, ControllerOutput, FixtureIndex, Protocol,
};

use crate::output_runtime::OutputFrame;

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

    pub fn frame_buffers(&self, frame: &OutputFrame) -> Vec<ControllerUniverseFrame> {
        let mut outputs = self.blackout_buffers();
        for (universe_plan, output) in self.universes.iter().zip(outputs.iter_mut()) {
            for route in &universe_plan.routes {
                let Some(fixture) = frame.fixtures.get(route.fixture_index.0) else {
                    continue;
                };
                let bytes = fixture_rgb_bytes(fixture);
                for offset in 0..route.channel_count {
                    let source_channel = route.fixture_channel_offset + offset;
                    let output_channel = route.start_channel + offset;
                    let Some(value) = bytes.get(source_channel) else {
                        continue;
                    };
                    output.data[output_channel] = *value;
                }
            }
        }
        outputs
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
    pub destination: SocketAddr,
    pub universe: u16,
    channel_count: usize,
    routes: Vec<ControllerRoutePlan>,
}

#[derive(Debug, Clone)]
struct ControllerRoutePlan {
    fixture_index: FixtureIndex,
    fixture_channel_offset: usize,
    start_channel: usize,
    channel_count: usize,
}

#[derive(Debug, Clone)]
pub struct ControllerUniverseFrame {
    pub destination: SocketAddr,
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
            Self::ProjectNotResolved => write!(formatter, "project must resolve before live output is available"),
            Self::UnsupportedProtocol {
                controller,
                protocol,
            } => write!(
                formatter,
                "controller `{controller}` uses unsupported live-output protocol `{protocol:?}`"
            ),
            Self::MissingDestination { controller } => {
                write!(formatter, "controller `{controller}` is missing a destination")
            }
            Self::MissingUniverse {
                controller,
                universe,
            } => write!(
                formatter,
                "controller `{controller}` does not declare universe `{universe}`"
            ),
            Self::InvalidUniverseId {
                controller,
                universe,
            } => write!(
                formatter,
                "controller `{controller}` universe `{universe}` is outside the E1.31 universe range"
            ),
            Self::InvalidUniverseRange {
                controller,
                universe,
                start,
                end,
            } => write!(
                formatter,
                "controller `{controller}` universe `{universe}` range `{start}..{end}` is outside DMX channels 1..512"
            ),
            Self::UnsupportedColorModel {
                fixture,
                color_model,
            } => write!(
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
            Self::RouteOverlap {
                fixture,
                controller,
                universe,
                channel,
            } => write!(
                formatter,
                "fixture `{fixture}` overlaps another route on `{controller}` universe `{universe}` at channel `{channel}`"
            ),
            Self::RouteTargetsLinearController {
                fixture,
                controller,
            } => write!(
                formatter,
                "fixture `{fixture}` route targets linear RGB controller `{controller}`"
            ),
            Self::MissingGroup { controller, group } => {
                write!(formatter, "controller `{controller}` references unknown group `{group}`")
            }
            Self::LinearGroupCountMismatch {
                controller,
                group,
                expected,
                actual,
            } => write!(
                formatter,
                "controller `{controller}` linear RGB group `{group}` has {actual} fixtures, expected {expected}"
            ),
            Self::LinearFixturePixelCountMismatch {
                fixture,
                expected,
                actual,
            } => write!(
                formatter,
                "fixture `{fixture}` has {actual} pixels, expected {expected} for linear RGB output"
            ),
            Self::InvalidLinearOutputConfig {
                controller,
                message,
            } => write!(
                formatter,
                "controller `{controller}` has invalid linear RGB output config: {message}"
            ),
        }
    }
}

impl std::error::Error for ControllerOutputError {}

pub fn build_output_plan(
    analysis: &ProjectAnalysis,
) -> Result<ControllerOutputPlan, ControllerOutputError> {
    let project = analysis
        .resolved
        .as_ref()
        .ok_or(ControllerOutputError::ProjectNotResolved)?;
    let mut universes: HashMap<UniverseKey, ControllerUniverseBuilder> = HashMap::new();
    let mut occupancy: HashMap<UniverseKey, Vec<bool>> = HashMap::new();

    for (controller_index, controller) in project.display.controllers.iter().enumerate() {
        if let ControllerOutput::LinearRgb { .. } = &controller.output {
            add_linear_rgb_controller(
                analysis,
                ControllerIndex(controller_index),
                controller,
                &mut universes,
            )?;
        }
    }

    for route in &project.display.patch.routes {
        let controller = project
            .display
            .controller(route.controller)
            .ok_or(ControllerOutputError::ProjectNotResolved)?;
        let fixture = project
            .display
            .layout
            .fixture(route.fixture)
            .ok_or(ControllerOutputError::ProjectNotResolved)?;
        let ControllerOutput::PatchedDmx {
            universes: declared_universes,
        } = &controller.output
        else {
            return Err(ControllerOutputError::RouteTargetsLinearController {
                fixture: fixture.name.clone(),
                controller: controller.name.clone(),
            });
        };
        let destination = controller_destination(controller)?;
        let universe_index =
            controller_universe_index(&controller.name, declared_universes, route.universe)?;

        if fixture.fixture.color_model != ColorModel::Rgb {
            return Err(ControllerOutputError::UnsupportedColorModel {
                fixture: fixture.name.clone(),
                color_model: fixture.fixture.color_model,
            });
        }
        let pixel_count = fixture_pixel_count(route.fixture, analysis)?;
        let channel_count = pixel_count * RGB_CHANNELS_PER_PIXEL;
        add_route_segments(RouteSegmentInput {
            universes: &mut universes,
            occupancy: &mut occupancy,
            controller_index: route.controller,
            controller_name: &controller.name,
            destination: destination.socket_addr(),
            declared_universes,
            start_universe_index: universe_index,
            start: route.start,
            fixture_index: route.fixture,
            fixture_name: &fixture.name,
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
    analysis: &ProjectAnalysis,
    controller_index: ControllerIndex,
    controller: &Controller,
    universes: &mut HashMap<UniverseKey, ControllerUniverseBuilder>,
) -> Result<(), ControllerOutputError> {
    let project = analysis
        .resolved
        .as_ref()
        .ok_or(ControllerOutputError::ProjectNotResolved)?;
    let ControllerOutput::LinearRgb {
        group,
        output_count,
        pixels_per_output,
        first_universe,
        slots_per_universe,
    } = &controller.output
    else {
        return Ok(());
    };
    if *output_count == 0 {
        return Err(ControllerOutputError::InvalidLinearOutputConfig {
            controller: controller.name.clone(),
            message: "output_count must be greater than zero",
        });
    }
    if *pixels_per_output == 0 {
        return Err(ControllerOutputError::InvalidLinearOutputConfig {
            controller: controller.name.clone(),
            message: "pixels_per_output must be greater than zero",
        });
    }
    if *slots_per_universe == 0 || *slots_per_universe > DMX_UNIVERSE_CHANNELS {
        return Err(ControllerOutputError::InvalidLinearOutputConfig {
            controller: controller.name.clone(),
            message: "slots_per_universe must be in 1..512",
        });
    }
    let destination = controller_destination(controller)?;
    let group = project
        .display
        .layout
        .groups
        .iter()
        .find(|candidate| candidate.name == *group)
        .ok_or_else(|| ControllerOutputError::MissingGroup {
            controller: controller.name.clone(),
            group: group.clone(),
        })?;
    if group.members.len() != *output_count {
        return Err(ControllerOutputError::LinearGroupCountMismatch {
            controller: controller.name.clone(),
            group: group.name.clone(),
            expected: *output_count,
            actual: group.members.len(),
        });
    }
    let fixture_channels = pixels_per_output
        .checked_mul(RGB_CHANNELS_PER_PIXEL)
        .ok_or_else(|| ControllerOutputError::InvalidLinearOutputConfig {
            controller: controller.name.clone(),
            message: "pixels_per_output is too large",
        })?;
    let total_slots = output_count.checked_mul(fixture_channels).ok_or_else(|| {
        ControllerOutputError::InvalidLinearOutputConfig {
            controller: controller.name.clone(),
            message: "output_count is too large",
        }
    })?;
    let universe_count = total_slots.div_ceil(*slots_per_universe);
    let last_universe = first_universe
        .checked_add(universe_count.saturating_sub(1) as u32)
        .ok_or_else(|| ControllerOutputError::InvalidLinearOutputConfig {
            controller: controller.name.clone(),
            message: "derived universe range is too large",
        })?;
    if *first_universe == 0 || last_universe > u16::MAX as u32 {
        return Err(ControllerOutputError::InvalidUniverseId {
            controller: controller.name.clone(),
            universe: last_universe,
        });
    }

    for (output_index, fixture_index) in group.members.iter().copied().enumerate() {
        let fixture = project
            .display
            .layout
            .fixture(fixture_index)
            .ok_or(ControllerOutputError::ProjectNotResolved)?;
        if fixture.fixture.color_model != ColorModel::Rgb {
            return Err(ControllerOutputError::UnsupportedColorModel {
                fixture: fixture.name.clone(),
                color_model: fixture.fixture.color_model,
            });
        }
        let actual_pixels = fixture_pixel_count(fixture_index, analysis)?;
        if actual_pixels != *pixels_per_output {
            return Err(ControllerOutputError::LinearFixturePixelCountMismatch {
                fixture: fixture.name.clone(),
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
                controller: controller_index,
                universe,
            };
            universes
                .entry(key)
                .or_insert_with(|| ControllerUniverseBuilder {
                    controller_name: controller.name.clone(),
                    destination: destination.socket_addr(),
                    universe: universe as u16,
                    channel_count: universe_slots,
                    routes: Vec::new(),
                })
                .routes
                .push(ControllerRoutePlan {
                    fixture_index,
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

fn controller_destination(
    controller: &Controller,
) -> Result<dawn_project::model::ControllerDestination, ControllerOutputError> {
    if controller.protocol != Protocol::Sacn {
        return Err(ControllerOutputError::UnsupportedProtocol {
            controller: controller.name.clone(),
            protocol: controller.protocol,
        });
    }
    controller
        .destination
        .ok_or_else(|| ControllerOutputError::MissingDestination {
            controller: controller.name.clone(),
        })
}

struct RouteSegmentInput<'a> {
    universes: &'a mut HashMap<UniverseKey, ControllerUniverseBuilder>,
    occupancy: &'a mut HashMap<UniverseKey, Vec<bool>>,
    controller_index: ControllerIndex,
    controller_name: &'a str,
    destination: SocketAddr,
    declared_universes: &'a [dawn_project::model::Universe],
    start_universe_index: usize,
    start: u32,
    fixture_index: FixtureIndex,
    fixture_name: &'a str,
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
            controller: input.controller_index,
            universe: universe.id,
        };
        let channel_count = universe_channel_count(universe);
        let used = input
            .occupancy
            .entry(key)
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
                fixture_index: input.fixture_index,
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
    universes: &[dawn_project::model::Universe],
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
    universe: &dawn_project::model::Universe,
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

fn universe_channel_count(universe: &dawn_project::model::Universe) -> usize {
    usize::from(universe.range.end - universe.range.start + 1)
}

fn fixture_rgb_bytes(fixture: &crate::output_runtime::OutputFixtureFrame) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(fixture.pixels.len() * RGB_CHANNELS_PER_PIXEL);
    for pixel in &fixture.pixels {
        bytes.push(pixel.color.red);
        bytes.push(pixel.color.green);
        bytes.push(pixel.color.blue);
    }
    bytes
}

fn fixture_pixel_count(
    fixture_index: FixtureIndex,
    analysis: &ProjectAnalysis,
) -> Result<usize, ControllerOutputError> {
    let project = analysis
        .resolved
        .as_ref()
        .ok_or(ControllerOutputError::ProjectNotResolved)?;
    let fixture = project
        .display
        .layout
        .fixture(fixture_index)
        .ok_or(ControllerOutputError::ProjectNotResolved)?;
    Ok(dawn_project::render::geometry_render_plan(
        &fixture.fixture.geometry,
        fixture.fixture.bulb_diameter,
    )
    .emitters
    .len())
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
    destination: SocketAddr,
    universe: u16,
    channel_count: usize,
    routes: Vec<ControllerRoutePlan>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct UniverseKey {
    controller: ControllerIndex,
    universe: u32,
}
