#![deny(unsafe_code)]
#![cfg_attr(
    not(test),
    deny(
        clippy::expect_used,
        clippy::panic,
        clippy::todo,
        clippy::unimplemented,
        clippy::unwrap_used
    )
)]

mod output;
mod sequence;
pub use output::*;
pub use sequence::raster::{
    EffectRasterPrepareBatch, EffectRasterRenderScratch, PreparedEffectRasterRenderer,
    PreparedEffectRasterSample,
};
pub(crate) use sequence::renderer::{
    GeneratedTargetCacheEntry, GeneratorContextTargetCacheEntry, GraphRenderCacheKey,
    MAX_GENERATED_EFFECTS, MAX_SIGNAL_SAMPLES_PER_OPERATOR_RENDER, PrepareTargetCache,
    PreparedAutomation, PreparedEffect, PreparedEffectImplementation, PreparedLayer, arc_key,
};
pub use sequence::renderer::{
    PreparedSequenceRenderer, RenderError, RenderedElement, RenderedFrame,
    RenderedTargetPixelAddress, SequenceRenderScratch,
};
pub use sequence::resolve_effect_target_pixel_addresses;

pub(crate) use dawn_language::dsl::{
    BoundParams, CompiledEffect, DslBindCache, DslVmScratch, EffectKind, Identifier,
    OperatorRunContext, RuntimeError, SignalSampler, Value,
};
pub(crate) use dawn_language::effect::{
    EffectDefinitionId, EffectImplementation, EffectInstId, EffectRef,
};
pub(crate) use dawn_language::element::ElementNodeId;
pub(crate) use dawn_language::model::DawnProject;
pub(crate) use dawn_language::native_effect::{self, BoundNativeEffect};
pub(crate) use dawn_language::operator::OperatorDefinition;
pub(crate) use dawn_language::sequence::{
    AutomationBinding, AutomationClip, Sequence, SequenceId, SequenceLayerId,
};
pub(crate) use dawn_language::setup::SetupId;
pub(crate) use dawn_language::validation::validate_sequence;
pub(crate) use dawn_language::values::{Color, Marks};
pub(crate) use indexmap::{IndexMap, IndexSet};
pub(crate) use sequence::effects::generators::{
    GeneratorExpansion, GeneratorPrepareContext, expand_generator, expand_native_generator,
};
pub(crate) use sequence::effects::parameters::EffectParamTiming;
pub(crate) use sequence::effects::preparation::{PrepareEffectContext, prepare_effect_inst};
pub(crate) use sequence::effects::sampling::{
    PreparedSampledEffectPixel, PreparedSampledEffectPixels, TargetColorAddress,
    effect_implementation_at, evenly_sample_indices, prepare_sample_groups_for_implementation,
    prepare_sampled_effect_pixel_groups, render_sampled_effect_target_colors, sample_effect_group,
};
pub(crate) use sequence::elements::{PreparedElement, prepare_elements};
pub(crate) use sequence::targets::{
    PreparedTargetPixel, generator_expansion_targets, prepare_target, prepare_target_pixels_cached,
};
pub(crate) use sequence::timeline::{
    build_effect_frame_index_for_window, frame_count, prepare_timing,
};
pub(crate) use std::collections::HashMap;
pub(crate) use std::sync::Arc;

#[cfg(test)]
mod tests;
