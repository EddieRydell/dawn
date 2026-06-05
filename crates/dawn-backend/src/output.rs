use std::{
    collections::HashMap,
    fmt,
    fs::File,
    io::{BufWriter, Write},
    path::Path,
};

use dawn_language::{
    analysis::ProjectAnalysis,
    document::SequenceDocument,
    model::{
        Color, ColorModel, Controller, ControllerIndex, ControllerOutput, FixtureIndex,
        RgbChannelOrder,
    },
    sequence_render::{OutputFixtureFrame, OutputFrame, SequenceRenderCache},
};

use crate::{
    types::{FseqExportOptions, FseqExportReport},
    BackendError, BackendErrorKind, BackendResult,
};

pub const DMX_UNIVERSE_CHANNELS: usize = 512;
const RGB_CHANNELS_PER_PIXEL: usize = 3;
const FSEQ_IDENTIFIER: &[u8; 4] = b"PSEQ";
const FSEQ_STANDARD_HEADER_LENGTH: usize = 32;
const FSEQ_MAJOR_VERSION: u8 = 2;
const FSEQ_MINOR_VERSION: u8 = 0;
const FSEQ_UNCOMPRESSED: u8 = 0;
const DEFAULT_PRODUCER: &str = "Dawn";

#[derive(Debug, Default)]
pub(crate) struct Output;

pub(crate) fn export_fseq_file_with_cache(
    analysis: &ProjectAnalysis,
    document: &SequenceDocument,
    path: impl AsRef<Path>,
    options: FseqExportOptions,
    cache: &mut SequenceRenderCache,
) -> BackendResult<FseqExportReport> {
    let file = File::create(path.as_ref()).map_err(|error| {
        BackendError::new(
            BackendErrorKind::Io,
            format!(
                "failed to create FSEQ file '{}': {error}",
                path.as_ref().display()
            ),
        )
    })?;
    export_fseq_with_cache(analysis, document, file, options, cache).map_err(|error| {
        BackendError::new(
            BackendErrorKind::Io,
            format!("failed to export FSEQ '{}': {error}", document.object_key),
        )
    })
}

fn export_fseq_with_cache(
    analysis: &ProjectAnalysis,
    document: &SequenceDocument,
    writer: impl Write,
    options: FseqExportOptions,
    cache: &mut SequenceRenderCache,
) -> Result<FseqExportReport, FseqExportError> {
    validate_step_ms(options.step_ms)?;
    if !document.duration_seconds.is_finite() || document.duration_seconds < 0.0 {
        return Err(FseqExportError::InvalidDuration(document.duration_seconds));
    }

    let plan = build_fseq_output_plan(analysis)?;
    let channel_count = plan.channel_count();
    if channel_count == 0 {
        return Err(FseqExportError::NoOutputChannels);
    }
    let channel_count_u32 = u32::try_from(channel_count)
        .map_err(|_| FseqExportError::TooManyChannels(channel_count))?;
    let frame_count = fseq_frame_count(document.duration_seconds, options.step_ms)?;
    let frame_count_u32 =
        u32::try_from(frame_count).map_err(|_| FseqExportError::TooManyFrames(frame_count))?;
    let variable_headers = variable_headers(document, &options.metadata)?;
    let data_offset = FSEQ_STANDARD_HEADER_LENGTH + variable_headers.len();
    let data_offset_u16 =
        u16::try_from(data_offset).map_err(|_| FseqExportError::HeaderTooLarge(data_offset))?;
    let frame_data_bytes = u64::from(channel_count_u32)
        .checked_mul(u64::from(frame_count_u32))
        .ok_or(FseqExportError::FrameDataTooLarge)?;
    let bytes_written = u64::from(data_offset_u16)
        .checked_add(frame_data_bytes)
        .ok_or(FseqExportError::FrameDataTooLarge)?;

    let mut writer = BufWriter::new(writer);
    write_header(
        &mut writer,
        data_offset_u16,
        channel_count_u32,
        frame_count_u32,
        options.step_ms,
        &variable_headers,
    )?;
    write_frames(
        &mut writer,
        analysis,
        document,
        &plan,
        frame_count,
        options.step_ms,
        cache,
    )?;
    writer.flush()?;

    Ok(FseqExportReport {
        sequence: document.object_key.clone(),
        step_ms: options.step_ms,
        frame_count: frame_count_u32,
        channel_count: channel_count_u32,
        bytes_written,
    })
}

#[derive(Debug)]
enum FseqExportError {
    InvalidStepMs(u8),
    InvalidDuration(f64),
    Output(ControllerOutputError),
    NoOutputChannels,
    TooManyChannels(usize),
    TooManyFrames(u64),
    HeaderTooLarge(usize),
    FrameDataTooLarge,
    Evaluation(String),
    Io(std::io::Error),
}

impl fmt::Display for FseqExportError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidStepMs(step_ms) => {
                write!(formatter, "FSEQ step_ms must be in 1..255, got {step_ms}")
            }
            Self::InvalidDuration(duration) => write!(
                formatter,
                "sequence duration must be finite and non-negative, got {duration}"
            ),
            Self::Output(error) => write!(formatter, "{error}"),
            Self::NoOutputChannels => write!(formatter, "project display has zero output channels"),
            Self::TooManyChannels(channel_count) => write!(
                formatter,
                "FSEQ v2 channel count limit exceeded: {channel_count}"
            ),
            Self::TooManyFrames(frame_count) => {
                write!(
                    formatter,
                    "FSEQ v2 frame count limit exceeded: {frame_count}"
                )
            }
            Self::HeaderTooLarge(header_length) => {
                write!(
                    formatter,
                    "FSEQ header limit exceeded: {header_length} bytes"
                )
            }
            Self::FrameDataTooLarge => write!(formatter, "FSEQ frame data size is too large"),
            Self::Evaluation(error) => write!(formatter, "{error}"),
            Self::Io(error) => write!(formatter, "{error}"),
        }
    }
}

impl std::error::Error for FseqExportError {}

impl From<ControllerOutputError> for FseqExportError {
    fn from(error: ControllerOutputError) -> Self {
        Self::Output(error)
    }
}

impl From<std::io::Error> for FseqExportError {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error)
    }
}

fn validate_step_ms(step_ms: u8) -> Result<(), FseqExportError> {
    if step_ms == 0 {
        Err(FseqExportError::InvalidStepMs(step_ms))
    } else {
        Ok(())
    }
}

fn fseq_frame_count(duration_seconds: f64, step_ms: u8) -> Result<u64, FseqExportError> {
    let duration_ms = duration_seconds * 1000.0;
    let count = (duration_ms / f64::from(step_ms)).ceil();
    if count > f64::from(u32::MAX) {
        return Err(FseqExportError::TooManyFrames(count as u64));
    }
    Ok(count.max(0.0) as u64)
}

fn variable_headers(
    document: &SequenceDocument,
    metadata: &crate::types::FseqExportMetadata,
) -> Result<Vec<u8>, FseqExportError> {
    let mut headers = Vec::new();
    let media_filename = metadata.media_filename.as_deref().or_else(|| {
        document
            .audio
            .as_ref()
            .map(|audio| audio.file_name.as_str())
    });
    if let Some(media_filename) = media_filename {
        append_variable_header(&mut headers, *b"mf", media_filename)?;
    }
    let producer = metadata.producer.as_deref().unwrap_or(DEFAULT_PRODUCER);
    append_variable_header(&mut headers, *b"sp", producer)?;
    Ok(headers)
}

fn append_variable_header(
    headers: &mut Vec<u8>,
    code: [u8; 2],
    value: &str,
) -> Result<(), FseqExportError> {
    let length = 4usize
        .checked_add(value.len())
        .and_then(|length| length.checked_add(1))
        .ok_or(FseqExportError::HeaderTooLarge(usize::MAX))?;
    let length_u16 = u16::try_from(length).map_err(|_| FseqExportError::HeaderTooLarge(length))?;
    headers.extend(length_u16.to_le_bytes());
    headers.extend(code);
    headers.extend(value.as_bytes());
    headers.push(0);
    Ok(())
}

fn write_header(
    writer: &mut impl Write,
    data_offset: u16,
    channel_count: u32,
    frame_count: u32,
    step_ms: u8,
    variable_headers: &[u8],
) -> Result<(), FseqExportError> {
    writer.write_all(FSEQ_IDENTIFIER)?;
    writer.write_all(&data_offset.to_le_bytes())?;
    writer.write_all(&[FSEQ_MINOR_VERSION, FSEQ_MAJOR_VERSION])?;
    writer.write_all(&(FSEQ_STANDARD_HEADER_LENGTH as u16).to_le_bytes())?;
    writer.write_all(&channel_count.to_le_bytes())?;
    writer.write_all(&frame_count.to_le_bytes())?;
    writer.write_all(&[step_ms, 0])?;
    writer.write_all(&[FSEQ_UNCOMPRESSED, 0, 0, 0])?;
    writer.write_all(&0u64.to_le_bytes())?;
    writer.write_all(variable_headers)?;
    Ok(())
}

fn write_frames(
    writer: &mut impl Write,
    analysis: &ProjectAnalysis,
    document: &SequenceDocument,
    plan: &ControllerOutputPlan,
    frame_count: u64,
    step_ms: u8,
    cache: &mut SequenceRenderCache,
) -> Result<(), FseqExportError> {
    let (mut evaluator, _) = cache
        .build_evaluator(analysis, document)
        .map_err(FseqExportError::Evaluation)?;
    for frame_index in 0..frame_count {
        let time_seconds = frame_index as f64 * f64::from(step_ms) / 1000.0;
        let frame = evaluator.evaluate(time_seconds, frame_index);
        let bytes = plan.frame_channel_bytes(&frame);
        writer.write_all(&bytes)?;
    }
    Ok(())
}

#[derive(Debug, Clone)]
struct ControllerOutputPlan {
    universes: Vec<ControllerUniversePlan>,
}

impl ControllerOutputPlan {
    fn frame_channel_bytes(&self, frame: &OutputFrame) -> Vec<u8> {
        self.frame_buffers(frame)
            .into_iter()
            .flat_map(|buffer| buffer.data)
            .collect()
    }

    fn frame_buffers(&self, frame: &OutputFrame) -> Vec<ControllerUniverseFrame> {
        let mut outputs = self.blackout_buffers();
        for (universe_plan, output) in self.universes.iter().zip(outputs.iter_mut()) {
            for route in &universe_plan.routes {
                let Some(fixture) = frame.fixtures.get(route.fixture_index.0) else {
                    continue;
                };
                let bytes = fixture_rgb_bytes(fixture, route.channel_order);
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

    fn channel_count(&self) -> usize {
        self.universes
            .iter()
            .map(|universe| universe.channel_count)
            .sum()
    }

    fn blackout_buffers(&self) -> Vec<ControllerUniverseFrame> {
        self.universes
            .iter()
            .map(|universe| ControllerUniverseFrame {
                data: vec![0; universe.channel_count],
            })
            .collect()
    }
}

#[derive(Debug, Clone)]
struct ControllerUniversePlan {
    channel_count: usize,
    routes: Vec<ControllerRoutePlan>,
}

#[derive(Debug, Clone)]
struct ControllerRoutePlan {
    fixture_index: FixtureIndex,
    channel_order: RgbChannelOrder,
    fixture_channel_offset: usize,
    start_channel: usize,
    channel_count: usize,
}

#[derive(Debug, Clone)]
struct ControllerUniverseFrame {
    data: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum ControllerOutputError {
    ProjectNotResolved,
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
            Self::ProjectNotResolved => write!(formatter, "project must resolve before output is available"),
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
                "fixture `{fixture}` uses unsupported output color model `{color_model:?}`"
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

fn build_fseq_output_plan(
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
            channel_order,
            universes: declared_universes,
        } = &controller.output
        else {
            return Err(ControllerOutputError::RouteTargetsLinearController {
                fixture: fixture.name.clone(),
                controller: controller.name.clone(),
            });
        };
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
            declared_universes,
            start_universe_index: universe_index,
            start: route.start,
            fixture_index: route.fixture,
            fixture_name: &fixture.name,
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
                    universe: universe as u16,
                    channel_count: universe_slots,
                    routes: Vec::new(),
                })
                .routes
                .push(ControllerRoutePlan {
                    fixture_index,
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

struct RouteSegmentInput<'a> {
    universes: &'a mut HashMap<UniverseKey, ControllerUniverseBuilder>,
    occupancy: &'a mut HashMap<UniverseKey, Vec<bool>>,
    controller_index: ControllerIndex,
    controller_name: &'a str,
    declared_universes: &'a [dawn_language::model::Universe],
    start_universe_index: usize,
    start: u32,
    fixture_index: FixtureIndex,
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
                universe: universe.id as u16,
                channel_count,
                routes: Vec::new(),
            })
            .routes
            .push(ControllerRoutePlan {
                fixture_index: input.fixture_index,
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
    universes: &[dawn_language::model::Universe],
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
    universe: &dawn_language::model::Universe,
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

fn universe_channel_count(universe: &dawn_language::model::Universe) -> usize {
    usize::from(universe.range.end - universe.range.start + 1)
}

fn fixture_rgb_bytes(fixture: &OutputFixtureFrame, channel_order: RgbChannelOrder) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(fixture.pixels.len() * RGB_CHANNELS_PER_PIXEL);
    for pixel in &fixture.pixels {
        bytes.extend(ordered_rgb_bytes(pixel.color, channel_order));
    }
    bytes
}

fn ordered_rgb_bytes(color: Color, channel_order: RgbChannelOrder) -> [u8; RGB_CHANNELS_PER_PIXEL] {
    match channel_order {
        RgbChannelOrder::Rgb => [color.red, color.green, color.blue],
        RgbChannelOrder::Rbg => [color.red, color.blue, color.green],
        RgbChannelOrder::Grb => [color.green, color.red, color.blue],
        RgbChannelOrder::Gbr => [color.green, color.blue, color.red],
        RgbChannelOrder::Brg => [color.blue, color.red, color.green],
        RgbChannelOrder::Bgr => [color.blue, color.green, color.red],
    }
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
    Ok(dawn_language::render::geometry_render_plan(
        &fixture.fixture.geometry,
        fixture.fixture.bulb_diameter,
    )
    .emitters
    .len())
}

#[derive(Debug, Clone)]
struct ControllerUniverseBuilder {
    controller_name: String,
    universe: u16,
    channel_count: usize,
    routes: Vec<ControllerRoutePlan>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct UniverseKey {
    controller: ControllerIndex,
    universe: u32,
}
