use std::borrow::Cow;
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, LazyLock, Mutex};

use indexmap::IndexMap;

/// Tracks which fixture IDs have already had a layout mismatch warning printed,
/// so we only warn once per fixture per session.
pub static LAYOUT_WARNED: LazyLock<Mutex<HashSet<FixtureId>>> =
    LazyLock::new(|| Mutex::new(HashSet::new()));

use crate::dsl::compiler::CompiledScript;
use crate::effects;
use crate::model::automation::{AutomationClip, ClipId};
use crate::model::color_gradient::ColorGradient;
use crate::model::params::{EffectParams, ParamKey, ParamSchema, ParamType, ParamValue};
use crate::model::show::Position2D;
use crate::model::time_range::TimeRange;
use crate::model::timeline::NodeTimeline;
use crate::model::{
    Color, EffectId, EffectInstance, EffectKind, FixtureId, GroupId, NodeId, Sequence, Show,
};

/// Slice metadata for one fixture inside a flat frame pixel buffer.
///
/// `start_pixel` is the fixture's first pixel index in `Frame.pixels`,
/// where each pixel occupies 4 bytes in RGBA order.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, ts_rs::TS)]
#[ts(export)]
pub struct FrameFixtureSpan {
    pub fixture_id: u32,
    pub start_pixel: usize,
    pub pixel_count: usize,
}

/// A single frame of output as a flat RGBA pixel buffer.
///
/// Pixels are laid out fixture-by-fixture. All fixtures that appear in the
/// preview layout come first in layout order, followed by any remaining
/// fixtures in show order. This gives the preview renderer a contiguous prefix
/// it can draw directly, while still preserving per-fixture spans for output.
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize, ts_rs::TS)]
#[ts(export)]
pub struct Frame {
    /// Flat RGBA byte buffer for the entire frame.
    pub pixels: Vec<u8>,
    /// Per-fixture spans inside `pixels`.
    pub fixture_spans: Vec<FrameFixtureSpan>,
    /// Diagnostic warnings when the frame is empty for a known reason
    /// (e.g. missing sequence, no node timelines). `None` when there is nothing to report.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub warnings: Option<Vec<String>>,
}

impl Frame {
    pub fn fixture_pixels(&self, fixture_id: u32) -> Option<&[u8]> {
        let span = self
            .fixture_spans
            .iter()
            .find(|span| span.fixture_id == fixture_id)?;
        let start = span.start_pixel.checked_mul(4)?;
        let end = start.checked_add(span.pixel_count.checked_mul(4)?)?;
        self.pixels.get(start..end)
    }
}

/// Evenly-spaced horizontal fallback positions for fixtures without layout data.
#[allow(clippy::cast_precision_loss)]
fn push_fallback_positions(dest: &mut Vec<Position2D>, pixel_count: usize) {
    for i in 0..pixel_count {
        let x = if pixel_count > 1 {
            i as f32 / (pixel_count - 1) as f32
        } else {
            0.5
        };
        dest.push(Position2D { x, y: 0.5 });
    }
}

/// Extend a byte buffer with raw RGBA bytes from a `Color` slice.
///
/// # Safety
/// `Color` is `#[repr(C)]` with fields `{r: u8, g: u8, b: u8, a: u8}`,
/// identical layout to `[u8; 4]`.
fn extend_with_colors(target: &mut Vec<u8>, colors: &[Color]) {
    const _: () = assert!(std::mem::size_of::<Color>() == 4);
    const _: () = assert!(std::mem::align_of::<Color>() == 1);

    // SAFETY: Color is #[repr(C)] {r: u8, g: u8, b: u8, a: u8} = 4 bytes.
    let bytes: &[u8] =
        unsafe { std::slice::from_raw_parts(colors.as_ptr().cast::<u8>(), colors.len() * 4) };
    target.extend_from_slice(bytes);
}

fn extend_with_black(target: &mut Vec<u8>, pixel_count: usize) {
    for _ in 0..pixel_count {
        target.extend_from_slice(&[0, 0, 0, 255]);
    }
}

/// Apply automation clip overrides to effect parameters.
///
/// For each entry in `param_links`, look up the corresponding clip.
/// If the clip is active at time `t`, evaluate it to get a 0-1 value,
/// then map that value to the parameter's type based on the current value.
/// If the clip has ended (not active), the parameter holds its last value
/// via Clamp mode on the clip.
///
/// For Curve params, the automation clip's curve is "windowed" into the
/// effect's time range: sampling the clip at absolute times across the
/// effect to build a slice curve. Where the effect extends beyond the clip,
/// values are held flat (clamp behavior).
///
/// `param_schemas` is used to determine param types when the param has no
/// stored value (effects use defaults from schema, so params are often absent).
#[allow(clippy::implicit_hasher)]
pub fn resolve_automation(
    params: &EffectParams,
    param_links: &HashMap<ParamKey, ClipId>,
    clip_lookup: &HashMap<&ClipId, &AutomationClip>,
    t: f64,
    effect_range: &TimeRange,
    param_schemas: &[ParamSchema],
) -> Option<EffectParams> {
    if param_links.is_empty() {
        return None;
    }
    let mut resolved = params.clone();
    for (key, clip_id) in param_links {
        if let Some(clip) = clip_lookup.get(clip_id) {
            // Determine if this is a Curve param: check stored value first,
            // then fall back to schema default (effects often omit defaults
            // from their stored params).
            let is_curve = params.get(key).map_or_else(
                || is_curve_in_schema(key, param_schemas),
                |v| v.as_curve().is_some(),
            );
            if is_curve {
                let mapped = slice_clip_as_curve(clip, effect_range);
                resolved.set_mut(key.clone(), mapped);
                continue;
            }
            // Scalar params: evaluate clip at current time.
            let y = clip.evaluate(t);
            if let Some(current) = params.get(key) {
                let mapped = map_automation_value(y, current);
                resolved.set_mut(key.clone(), mapped);
            } else {
                resolved.set_mut(key.clone(), ParamValue::Float(y));
            }
        }
    }
    Some(resolved)
}

/// Check whether a param key is a Curve type according to the schema.
fn is_curve_in_schema(key: &ParamKey, schemas: &[ParamSchema]) -> bool {
    schemas
        .iter()
        .any(|s| &s.key == key && matches!(s.param_type, ParamType::Curve))
}

/// Sample an automation clip across an effect's time range to produce a Curve
/// that acts as a "window" into the clip. The effect sees only the slice of the
/// clip that overlaps its own time range.
///
/// Uses the clip's own control points within the overlap region for accuracy,
/// plus boundary samples at the effect's start/end.
#[allow(clippy::cast_precision_loss)]
fn slice_clip_as_curve(clip: &AutomationClip, effect_range: &TimeRange) -> ParamValue {
    use crate::model::curve::{Curve, CurvePoint};

    let eff_start = effect_range.start();
    let eff_dur = effect_range.duration();

    if eff_dur <= 0.0 {
        let y = clip.evaluate(eff_start);
        return ParamValue::Curve(Curve::constant(y));
    }

    let mut points: Vec<CurvePoint> = Vec::new();

    points.push(CurvePoint {
        x: 0.0,
        y: clip.evaluate(eff_start),
    });
    points.push(CurvePoint {
        x: 1.0,
        y: clip.evaluate(effect_range.end()),
    });

    let clip_start = clip.time_range.start();
    let clip_dur = clip.time_range.duration();
    if clip_dur > 0.0 {
        for cp in clip.curve.points() {
            let abs_t = clip_start + cp.x * clip_dur;
            let eff_x = (abs_t - eff_start) / eff_dur;
            if eff_x > 0.0 && eff_x < 1.0 {
                points.push(CurvePoint {
                    x: eff_x,
                    y: clip.evaluate(abs_t),
                });
            }
        }
    }

    points.sort_by(|a, b| a.x.partial_cmp(&b.x).unwrap_or(std::cmp::Ordering::Equal));
    points.dedup_by(|a, b| (a.x - b.x).abs() < crate::model::time_range::TIME_EPSILON);

    ParamValue::Curve(Curve::new(points).unwrap_or_else(Curve::linear))
}

/// Map a normalized 0-1 automation value to the appropriate `ParamValue`
/// based on the current parameter value's type.
pub fn map_automation_value(y: f64, current: &ParamValue) -> ParamValue {
    match current {
        ParamValue::Bool(_) => ParamValue::Bool(y >= 0.5),
        _ => ParamValue::Float(y),
    }
}

/// Pre-computed lookups shared across all node timelines during frame evaluation.
struct EvalContext<'a> {
    pixel_counts: HashMap<FixtureId, usize>,
    layout_map: HashMap<FixtureId, &'a [Position2D]>,
    group_fixtures: HashMap<GroupId, Vec<FixtureId>>,
    clip_lookup: HashMap<&'a ClipId, &'a AutomationClip>,
    motion_path_lib: &'a HashMap<String, crate::model::MotionPath>,
    script_cache: Option<&'a IndexMap<String, Arc<CompiledScript>>>,
    gradient_lib: &'a HashMap<String, ColorGradient>,
}

impl<'a> EvalContext<'a> {
    #[allow(clippy::cast_precision_loss)]
    fn new(
        show: &'a Show,
        sequence: &'a Sequence,
        script_cache: Option<&'a IndexMap<String, Arc<CompiledScript>>>,
        gradient_lib: &'a HashMap<String, ColorGradient>,
    ) -> Self {
        Self {
            pixel_counts: show
                .fixtures
                .iter()
                .map(|f| (f.id, f.pixel_count as usize))
                .collect(),
            layout_map: show
                .layout
                .fixtures
                .iter()
                .map(|fl| (fl.fixture_id, fl.pixel_positions.as_slice()))
                .collect(),
            group_fixtures: show
                .groups
                .iter()
                .map(|g| (g.id, g.resolve_fixture_ids(&show.groups)))
                .collect(),
            clip_lookup: sequence.clip_lookup(),
            motion_path_lib: &sequence.motion_paths,
            script_cache,
            gradient_lib,
        }
    }

    /// Resolve which fixture IDs a node covers.
    fn resolve_node_fixtures<'b>(&'b self, node_id: &'b NodeId) -> &'b [FixtureId] {
        match node_id {
            NodeId::Fixture(fid) => std::slice::from_ref(fid),
            NodeId::Group(gid) => self.group_fixtures.get(gid).map_or(&[], |v| v.as_slice()),
        }
    }
}

/// Evaluate a node timeline at time `t`, blending results into `frame`.
#[allow(clippy::cast_precision_loss, clippy::implicit_hasher)]
fn evaluate_node_timeline(
    node_id: &NodeId,
    timeline: &NodeTimeline,
    target_fixtures: &[FixtureId],
    t: f64,
    effect_filter: Option<&[EffectId]>,
    ctx: &EvalContext<'_>,
    frame: &mut HashMap<FixtureId, Vec<Color>>,
) {
    // Binary search for active effects.
    let end_idx = timeline
        .items
        .partition_point(|item| item.time_range().start() <= t);

    let active: Vec<&EffectInstance> = timeline
        .items
        .get(..end_idx)
        .unwrap_or(&timeline.items)
        .iter()
        .filter_map(|item| item.as_effect())
        .filter(|e| {
            e.time_range.contains(t) && effect_filter.is_none_or(|f| f.iter().any(|id| id == &e.id))
        })
        .collect();
    if active.is_empty() {
        return;
    }

    // Compute total pixel count across all target fixtures.
    let total_pixels: usize = target_fixtures
        .iter()
        .map(|id| ctx.pixel_counts.get(id).copied().unwrap_or(0))
        .sum();

    for effect_instance in &active {
        let t_normalized = effect_instance.time_range.normalize(t);
        let spatial = effects::needs_positions(&effect_instance.kind);

        let auto_params = if effect_instance.param_links.is_empty() {
            None
        } else {
            let schemas = crate::registry::types::effect_schema_for_kind(
                &effect_instance.kind,
                ctx.script_cache,
            )
            .unwrap_or_else(|err| {
                log::warn!(
                    "Could not resolve schema for effect {:?}: {}",
                    effect_instance.id,
                    err
                );
                Vec::new()
            });
            resolve_automation(
                &effect_instance.params,
                &effect_instance.param_links,
                &ctx.clip_lookup,
                t,
                &effect_instance.time_range,
                &schemas,
            )
        };
        let base_params = auto_params.as_ref().unwrap_or(&effect_instance.params);

        let resolved_params: Cow<'_, _> = if base_params.has_refs() {
            Cow::Owned(base_params.resolve_refs(ctx.gradient_lib))
        } else {
            Cow::Borrowed(base_params)
        };

        let positions: Option<Vec<Position2D>> = if spatial {
            Some(build_spatial_positions(
                target_fixtures,
                &ctx.pixel_counts,
                &ctx.layout_map,
                total_pixels,
            ))
        } else {
            None
        };

        let mut global_pixel_offset = 0usize;

        for &fixture_id in target_fixtures {
            let pixel_count = ctx.pixel_counts.get(&fixture_id).copied().unwrap_or(0);
            if pixel_count == 0 {
                continue;
            }

            let pixels = frame
                .entry(fixture_id)
                .or_insert_with(|| vec![Color::BLACK; pixel_count]);

            let fixture_positions = positions
                .as_ref()
                .and_then(|p| p.get(global_pixel_offset..global_pixel_offset + pixel_count));

            let handled = effects::evaluate_pixels(
                &effect_instance.kind,
                t_normalized,
                pixels,
                global_pixel_offset,
                total_pixels,
                &resolved_params,
                effect_instance.blend_mode,
                effect_instance.opacity,
                fixture_positions,
            );

            if !handled {
                if let EffectKind::Script(ref script_name) = effect_instance.kind {
                    if let Some(compiled) =
                        ctx.script_cache.and_then(|cache| cache.get(script_name))
                    {
                        effects::script::evaluate_pixels_batch(
                            compiled,
                            t_normalized,
                            t,
                            pixels,
                            global_pixel_offset,
                            total_pixels,
                            &resolved_params,
                            effect_instance.blend_mode,
                            effect_instance.opacity,
                            fixture_positions,
                            Some(ctx.motion_path_lib),
                        );
                    }
                }
            }

            global_pixel_offset += pixel_count;
        }
    }

    // Suppress unused variable warning for node_id (used for debugging context).
    let _ = node_id;
}

/// Build a flat position vector for spatial effects across all target fixtures.
fn build_spatial_positions(
    target_fixtures: &[FixtureId],
    pixel_counts: &HashMap<FixtureId, usize>,
    layout_map: &HashMap<FixtureId, &[Position2D]>,
    total_pixels: usize,
) -> Vec<Position2D> {
    let mut pos_vec = Vec::with_capacity(total_pixels);
    for &fid in target_fixtures {
        let pc = pixel_counts.get(&fid).copied().unwrap_or(0);
        if let Some(positions) = layout_map.get(&fid) {
            if positions.len() == pc {
                pos_vec.extend_from_slice(positions);
            } else {
                if let Ok(mut warned) = LAYOUT_WARNED.lock() {
                    if warned.insert(fid) {
                        log::warn!(
                            "Layout mismatch for fixture {:?}: expected {} positions, got {} — using fallback",
                            fid, pc, positions.len()
                        );
                    }
                }
                push_fallback_positions(&mut pos_vec, pc);
            }
        } else {
            push_fallback_positions(&mut pos_vec, pc);
        }
    }
    pos_vec
}

/// Encode a fixture-color map into a flat frame buffer for preview and output.
fn encode_frame(
    show: &Show,
    frame: HashMap<FixtureId, Vec<Color>>,
    warnings: Vec<String>,
) -> Frame {
    let fixture_map: HashMap<FixtureId, &crate::model::FixtureDef> = show
        .fixtures
        .iter()
        .map(|fixture| (fixture.id, fixture))
        .collect();
    let mut layout_fixture_ids = HashSet::new();
    let total_pixels: usize = show
        .fixtures
        .iter()
        .map(|fixture| fixture.pixel_count as usize)
        .sum();
    let mut pixels = Vec::with_capacity(total_pixels * 4);
    let mut fixture_spans = Vec::with_capacity(show.fixtures.len());

    for layout_fixture in &show.layout.fixtures {
        let Some(fixture) = fixture_map.get(&layout_fixture.fixture_id) else {
            continue;
        };
        layout_fixture_ids.insert(layout_fixture.fixture_id);
        let start_pixel = pixels.len() / 4;
        let pixel_count = fixture.pixel_count as usize;
        if let Some(colors) = frame.get(&layout_fixture.fixture_id) {
            extend_with_colors(&mut pixels, colors);
        } else {
            extend_with_black(&mut pixels, pixel_count);
        }
        fixture_spans.push(FrameFixtureSpan {
            fixture_id: layout_fixture.fixture_id.0,
            start_pixel,
            pixel_count,
        });
    }

    for fixture in &show.fixtures {
        if layout_fixture_ids.contains(&fixture.id) {
            continue;
        }
        let start_pixel = pixels.len() / 4;
        let pixel_count = fixture.pixel_count as usize;
        if let Some(colors) = frame.get(&fixture.id) {
            extend_with_colors(&mut pixels, colors);
        } else {
            extend_with_black(&mut pixels, pixel_count);
        }
        fixture_spans.push(FrameFixtureSpan {
            fixture_id: fixture.id.0,
            start_pixel,
            pixel_count,
        });
    }

    Frame {
        pixels,
        fixture_spans,
        warnings: if warnings.is_empty() {
            None
        } else {
            Some(warnings)
        },
    }
}

/// Evaluate the full show at a given time, producing a flat frame buffer.
///
/// Pipeline:
/// 1. Start with all fixtures at BLACK
/// 2. Evaluate fixture-level node timelines first (leaf nodes)
/// 3. Then evaluate group-level node timelines (effects cascade to descendants)
/// 4. Flatten fixture colors into one contiguous RGBA buffer
///
/// If `effect_filter` is provided, only effects with matching IDs are evaluated.
#[allow(clippy::cast_precision_loss, clippy::implicit_hasher)]
pub fn evaluate(
    show: &Show,
    sequence_index: usize,
    t: f64,
    effect_filter: Option<&[EffectId]>,
    script_cache: Option<&IndexMap<String, Arc<CompiledScript>>>,
    gradient_lib: &HashMap<String, ColorGradient>,
) -> Frame {
    let Some(sequence) = show.sequences.get(sequence_index) else {
        return encode_frame(
            show,
            HashMap::new(),
            vec![format!(
                "Sequence not found (index {sequence_index}, show has {})",
                show.sequences.len()
            )],
        );
    };

    if sequence.node_timelines.is_empty() {
        return encode_frame(
            show,
            HashMap::new(),
            vec!["No node timelines in sequence".to_string()],
        );
    }

    let ctx = EvalContext::new(show, sequence, script_cache, gradient_lib);

    let mut frame: HashMap<FixtureId, Vec<Color>> = HashMap::new();
    let warnings: Vec<String> = Vec::new();

    // Evaluate fixture-level timelines first (leaf nodes).
    for (node_id, timeline) in &sequence.node_timelines {
        if matches!(node_id, NodeId::Fixture(_)) {
            let fixtures = ctx.resolve_node_fixtures(node_id);
            evaluate_node_timeline(
                node_id,
                timeline,
                fixtures,
                t,
                effect_filter,
                &ctx,
                &mut frame,
            );
        }
    }

    // Then evaluate group-level timelines (effects cascade to all descendant fixtures).
    // Groups are evaluated after fixtures so group effects layer on top.
    for (node_id, timeline) in &sequence.node_timelines {
        if matches!(node_id, NodeId::Group(_)) {
            let fixtures = ctx.resolve_node_fixtures(node_id);
            evaluate_node_timeline(
                node_id,
                timeline,
                fixtures,
                t,
                effect_filter,
                &ctx,
                &mut frame,
            );
        }
    }

    encode_frame(show, frame, warnings)
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation
)]
#[path = "evaluator_tests.rs"]
mod evaluator_tests;
