use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// How multiple effect layers combine their output.
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    Serialize,
    Deserialize,
    TS,
    JsonSchema,
    strum::Display,
    strum::VariantArray,
)]
#[ts(export)]
pub enum BlendMode {
    /// Top layer fully replaces the layer below.
    Override,
    /// Additive blend (clamped at 255 per channel).
    Add,
    /// Multiplicative blend.
    Multiply,
    /// Per-channel maximum.
    Max,
    /// Alpha composite (foreground over background).
    Alpha,
    /// Saturating subtraction per channel.
    Subtract,
    /// Per-channel minimum.
    Min,
    /// Per-channel average.
    Average,
    /// Screen blend (complement of multiply).
    Screen,
    /// Where foreground is non-black, output black; else preserve background.
    Mask,
    /// Scale background brightness by foreground luminance.
    IntensityOverlay,
}

impl BlendMode {
    pub const fn description(&self) -> &'static str {
        match self {
            BlendMode::Override => "Solo effect, replaces everything below",
            BlendMode::Add => "Glow, energy buildup — adds light (never darkens)",
            BlendMode::Multiply => "Shadow, dimming — darkens (never brightens)",
            BlendMode::Max => "Peak detection — takes brightest channel",
            BlendMode::Alpha => "Standard overlay — uses opacity for transparency",
            BlendMode::Subtract => "Mask out colors from below",
            BlendMode::Min => "Minimum of both — creates dark intersections",
            BlendMode::Average => "Blend of both — good for smooth transitions",
            BlendMode::Screen => "Soft brightening — lighter than Add",
            BlendMode::Mask => "Uses top layer as brightness mask on bottom",
            BlendMode::IntensityOverlay => "Uses top layer's brightness to modulate bottom",
        }
    }
}
