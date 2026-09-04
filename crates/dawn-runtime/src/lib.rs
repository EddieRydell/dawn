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
pub use dawn_language::values::{SampleDuration, SampleTime};
pub use output::*;
pub use sequence::raster::{
    EffectRasterPrepareBatch, EffectRasterRenderScratch, PreparedEffectRasterRenderer,
    PreparedEffectRasterSample,
};
pub(crate) use sequence::renderer::{
    EffectAutomationScratch, GeneratedTargetCacheEntry, GeneratorContextTargetCacheEntry,
    MAX_GENERATED_EFFECTS, PrepareTargetCache, PreparedAutomation, PreparedEffect,
    PreparedEffectImplementation, arc_key,
};
pub use sequence::renderer::{
    PreparedSequenceRenderer, RenderError, RenderedElement, RenderedFrame,
    RenderedTargetPixelAddress, SequenceRenderScratch,
};
pub use sequence::resolve_effect_target_pixel_addresses;

pub(crate) use sequence::effects::parameters::EffectParamTiming;
pub(crate) use sequence::elements::PreparedElement;

#[cfg(test)]
mod tests;
