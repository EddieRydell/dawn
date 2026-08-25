pub(crate) mod targets;
pub(crate) mod timeline;

pub(crate) use timeline::{
    build_effect_frame_index, build_effect_frame_index_for_window, frame_count, prepare_timing,
};

pub(crate) use targets::{
    PreparedTargetCache, PreparedTargetPixel, generator_expansion_targets, prepare_target,
    prepare_target_pixels_cached,
};

pub use targets::resolve_effect_target_pixel_addresses;
