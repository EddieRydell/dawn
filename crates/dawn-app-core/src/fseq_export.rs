use std::fmt;
use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::Path;

use crate::document::SequenceEditorDocument;
use dawn_project::DawnProject;

use crate::controller_output::{
    build_fseq_output_plan, ControllerOutputError, ControllerOutputPlan,
};
use crate::output_runtime::SequenceRenderPlan;

const FSEQ_IDENTIFIER: &[u8; 4] = b"PSEQ";
const FSEQ_STANDARD_HEADER_LENGTH: usize = 32;
const FSEQ_MAJOR_VERSION: u8 = 2;
const FSEQ_MINOR_VERSION: u8 = 0;
const FSEQ_UNCOMPRESSED: u8 = 0;
const DEFAULT_PRODUCER: &str = "Dawn";

#[derive(Debug, Clone)]
pub struct FseqExportOptions {
    pub step_ms: u8,
    pub metadata: FseqExportMetadata,
}

impl Default for FseqExportOptions {
    fn default() -> Self {
        Self {
            step_ms: 50,
            metadata: FseqExportMetadata::default(),
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct FseqExportMetadata {
    pub media_filename: Option<String>,
    pub producer: Option<String>,
}

#[derive(Debug, Clone)]
pub struct FseqExportReport {
    pub sequence: String,
    pub step_ms: u8,
    pub frame_count: u32,
    pub channel_count: u32,
    pub bytes_written: u64,
}

#[derive(Debug)]
pub enum FseqExportError {
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
            Self::InvalidDuration(duration) => {
                write!(
                    formatter,
                    "sequence duration must be finite and non-negative, got {duration}"
                )
            }
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

pub fn export_fseq_file(
    project: &DawnProject,
    document: &SequenceEditorDocument,
    path: impl AsRef<Path>,
    options: FseqExportOptions,
) -> Result<FseqExportReport, FseqExportError> {
    let file = File::create(path)?;
    export_fseq(project, document, file, options)
}

pub fn export_fseq(
    project: &DawnProject,
    document: &SequenceEditorDocument,
    writer: impl Write,
    options: FseqExportOptions,
) -> Result<FseqExportReport, FseqExportError> {
    validate_step_ms(options.step_ms)?;
    if !document.duration_seconds.is_finite() || document.duration_seconds < 0.0 {
        return Err(FseqExportError::InvalidDuration(document.duration_seconds));
    }

    let plan = build_fseq_output_plan(project)?;
    let channel_count = plan.channel_count();
    if channel_count == 0 {
        return Err(FseqExportError::NoOutputChannels);
    }
    let channel_count_u32 = u32::try_from(channel_count)
        .map_err(|_| FseqExportError::TooManyChannels(channel_count))?;
    let frame_count = frame_count(document.duration_seconds, options.step_ms)?;
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
        project,
        document,
        &plan,
        frame_count,
        options.step_ms,
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

fn validate_step_ms(step_ms: u8) -> Result<(), FseqExportError> {
    if step_ms == 0 {
        Err(FseqExportError::InvalidStepMs(step_ms))
    } else {
        Ok(())
    }
}

fn frame_count(duration_seconds: f64, step_ms: u8) -> Result<u64, FseqExportError> {
    let duration_ms = duration_seconds * 1000.0;
    let count = (duration_ms / f64::from(step_ms)).ceil();
    if count > f64::from(u32::MAX) {
        return Err(FseqExportError::TooManyFrames(count as u64));
    }
    Ok(count.max(0.0) as u64)
}

fn variable_headers(
    document: &SequenceEditorDocument,
    metadata: &FseqExportMetadata,
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
    project: &DawnProject,
    document: &SequenceEditorDocument,
    plan: &ControllerOutputPlan,
    frame_count: u64,
    step_ms: u8,
) -> Result<(), FseqExportError> {
    let mut evaluator =
        SequenceRenderPlan::new(project, document).map_err(FseqExportError::Evaluation)?;
    for frame_index in 0..frame_count {
        let time_seconds = frame_index as f64 * f64::from(step_ms) / 1000.0;
        let frame = evaluator.render_frame(time_seconds, frame_index);
        let bytes = plan.frame_channel_bytes(&frame);
        writer.write_all(&bytes)?;
    }
    Ok(())
}
