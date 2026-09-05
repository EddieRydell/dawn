use crate::automation::AutomationMapping;
use crate::dsl::bytecode::BytecodeProgram;
use crate::dsl::{BoundParams, RuntimeError, VmWorkspace};
use crate::native_effect::NativeSample;
use crate::values::{
    Color, Curve, SampleDuration, SampleTime, SampleTimeError, sample_time_from_frame,
};
use crate::{BuiltinEffect, BuiltinOperator};
use alloc::boxed::Box;
#[cfg(not(feature = "atomic"))]
use alloc::rc::Rc as Arc;
use alloc::string::String;
#[cfg(feature = "atomic")]
use alloc::sync::Arc;
use alloc::vec;
use alloc::vec::Vec;

pub const MAX_SIGNAL_CACHE_ENTRIES_PER_PIXEL: usize = 1_024;

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum EvaluationError {
    InvalidTiming { reason: String },
    InvalidGraph { message: String },
    Vm { message: String },
    OutputSize { expected: usize, actual: usize },
    InvalidWorkspace,
}

impl From<RuntimeError> for EvaluationError {
    fn from(error: RuntimeError) -> Self {
        Self::Vm {
            message: error.message,
        }
    }
}

#[derive(Clone, Debug)]
pub struct PreparedSequence {
    pub workspace_key: u32,
    pub frame_rate: u32,
    pub frame_count: u32,
    pub duration: SampleDuration,
    pub elements: Box<[PreparedElement]>,
    pub element_cell_offsets: Box<[usize]>,
    pub pixel_count: usize,
    pub effects: Box<[PreparedEffect]>,
    pub programs: Box<[BytecodeProgram]>,
    pub targets: Box<[PreparedTarget]>,
    pub target_pixels: Box<[PreparedPixel]>,
    pub effects_by_layer: Box<[Box<[usize]>]>,
    pub layers: Box<[PreparedLayer]>,
    pub signal_graph: PreparedSignalGraph,
}

#[derive(Clone, Copy, Debug)]
pub struct PreparedElement {
    pub id: u32,
    pub pixel_count: usize,
}

#[derive(Clone, Debug, PartialEq)]
pub struct EvaluatedFrame {
    pub frame_index: u32,
    pub frame_rate: u32,
    pub sample_time: SampleTime,
    pub elements: Vec<EvaluatedElement>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct EvaluatedElement {
    pub element_id: u32,
    pub pixels: Vec<Color>,
}

#[derive(Clone, Debug)]
pub struct PreparedEffect {
    pub start_time: SampleTime,
    pub duration: SampleDuration,
    pub target: u32,
    pub implementation: PreparedEffectImplementation,
    pub automation: Option<Box<PreparedEffectAutomation>>,
}

impl PreparedEffect {
    pub fn is_active(&self, sample_time: SampleTime) -> bool {
        sample_time >= self.start_time
            && self
                .start_time
                .checked_add_duration(self.duration)
                .is_some_and(|end| sample_time < end)
    }

    pub fn local_time(&self, sample_time: SampleTime) -> SampleDuration {
        sample_time
            .checked_duration_since(self.start_time)
            .unwrap_or(SampleDuration::from_ticks(0))
    }

    pub fn progress(&self, sample_time: SampleTime) -> f32 {
        let elapsed = sample_time
            .checked_duration_since(self.start_time)
            .map_or(0, |duration| duration.ticks());
        (elapsed as f32 / self.duration.ticks() as f32).clamp(0.0, 1.0)
    }
}

#[derive(Clone, Debug)]
pub enum PreparedEffectImplementation {
    Dsl {
        program: u32,
        bound_params: BoundParams,
    },
    Native {
        sample: NativeSample,
        params: Option<(BuiltinEffect, BoundParams)>,
    },
}

#[derive(Clone, Copy, Debug)]
pub struct PreparedLayer {
    pub enabled: bool,
}

#[derive(Clone, Debug)]
pub struct PreparedEffectAutomation {
    pub bindings: Box<[PreparedAutomation]>,
}

#[derive(Clone, Debug)]
pub struct PreparedAutomation {
    pub start: SampleTime,
    pub duration: SampleDuration,
    pub curve: Arc<Curve>,
    pub mapping: AutomationMapping,
    pub param_index: u16,
}

impl PreparedAutomation {
    pub fn position(&self, sample_time: SampleTime) -> f32 {
        let elapsed = sample_time
            .checked_duration_since(self.start)
            .map_or(0, |duration| duration.ticks());
        if self.duration.ticks() == 0 {
            0.0
        } else {
            (elapsed as f32 / self.duration.ticks() as f32).clamp(0.0, 1.0)
        }
    }
}

#[derive(Clone, Debug)]
pub struct PreparedSignalGraph {
    pub output_index: usize,
    pub target: u32,
    pub nodes: Box<[PreparedSignalNode]>,
    pub vm_workspace_count: usize,
    pub frame_nodes: Box<[usize]>,
    pub frame_slots: Box<[u16]>,
    pub frame_buffer_count: u16,
}

#[derive(Clone, Debug)]
pub struct PreparedSignalNode {
    pub kind: PreparedSignalKind,
}

#[derive(Clone, Debug)]
pub enum PreparedSignalKind {
    Layer {
        layer_index: usize,
    },
    Operator {
        operator: PreparedOperatorNode,
        inputs: Box<[usize]>,
        automation: Box<[PreparedAutomation]>,
        vm_slot: u16,
    },
    Output {
        inputs: Box<[usize]>,
    },
}

#[derive(Clone, Debug)]
pub enum PreparedOperator {
    Native(BuiltinOperator),
    Dsl(u32),
}

#[derive(Clone, Debug)]
pub struct PreparedOperatorNode {
    pub implementation: PreparedOperator,
    pub params: BoundParams,
}

#[derive(Clone, Debug)]
pub struct PreparedTarget {
    pub pixels: core::ops::Range<u32>,
    /// Zero disables sample reuse; otherwise this is the required cache width.
    pub sample_count: u32,
}

#[derive(Clone, Debug)]
pub struct PreparedPixel {
    pub element_index: u16,
    pub element_cell_index: u16,
    pub pixel_index: u32,
    pub pixel_count: u32,
    pub pixel_fraction: f32,
}

impl PreparedPixel {
    pub fn try_new(
        element_index: usize,
        element_cell_index: usize,
        pixel_index: usize,
        pixel_count: usize,
        pixel_fraction: f32,
    ) -> Option<Self> {
        Some(Self {
            element_index: u16::try_from(element_index).ok()?,
            element_cell_index: u16::try_from(element_cell_index).ok()?,
            pixel_index: u32::try_from(pixel_index).ok()?,
            pixel_count: u32::try_from(pixel_count).ok()?,
            pixel_fraction,
        })
    }

    pub fn element_index(&self) -> usize {
        self.element_index as usize
    }

    pub fn element_cell_index(&self) -> usize {
        self.element_cell_index as usize
    }

    pub fn pixel_index(&self) -> usize {
        self.pixel_index as usize
    }

    pub fn pixel_count(&self) -> usize {
        self.pixel_count as usize
    }
}

#[derive(Debug)]
pub struct EvaluationWorkspace {
    pub(crate) effect_vm: VmWorkspace,
    pub(crate) operator_vm: Vec<VmWorkspace>,
    pub(crate) signal_cache: Vec<CachedSignal>,
    pub(crate) signal_buffers: Box<[Color]>,
    pub(crate) effect_samples: Vec<CachedEffectSample>,
    pub(crate) effect_automation: Vec<Option<EffectAutomationWorkspace>>,
    pub(crate) operator_automation: Vec<Option<BoundParams>>,
    pub(crate) workspace_key: Option<u32>,
    pub(crate) frame_colors: Vec<Color>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct SignalCacheKey {
    pub(crate) node_index: usize,
    pub(crate) sample_time: SampleTime,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct CachedSignal {
    pub(crate) key: SignalCacheKey,
    pub(crate) color: Color,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct CachedEffectSample {
    pub(crate) pixel_count: u32,
    pub(crate) color: Color,
}

#[derive(Debug, Default)]
pub(crate) struct EffectAutomationWorkspace {
    pub(crate) params: Option<BoundParams>,
    pub(crate) native_sample: Option<NativeSample>,
    pub(crate) sample_time: Option<SampleTime>,
}

impl PreparedSequence {
    pub fn target(&self, index: u32) -> &[PreparedPixel] {
        let range = &self.targets[index as usize].pixels;
        &self.target_pixels[range.start as usize..range.end as usize]
    }

    pub fn frame_count(&self) -> u32 {
        self.frame_count
    }

    pub fn frame_rate(&self) -> u32 {
        self.frame_rate
    }

    pub fn pixel_count(&self) -> usize {
        self.pixel_count
    }

    pub fn duration(&self) -> SampleDuration {
        self.duration
    }

    /// Preallocates frame buffers, VM registers, and automation storage.
    /// Calculated array construction and reference-valued VM operations can
    /// still allocate; workspace preparation alone does not exclude them.
    pub fn workspace(&self) -> EvaluationWorkspace {
        let mut workspace = EvaluationWorkspace {
            effect_vm: VmWorkspace::default(),
            operator_vm: (0..self.signal_graph.vm_workspace_count)
                .map(|_| VmWorkspace::default())
                .collect(),
            signal_cache: Vec::with_capacity(
                if self.signal_graph.frame_nodes.iter().any(|&index| {
                    matches!(
                        self.signal_graph.nodes[index].kind,
                        PreparedSignalKind::Operator {
                            operator: PreparedOperatorNode {
                                implementation: PreparedOperator::Dsl(_)
                                    | PreparedOperator::Native(
                                        BuiltinOperator::Delay | BuiltinOperator::Echo
                                    ),
                                ..
                            },
                            ..
                        }
                    )
                }) {
                    MAX_SIGNAL_CACHE_ENTRIES_PER_PIXEL
                } else {
                    0
                },
            ),
            signal_buffers: vec![
                Color {
                    red: 0,
                    green: 0,
                    blue: 0
                };
                usize::from(self.signal_graph.frame_buffer_count)
                    * self.pixel_count
            ]
            .into_boxed_slice(),
            effect_samples: vec![
                CachedEffectSample {
                    pixel_count: 0,
                    color: Color {
                        red: 0,
                        green: 0,
                        blue: 0
                    }
                };
                self.effects
                    .iter()
                    .map(|effect| self.targets[effect.target as usize].sample_count as usize)
                    .max()
                    .unwrap_or(0)
            ],
            effect_automation: self
                .effects
                .iter()
                .map(|effect| {
                    let automation = effect.automation.as_ref()?;
                    let params = match &effect.implementation {
                        PreparedEffectImplementation::Dsl { bound_params, .. }
                        | PreparedEffectImplementation::Native {
                            params: Some((_, bound_params)),
                            ..
                        } => bound_params,
                        PreparedEffectImplementation::Native { params: None, .. } => return None,
                    };
                    Some(EffectAutomationWorkspace {
                        params: Some(automation_params(params, &automation.bindings)),
                        native_sample: None,
                        sample_time: None,
                    })
                })
                .collect(),
            operator_automation: self
                .signal_graph
                .nodes
                .iter()
                .map(|node| match &node.kind {
                    PreparedSignalKind::Operator {
                        operator,
                        automation,
                        ..
                    } if !automation.is_empty() => {
                        Some(automation_params(&operator.params, automation))
                    }
                    _ => None,
                })
                .collect(),
            workspace_key: Some(self.workspace_key),
            frame_colors: Vec::new(),
        };
        for effect in self.effects.iter() {
            if let PreparedEffectImplementation::Dsl {
                program,
                bound_params,
            } = &effect.implementation
            {
                workspace
                    .effect_vm
                    .reserve(&self.programs[*program as usize], bound_params.len());
            }
        }
        for node in self.signal_graph.nodes.iter() {
            let PreparedSignalKind::Operator {
                operator:
                    PreparedOperatorNode {
                        implementation: PreparedOperator::Dsl(program),
                        params,
                    },
                vm_slot,
                ..
            } = &node.kind
            else {
                continue;
            };
            workspace.operator_vm[usize::from(*vm_slot)]
                .reserve(&self.programs[*program as usize], params.len());
        }
        workspace
    }

    pub fn evaluate(
        &self,
        sample_time: SampleTime,
        output: &mut [Color],
        workspace: &mut EvaluationWorkspace,
    ) -> Result<(), EvaluationError> {
        if output.len() != self.pixel_count {
            return Err(EvaluationError::OutputSize {
                expected: self.pixel_count,
                actual: output.len(),
            });
        }
        if workspace.workspace_key != Some(self.workspace_key) {
            return Err(EvaluationError::InvalidWorkspace);
        }
        if sample_time.ticks() >= self.duration.ticks() {
            output.fill(Color {
                red: 0,
                green: 0,
                blue: 0,
            });
            return Ok(());
        }
        crate::evaluation::sample_signal_graph(self, sample_time, output, workspace)
    }

    pub fn evaluate_frame(&self, frame_index: u32) -> Result<EvaluatedFrame, EvaluationError> {
        self.evaluate_frame_with_workspace(frame_index, &mut self.workspace())
    }

    pub fn evaluate_frame_with_workspace(
        &self,
        frame_index: u32,
        workspace: &mut EvaluationWorkspace,
    ) -> Result<EvaluatedFrame, EvaluationError> {
        let frame_index = frame_index.min(self.frame_count.saturating_sub(1));
        let sample_time =
            sample_time_from_frame(frame_index, self.frame_rate).map_err(sample_time_error)?;
        self.evaluate_elements(frame_index, sample_time, workspace)
    }

    fn evaluate_elements(
        &self,
        frame_index: u32,
        sample_time: SampleTime,
        workspace: &mut EvaluationWorkspace,
    ) -> Result<EvaluatedFrame, EvaluationError> {
        let mut colors = core::mem::take(&mut workspace.frame_colors);
        colors.resize(
            self.pixel_count,
            Color {
                red: 0,
                green: 0,
                blue: 0,
            },
        );
        if let Err(error) = self.evaluate(sample_time, &mut colors, workspace) {
            workspace.frame_colors = colors;
            return Err(error);
        }
        let mut offset = 0;
        let elements = self
            .elements
            .iter()
            .map(|element| {
                let end = offset + element.pixel_count;
                let pixels = colors[offset..end].to_vec();
                offset = end;
                EvaluatedElement {
                    element_id: element.id,
                    pixels,
                }
            })
            .collect();
        workspace.frame_colors = colors;
        Ok(EvaluatedFrame {
            frame_index,
            frame_rate: self.frame_rate,
            sample_time,
            elements,
        })
    }

    pub fn active_effect_count(&self, sample_time: SampleTime) -> usize {
        self.effects
            .iter()
            .filter(|effect| effect.is_active(sample_time))
            .count()
    }

    pub fn active_effect_count_at_frame(&self, frame_index: u32) -> usize {
        sample_time_from_frame(frame_index, self.frame_rate)
            .map(|time| self.active_effect_count(time))
            .unwrap_or(0)
    }
}

fn automation_params(params: &BoundParams, automation: &[PreparedAutomation]) -> BoundParams {
    let mut params = params.clone_for_automation();
    for binding in automation {
        params.reserve_automation(
            usize::from(binding.param_index),
            &binding.curve,
            &binding.mapping,
        );
    }
    params
}

fn sample_time_error(error: SampleTimeError) -> EvaluationError {
    EvaluationError::InvalidTiming {
        reason: alloc::format!("invalid sample time: {error:?}"),
    }
}
