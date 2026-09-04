// Execution has three stages:
// 1. Dawn source is compiled once to effect/operator bytecode by dawn-language.
// 2. A sequence is prepared once: generators expand, targets resolve, and the
//    authored composition becomes a PreparedSignalGraph.
// 3. The prepared graph is sampled at a typed SampleTime. Effect layers are
//    source signals, operators transform signals, and the output node writes
//    one flat pixel signal into a caller-owned buffer.
pub(crate) mod color;
pub(crate) mod composition;
pub(crate) mod effects;
pub(crate) mod elements;
pub(crate) mod raster;
pub(crate) mod renderer;
pub(crate) mod targets;
pub(crate) mod timeline;

pub use targets::resolve_effect_target_pixel_addresses;
