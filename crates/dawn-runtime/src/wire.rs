//! Portable prepared-sequence archives. The format uses 32-bit little-endian
//! fields; rkyv owns pointer relocation, sharing, and archive validation.

use crate::sequence::PreparedSequence;
use crate::values::{SampleDuration, SampleTime};
use alloc::vec::Vec;
use rkyv::rancor::Fallible;
use rkyv::with::{ArchiveWith, DeserializeWith, SerializeWith};
use rkyv::{Archive, Archived, Place};

pub const HEADER_BYTES: usize = 16;
const MAGIC: [u8; 4] = *b"DAWN";
const VERSION: u32 = 1;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LoadError {
    Header,
    Version,
    Checksum,
    Archive,
    Limit,
    InvalidSequence,
}

/// Admission limits for uploads from a trusted Dawn compiler. Workspace bytes
/// are a conservative admission estimate, not a fallible allocator or sandbox.
/// Decoding allocates owned data; callers must also reserve heap for that data,
/// archive validation, the upload buffer, and their other tasks.
#[derive(Clone, Copy, Debug)]
pub struct LoadLimits {
    pub payload_bytes: usize,
    pub pixels: usize,
    pub graph_nodes: usize,
    pub workspace_bytes: usize,
}

impl Default for LoadLimits {
    fn default() -> Self {
        Self {
            payload_bytes: 256 * 1024,
            pixels: 4096,
            graph_nodes: 256,
            workspace_bytes: 128 * 1024,
        }
    }
}

pub fn encode_sequence(sequence: &PreparedSequence) -> Result<Vec<u8>, LoadError> {
    let payload =
        rkyv::to_bytes::<rkyv::rancor::Failure>(sequence).map_err(|_| LoadError::Archive)?;
    let length = u32::try_from(payload.len()).map_err(|_| LoadError::Limit)?;
    let mut bytes = Vec::with_capacity(HEADER_BYTES + payload.len());
    bytes.extend_from_slice(&MAGIC);
    bytes.extend_from_slice(&VERSION.to_le_bytes());
    bytes.extend_from_slice(&length.to_le_bytes());
    bytes.extend_from_slice(&crc32fast::hash(&payload).to_le_bytes());
    bytes.extend_from_slice(&payload);
    Ok(bytes)
}

/// Reads only the fixed header, so transports can limit an upload before allocating it.
pub fn payload_length(header: &[u8], limits: LoadLimits) -> Result<usize, LoadError> {
    let header: &[u8; HEADER_BYTES] = header.try_into().map_err(|_| LoadError::Header)?;
    if header[..4] != MAGIC {
        return Err(LoadError::Header);
    }
    let word = |offset| {
        u32::from_le_bytes([
            header[offset],
            header[offset + 1],
            header[offset + 2],
            header[offset + 3],
        ])
    };
    if word(4) != VERSION {
        return Err(LoadError::Version);
    }
    let length = word(8) as usize;
    if length > limits.payload_bytes {
        return Err(LoadError::Limit);
    }
    Ok(length)
}

pub fn decode_sequence(bytes: &[u8], limits: LoadLimits) -> Result<PreparedSequence, LoadError> {
    use rkyv::validation::{Validator, archive::ArchiveValidator, shared::SharedValidator};
    let header = bytes.get(..HEADER_BYTES).ok_or(LoadError::Header)?;
    let length = payload_length(header, limits)?;
    let payload = bytes.get(HEADER_BYTES..).ok_or(LoadError::Header)?;
    if payload.len() != length {
        return Err(LoadError::Header);
    }
    if crc32fast::hash(payload).to_le_bytes() != header[12..16] {
        return Err(LoadError::Checksum);
    }
    let mut validator = Validator::new(
        ArchiveValidator::with_max_depth(payload, core::num::NonZeroUsize::new(64)),
        SharedValidator::new(),
    );
    let archived = rkyv::api::access_with_context::<
        Archived<PreparedSequence>,
        _,
        rkyv::rancor::Failure,
    >(payload, &mut validator)
    .map_err(|_| LoadError::Archive)?;
    if archived.signals.pixel_count.to_native() as usize > limits.pixels
        || archived.signals.plan.nodes.len() > limits.graph_nodes
        || archived.signals.plan.vm_workspace_count.to_native() as usize
            > archived.signals.plan.nodes.len()
        || archived.signals.plan.frame_buffer_count.to_native() as usize
            > archived.signals.plan.nodes.len()
    {
        return Err(LoadError::Limit);
    }
    drop(validator);
    let sequence = rkyv::deserialize::<PreparedSequence, rkyv::rancor::Failure>(archived)
        .map_err(|_| LoadError::Archive)?;
    validate_sequence(&sequence, limits)?;
    Ok(sequence)
}

fn validate_sequence(sequence: &PreparedSequence, limits: LoadLimits) -> Result<(), LoadError> {
    use crate::element::ElementLayout;
    use crate::patch::{PatchStep, PatchValueLayout};
    use crate::signal::{PreparedEffectImplementation, PreparedOperator, PreparedSignalKind};
    let bad = LoadError::InvalidSequence;
    let signal = &sequence.signals;
    let plan = &signal.plan;
    if signal.frame_rate == 0
        || plan.output_index >= plan.nodes.len()
        || plan.target as usize >= signal.targets.len()
        || signal.element_cell_offsets.len() != signal.elements.len()
        || signal.layers.len() != signal.effects_by_layer.len()
        || plan.frame_slots.len() != plan.nodes.len()
    {
        return Err(bad);
    }
    let mut workspace = 0usize;
    let mut reserve = |count: usize, width: usize| -> Result<(), LoadError> {
        workspace = workspace
            .checked_add(count.checked_mul(width).ok_or(LoadError::Limit)?)
            .ok_or(LoadError::Limit)?;
        if workspace > limits.workspace_bytes {
            return Err(LoadError::Limit);
        }
        Ok(())
    };
    reserve(signal.pixel_count, usize::from(plan.frame_buffer_count) * 3)?;
    // All VM slots reserve the component-wise largest layouts they can execute.
    // Budgeting that maximum for every slot also covers a program reused by
    // several operators, and array capacity/width maxima from different programs.
    let mut registers = [0usize; 5];
    let mut array_capacity = 0usize;
    let mut array_width = 0usize;
    for program in &signal.programs {
        if program.pixel_entry as usize > program.instructions.len() {
            return Err(bad);
        }
        let layout = program.layout;
        for (maximum, count) in registers.iter_mut().zip([
            layout.ints,
            layout.floats,
            layout.bools,
            layout.colors,
            layout.refs,
        ]) {
            *maximum = (*maximum).max(count as usize);
        }
        array_capacity = array_capacity.max(program.array_capacity as usize);
        array_width = array_width.max(program.array_width as usize);
    }
    for _ in 0..=plan.vm_workspace_count {
        reserve(1, 512)?;
        for count in registers {
            reserve(count, 64)?;
        }
        if array_capacity != 0 {
            reserve(1, 4096)?; // Offset allocator's fixed bin table and array metadata.
            reserve(array_capacity, 128)?; // Allocation nodes, free list and references.
            reserve(
                array_capacity,
                array_width.checked_mul(64).ok_or(LoadError::Limit)?,
            )?;
        }
    }
    reserve(plan.nodes.len(), 32)?;
    reserve(sequence.elements.len(), 64)?;
    reserve(sequence.patch.value_layouts.len(), 64)?;
    for control in &sequence.controls {
        reserve(control.explicit_fixture_count(), 16)?;
    }
    for &width in &sequence.output_widths {
        reserve(width as usize, 1)?;
    }
    let cells = |layout: ElementLayout| -> usize {
        match layout {
            ElementLayout::Color(n) | ElementLayout::Scalar(n) | ElementLayout::Indexed(n) => {
                n as usize
            }
            ElementLayout::Fixture(_) => 1,
        }
    };
    for &(_, layout) in &sequence.elements {
        reserve(cells(layout), 32)?;
        if let ElementLayout::Fixture(functions) = layout {
            reserve(functions as usize, 32)?;
        }
    }
    let mut count = 0usize;
    for (element, &offset) in signal.elements.iter().zip(&signal.element_cell_offsets) {
        if offset != count {
            return Err(bad);
        }
        count = count
            .checked_add(element.pixel_count)
            .ok_or(LoadError::Limit)?;
    }
    if count != signal.pixel_count {
        return Err(bad);
    }
    for target in &signal.targets {
        let pixels = signal
            .target_pixels
            .get(target.pixels.start as usize..target.pixels.end as usize)
            .ok_or(bad)?;
        let mut previous = None;
        for pixel in pixels {
            let element = signal
                .elements
                .get(pixel.element_index as usize)
                .ok_or(bad)?;
            let address = (pixel.element_index, pixel.element_cell_index);
            if pixel.element_cell_index as usize >= element.pixel_count
                || pixel.pixel_count == 0
                || pixel.pixel_index >= pixel.pixel_count
                || !pixel.pixel_fraction.is_finite()
                || previous.is_some_and(|old| old >= address)
            {
                return Err(bad);
            }
            previous = Some(address);
        }
        reserve(target.sample_count as usize, 8)?;
    }
    if signal.target(plan.target).len() != signal.pixel_count {
        return Err(bad);
    }
    let mut automation_slot = 0;
    for effect in &signal.effects {
        if effect.target as usize >= signal.targets.len()
            || effect
                .start_time
                .checked_add_duration(effect.duration)
                .is_none()
        {
            return Err(bad);
        }
        if let PreparedEffectImplementation::Dsl { program, .. } = effect.implementation
            && program as usize >= signal.programs.len()
        {
            return Err(bad);
        }
        if let Some(automation) = &effect.automation {
            reserve(1, 256)?;
            match &effect.implementation {
                PreparedEffectImplementation::Dsl { bound_params, .. }
                | PreparedEffectImplementation::Native {
                    params: Some((_, bound_params)),
                    ..
                } => {
                    reserve(
                        1,
                        bound_params
                            .automation_storage_estimate(&automation.bindings)
                            .ok_or(LoadError::Limit)?,
                    )?;
                }
                _ => {}
            }
            if automation.workspace_slot != automation_slot {
                return Err(bad);
            }
            automation_slot += 1;
        }
    }
    for layer in &signal.effects_by_layer {
        let mut previous = None;
        for &index in layer {
            let effect = signal.effects.get(index).ok_or(bad)?;
            if previous.is_some_and(|time| time > effect.start_time) {
                return Err(bad);
            }
            previous = Some(effect.start_time);
        }
    }
    let mut automation_slot = 0;
    let mut depths = Vec::with_capacity(plan.nodes.len());
    for (index, node) in plan.nodes.iter().enumerate() {
        let inputs = match &node.kind {
            PreparedSignalKind::Layer { layer_index } => {
                if *layer_index >= signal.layers.len() {
                    return Err(bad);
                }
                &[][..]
            }
            PreparedSignalKind::Operator {
                operator,
                inputs,
                automation,
                vm_slot,
            } => {
                if *vm_slot as usize >= plan.vm_workspace_count {
                    return Err(bad);
                }
                if let PreparedOperator::Dsl(program) = operator.implementation
                    && program as usize >= signal.programs.len()
                {
                    return Err(bad);
                }
                if !automation.is_empty() {
                    reserve(1, 256)?;
                    reserve(
                        1,
                        operator
                            .params
                            .automation_storage_estimate(automation)
                            .ok_or(LoadError::Limit)?,
                    )?;
                    if operator.automation_slot != automation_slot {
                        return Err(bad);
                    }
                    automation_slot += 1;
                }
                let required = match operator.implementation {
                    PreparedOperator::Native(
                        crate::BuiltinOperator::Max
                        | crate::BuiltinOperator::Add
                        | crate::BuiltinOperator::Multiply
                        | crate::BuiltinOperator::IntensityModulate,
                    ) => 2,
                    PreparedOperator::Native(_) => 1,
                    PreparedOperator::Dsl(_) => inputs.len(),
                };
                if inputs.len() != required {
                    return Err(bad);
                }
                &inputs[..]
            }
            PreparedSignalKind::Output { inputs } => &inputs[..],
        };
        if inputs.iter().any(|&input| input >= index) {
            return Err(bad);
        }
        let depth = inputs.iter().map(|&input| depths[input]).max().unwrap_or(0) + 1;
        if depth > 32 {
            return Err(LoadError::Limit);
        }
        depths.push(depth);
    }
    for &node in &plan.frame_nodes {
        if node >= plan.nodes.len() || plan.frame_slots[node] >= plan.frame_buffer_count {
            return Err(bad);
        }
    }
    if plan.frame_slots[plan.output_index] >= plan.frame_buffer_count {
        return Err(bad);
    }
    for &(element, ref span) in &sequence.color_spans {
        let &(_, layout) = sequence.elements.get(element as usize).ok_or(bad)?;
        if !matches!(layout, ElementLayout::Color(_) | ElementLayout::Fixture(_))
            || span.start > span.end
            || span.end as usize > signal.pixel_count
            || (span.end - span.start) as usize != cells(layout)
        {
            return Err(bad);
        }
    }
    for control in &sequence.controls {
        for address in &control.addresses {
            let &(_, layout) = sequence.elements.get(address.element as usize).ok_or(bad)?;
            if address.cell as usize >= cells(layout) {
                return Err(bad);
            }
        }
    }
    for (element, range) in &sequence.fixture_behaviors.bindings {
        if !matches!(
            sequence.elements.get(*element as usize),
            Some((_, ElementLayout::Fixture(_)))
        ) || sequence
            .fixture_behaviors
            .rules
            .get(range.start as usize..range.end as usize)
            .is_none()
        {
            return Err(bad);
        }
    }
    let patch = &sequence.patch;
    for layout in &patch.value_layouts {
        match *layout {
            PatchValueLayout::Color(n)
            | PatchValueLayout::Scalar(n)
            | PatchValueLayout::Indexed(n)
            | PatchValueLayout::Components(n)
            | PatchValueLayout::Slots(n) => reserve(n as usize, 32)?,
            PatchValueLayout::Fixture { width, functions } => reserve(
                width as usize,
                (functions as usize)
                    .checked_add(1)
                    .and_then(|n| n.checked_mul(32))
                    .ok_or(LoadError::Limit)?,
            )?,
        }
    }
    for step in &patch.steps {
        match step {
            PatchStep::Source { output, source } => {
                if *output as usize >= patch.value_layouts.len() {
                    return Err(bad);
                }
                for span in &source.spans {
                    let &(_, layout) = sequence.elements.get(span.element as usize).ok_or(bad)?;
                    if span.cells.start > span.cells.end || span.cells.end as usize > cells(layout)
                    {
                        return Err(bad);
                    }
                }
            }
            PatchStep::Filter {
                input,
                output_start,
                ..
            }
            | PatchStep::Fixture {
                input,
                output_start,
                ..
            } => {
                if *input == *output_start
                    || *input as usize >= patch.value_layouts.len()
                    || *output_start as usize >= patch.value_layouts.len()
                {
                    return Err(bad);
                }
                if let PatchStep::Fixture { program, .. } = step
                    && *program as usize >= patch.fixture_programs.len()
                {
                    return Err(bad);
                }
            }
            PatchStep::Sink {
                input,
                frame,
                start,
                end,
            } => {
                if *input as usize >= patch.value_layouts.len()
                    || start > end
                    || sequence
                        .output_widths
                        .get(*frame as usize)
                        .is_none_or(|width| end > width)
                {
                    return Err(bad);
                }
            }
        }
    }
    Ok(())
}

pub(crate) struct Microseconds;

macro_rules! archive_clock {
    ($clock:ty) => {
        impl ArchiveWith<$clock> for Microseconds {
            type Archived = Archived<u32>;
            type Resolver = ();
            fn resolve_with(value: &$clock, _: (), out: Place<Self::Archived>) {
                value.ticks().resolve((), out);
            }
        }
        impl<S: Fallible + ?Sized> SerializeWith<$clock, S> for Microseconds {
            fn serialize_with(_: &$clock, _: &mut S) -> Result<(), S::Error> {
                Ok(())
            }
        }
        impl<D: Fallible + ?Sized> DeserializeWith<Archived<u32>, $clock, D> for Microseconds {
            fn deserialize_with(value: &Archived<u32>, _: &mut D) -> Result<$clock, D::Error> {
                Ok(<$clock>::from_ticks(value.to_native()))
            }
        }
    };
}
archive_clock!(SampleTime);
archive_clock!(SampleDuration);
