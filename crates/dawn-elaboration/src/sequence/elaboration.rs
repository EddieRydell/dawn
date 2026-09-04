use dawn_language::dsl::DslBindCache;
use dawn_language::model::DawnProject;
use dawn_language::sequence::SequenceId;
use dawn_language::setup::SetupId;
use dawn_language::validation::validate_sequence;
use dawn_language::values::sample_duration_from_dawn_duration;
use indexmap::IndexSet;
use std::collections::HashMap;
use std::sync::atomic::{AtomicU32, Ordering};

use super::composition::{PrepareGraphContext, prepare_signal_graph};
use super::effects::generators::PrepareTargetCache;
use super::effects::preparation::{PrepareEffectContext, prepare_effect_inst};
use super::elements::prepare_elements;
use super::renderer::RenderError;
use super::timeline::{frame_count, prepare_timing};
use crate::{PreparedLayer, PreparedSequence};

static NEXT_SEQUENCE_ID: AtomicU32 = AtomicU32::new(1);

pub fn elaborate_sequence(
    project: &DawnProject,
    setup_id: &SetupId,
    sequence_id: &SequenceId,
) -> Result<PreparedSequence, RenderError> {
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
    let mut pixel_count = 0;
    let element_cell_offsets = elements
        .iter()
        .map(|element| {
            let offset = pixel_count;
            pixel_count += element.pixel_count;
            offset
        })
        .collect::<Vec<_>>();
    let element_ids = elements
        .iter()
        .map(|element| element.id)
        .collect::<IndexSet<_>>();
    let mut effects = Vec::with_capacity(sequence.effects.len());
    let mut generated_child_count = 0usize;
    let mut bind_cache = DslBindCache::default();
    let mut sample_programs = HashMap::new();
    let mut target_cache = PrepareTargetCache::default();
    let layers = sequence
        .layers
        .iter()
        .map(|layer| PreparedLayer {
            enabled: layer.enabled,
        })
        .collect::<Vec<_>>();
    let mut effects_by_layer = vec![Vec::new(); layers.len()];
    for effect in &sequence.effects {
        let layer_index = sequence
            .layers
            .iter()
            .position(|layer| layer.id == effect.layer_id)
            .ok_or_else(|| RenderError::BadGraph {
                message: format!(
                    "effect {} references missing layer {}",
                    effect.id.0, effect.layer_id.0
                ),
            })?;
        let first_prepared_effect = effects.len();
        prepare_effect_inst(
            PrepareEffectContext {
                project,
                sequence,
                elements: &elements,
                element_ids: &element_ids,
                groups: &groups,
                effects: &mut effects,
                generated_child_count: &mut generated_child_count,
                bind_cache: &mut bind_cache,
                sample_programs: &mut sample_programs,
                target_cache: &mut target_cache,
            },
            effect,
        )?;
        effects_by_layer[layer_index].extend(first_prepared_effect..effects.len());
    }
    for layer_effects in &mut effects_by_layer {
        layer_effects.sort_unstable_by(|left, right| {
            effects[*left]
                .start_time
                .cmp(&effects[*right].start_time)
                .then(left.cmp(right))
        });
    }
    let frame_rate = sequence.frame_rate;
    let duration = sample_duration_from_dawn_duration(&sequence.duration).map_err(|_| {
        RenderError::InvalidTiming {
            reason: "sequence duration exceeds the runtime clock range".to_string(),
        }
    })?;
    let frame_count = frame_count(&sequence.duration, frame_rate)?;
    let signal_graph = prepare_signal_graph(
        PrepareGraphContext {
            project,
            sequence,
            elements: &elements,
        },
        &sequence.composition_graph,
    )?;
    Ok(PreparedSequence {
        workspace_key: NEXT_SEQUENCE_ID.fetch_add(1, Ordering::Relaxed),
        frame_rate,
        frame_count,
        duration,
        elements: elements
            .iter()
            .map(|element| dawn_runtime::sequence::PreparedElement {
                id: element.id.0,
                pixel_count: element.pixel_count,
            })
            .collect(),
        element_cell_offsets: element_cell_offsets.into_boxed_slice(),
        pixel_count,
        effects: effects.into_boxed_slice(),
        effects_by_layer: effects_by_layer
            .into_iter()
            .map(Vec::into_boxed_slice)
            .collect(),
        layers: layers.into_boxed_slice(),
        signal_graph,
    })
}
