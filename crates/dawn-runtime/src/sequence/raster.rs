use crate::sequence::color::black;
use crate::*;

#[derive(Clone, Debug)]
pub struct PreparedEffectRasterRenderer {
    frame_rate: u32,
    frame_count: u64,
    index_start_frame: u64,
    start_seconds: f64,
    duration_seconds: f64,
    target: Arc<Vec<PreparedTargetPixel>>,
    effects: Vec<PreparedEffect>,
    effects_by_frame: Vec<Vec<usize>>,
}

#[derive(Clone, Debug)]
pub struct PreparedEffectRasterSample {
    row_count: usize,
    effect_pixels: Vec<PreparedSampledEffectPixels>,
}

#[derive(Debug, Default)]
pub struct EffectRasterRenderScratch {
    effect_vm: Vec<DslVmScratch>,
    bind_cache: DslBindCache,
}

pub struct EffectRasterPrepareBatch<'a> {
    project: &'a DawnProject,
    sequence: &'a Sequence,
    elements: Vec<PreparedElement>,
    element_ids: IndexSet<ElementNodeId>,
    groups: IndexMap<ElementNodeId, Vec<ElementNodeId>>,
    frame_rate: u32,
    frame_count: u64,
    bind_cache: DslBindCache,
    compiled_effects: HashMap<EffectDefinitionId, Arc<CompiledEffect>>,
    target_cache: PrepareTargetCache,
}

impl PreparedEffectRasterRenderer {
    pub fn prepare(
        project: &DawnProject,
        setup_id: &SetupId,
        sequence_id: &SequenceId,
        effect_id: &EffectInstId,
    ) -> Result<Self, RenderError> {
        let mut batch = EffectRasterPrepareBatch::prepare(project, setup_id, sequence_id)?;
        batch.prepare_effect(effect_id)
    }

    pub fn start_seconds(&self) -> f64 {
        self.start_seconds
    }

    pub fn duration_seconds(&self) -> f64 {
        self.duration_seconds
    }

    pub fn frame_rate(&self) -> u32 {
        self.frame_rate
    }

    pub fn target_pixel_count(&self) -> usize {
        self.target.len()
    }

    pub fn sampled_raster_column_seconds(
        &self,
        column_index: usize,
        column_count: usize,
    ) -> Result<f64, RenderError> {
        if column_count == 0 || column_index >= column_count {
            return Err(RenderError::InvalidTiming {
                reason: "raster column index must be within a non-empty raster".to_string(),
            });
        }
        let end_frame = ((self.start_seconds + self.duration_seconds) * f64::from(self.frame_rate))
            .ceil()
            .max(self.index_start_frame as f64) as u64;
        let active_frame_count = end_frame.saturating_sub(self.index_start_frame).max(1);
        let sample_offset = (((column_index as f64 + 0.5) * active_frame_count as f64
            / column_count as f64)
            .floor() as u64)
            .min(active_frame_count.saturating_sub(1));
        Ok((self.index_start_frame + sample_offset) as f64 / f64::from(self.frame_rate))
    }

    pub fn prepare_sampled_raster(&self, row_count: usize) -> PreparedEffectRasterSample {
        let sample_indices = evenly_sample_indices(self.target.len(), row_count);
        let row_count = sample_indices.len();
        let mut sample_lookup = HashMap::<TargetColorAddress, Vec<usize>>::new();
        for (row_index, target_index) in sample_indices.into_iter().enumerate() {
            if let Some(pixel) = self.target.get(target_index) {
                sample_lookup
                    .entry(TargetColorAddress {
                        element_index: pixel.element_index,
                        element_cell_index: pixel.element_cell_index,
                    })
                    .or_default()
                    .push(row_index);
            }
        }
        let effect_pixels = self
            .effects
            .iter()
            .map(|effect| {
                let pixels = effect
                    .target
                    .iter()
                    .filter_map(|pixel| {
                        sample_lookup
                            .get(&TargetColorAddress {
                                element_index: pixel.element_index,
                                element_cell_index: pixel.element_cell_index,
                            })
                            .map(|rows| PreparedSampledEffectPixel {
                                pixel: pixel.clone(),
                                rows: rows.clone(),
                            })
                    })
                    .collect::<Vec<_>>();
                let groups = prepare_sampled_effect_pixel_groups(&pixels);
                PreparedSampledEffectPixels { pixels, groups }
            })
            .collect();
        PreparedEffectRasterSample {
            row_count,
            effect_pixels,
        }
    }

    pub fn render_sampled_raster_column_with_scratch(
        &self,
        sample: &PreparedEffectRasterSample,
        audio_seconds: f64,
        scratch: &mut EffectRasterRenderScratch,
    ) -> Result<Vec<Color>, RenderError> {
        if !audio_seconds.is_finite() {
            return Err(RenderError::InvalidTiming {
                reason: "audio seconds must be finite".to_string(),
            });
        }
        let max_frame = self.frame_count.saturating_sub(1);
        let frame_index = (audio_seconds * f64::from(self.frame_rate)).floor();
        let frame_index = if frame_index < 0.0 {
            0
        } else if frame_index > max_frame as f64 {
            max_frame
        } else {
            frame_index as u64
        };
        self.render_sampled_raster_column_at_with_scratch(sample, frame_index, scratch)
    }

    fn render_sampled_raster_column_at_with_scratch(
        &self,
        sample: &PreparedEffectRasterSample,
        frame_index: u64,
        scratch: &mut EffectRasterRenderScratch,
    ) -> Result<Vec<Color>, RenderError> {
        scratch
            .effect_vm
            .resize_with(self.effects.len(), DslVmScratch::default);
        let sample_seconds = frame_index as f64 / f64::from(self.frame_rate);
        let mut rendered = vec![black(); sample.row_count];

        let local_frame_index = frame_index.saturating_sub(self.index_start_frame);
        if let Some(active_effects) = self.effects_by_frame.get(local_frame_index as usize) {
            for effect_index in active_effects {
                let Some(effect) = self.effects.get(*effect_index) else {
                    continue;
                };
                if sample_seconds < effect.start_seconds
                    || sample_seconds >= effect.start_seconds + effect.duration_seconds
                {
                    continue;
                }
                let Some(effect_pixels) = sample.effect_pixels.get(*effect_index) else {
                    continue;
                };
                render_sampled_effect_target_colors(
                    effect,
                    effect_pixels,
                    &mut rendered,
                    sample_seconds,
                    &mut scratch.effect_vm[*effect_index],
                    &mut scratch.bind_cache,
                )?;
            }
        }

        Ok(rendered)
    }
}

impl<'a> EffectRasterPrepareBatch<'a> {
    pub fn prepare(
        project: &'a DawnProject,
        setup_id: &SetupId,
        sequence_id: &SequenceId,
    ) -> Result<Self, RenderError> {
        let setup = project
            .setups
            .get(setup_id)
            .ok_or_else(|| RenderError::MissingSetup {
                setup_id: setup_id.clone(),
            })?;
        let tree = project
            .element_trees
            .get(&setup.elements)
            .ok_or(RenderError::MissingElementTree)?;
        let sequence =
            project
                .sequences
                .get(sequence_id)
                .ok_or_else(|| RenderError::MissingSequence {
                    sequence_id: sequence_id.clone(),
                })?;
        validate_sequence(project, sequence).map_err(|error| RenderError::BadGraph {
            message: error.message,
        })?;
        prepare_timing(sequence)?;

        let (elements, groups) = prepare_elements(project, tree)?;
        let element_ids = elements
            .iter()
            .map(|element| element.id)
            .collect::<IndexSet<_>>();
        let frame_rate = sequence.frame_rate;
        let duration_seconds = sequence.duration.as_seconds_f64();
        let frame_count = frame_count(duration_seconds, frame_rate);

        Ok(Self {
            project,
            sequence,
            elements,
            element_ids,
            groups,
            frame_rate,
            frame_count,
            bind_cache: DslBindCache::default(),
            compiled_effects: HashMap::new(),
            target_cache: PrepareTargetCache::default(),
        })
    }

    pub fn prepare_effect(
        &mut self,
        effect_id: &EffectInstId,
    ) -> Result<PreparedEffectRasterRenderer, RenderError> {
        let effect = self
            .sequence
            .effects
            .iter()
            .find(|effect| effect.id == *effect_id)
            .ok_or(RenderError::MissingEffectInstance {
                effect_id: effect_id.clone(),
            })?;
        let mut effects = Vec::new();
        let mut generated_child_count = 0usize;
        let target = prepare_effect_inst(
            PrepareEffectContext {
                project: self.project,
                sequence: self.sequence,
                layer_id: SequenceLayerId(0),
                elements: &self.elements,
                element_ids: &self.element_ids,
                groups: &self.groups,
                effects: &mut effects,
                generated_child_count: &mut generated_child_count,
                bind_cache: &mut self.bind_cache,
                compiled_effects: &mut self.compiled_effects,
                target_cache: &mut self.target_cache,
            },
            effect,
        )?;

        let start_seconds = effect.start.as_seconds_f64();
        let duration_seconds = effect.duration.as_seconds_f64();
        let index_start_frame = (start_seconds * f64::from(self.frame_rate)).ceil().max(0.0) as u64;
        let index_end_frame = ((start_seconds + duration_seconds) * f64::from(self.frame_rate))
            .ceil()
            .max(index_start_frame as f64) as u64;
        let index_frame_count = index_end_frame.saturating_sub(index_start_frame).max(1);
        let effects_by_frame = build_effect_frame_index_for_window(
            &effects,
            index_start_frame,
            index_frame_count,
            self.frame_rate,
        );
        Ok(PreparedEffectRasterRenderer {
            frame_rate: self.frame_rate,
            frame_count: self.frame_count,
            index_start_frame,
            start_seconds,
            duration_seconds,
            target,
            effects,
            effects_by_frame,
        })
    }
}
