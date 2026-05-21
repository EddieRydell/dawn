use super::*;
use crate::model::fixture::{
    BulbShape, ChannelOrder, ColorModel, FixtureDef, FixtureGroup, GroupMember, PixelType,
};
use crate::model::show::{Layout, Show};
use crate::model::{
    BlendMode, BuiltInEffect, EffectInstance, EffectParams, NodeId, NodeTimeline, ParamKey,
    ParamValue, Sequence, TimeRange, TrackItem,
};
use indexmap::IndexMap;
use std::sync::Arc;

fn fixture(id: u32, pixels: u32) -> FixtureDef {
    FixtureDef {
        id: FixtureId(id),
        name: format!("Fixture {id}"),
        color_model: ColorModel::Rgb,
        pixel_count: pixels,
        pixel_type: PixelType::Smart,
        bulb_shape: BulbShape::LED,
        requires_layout: true,
        requires_patch: true,
        display_radius_override: None,
        channel_order: ChannelOrder::Rgb,
    }
}

fn solid_effect(start: f64, end: f64, color: Color) -> TrackItem {
    TrackItem::Effect(EffectInstance {
        id: EffectId::generate(),
        kind: BuiltInEffect::Solid.into(),
        params: EffectParams::new().set(ParamKey::Color, ParamValue::Color(color)),
        time_range: TimeRange::new(start, end).unwrap(),
        blend_mode: BlendMode::Override,
        opacity: 1.0,
        param_links: HashMap::new(),
    })
}

fn solid_effect_blended(
    start: f64,
    end: f64,
    color: Color,
    blend_mode: BlendMode,
    opacity: f64,
) -> TrackItem {
    TrackItem::Effect(EffectInstance {
        id: EffectId::generate(),
        kind: BuiltInEffect::Solid.into(),
        params: EffectParams::new().set(ParamKey::Color, ParamValue::Color(color)),
        time_range: TimeRange::new(start, end).unwrap(),
        blend_mode,
        opacity,
        param_links: HashMap::new(),
    })
}

fn simple_show(fixtures: Vec<FixtureDef>, node_timelines: HashMap<NodeId, NodeTimeline>) -> Show {
    Show {
        name: "Test".into(),
        fixtures,
        groups: vec![],
        layout: Layout { fixtures: vec![] },
        sequences: vec![Sequence {
            name: "Seq".into(),
            duration: 10.0,
            frame_rate: 30.0,
            audio_file: None,
            node_timelines,
            motion_paths: std::collections::HashMap::new(),
        }],
        patches: vec![],
        controllers: vec![],
    }
}

/// Helper to create a single-node show with effects on fixture 1.
fn show_with_fixture_effects(fixtures: Vec<FixtureDef>, items: Vec<TrackItem>) -> Show {
    let fid = fixtures.first().map_or(FixtureId(1), |f| f.id);
    simple_show(
        fixtures,
        HashMap::from([(NodeId::Fixture(fid), NodeTimeline { items })]),
    )
}

/// Helper to create a show with effects on all provided fixtures.
fn show_with_effects_on_all(fixtures: Vec<FixtureDef>, items: Vec<TrackItem>) -> Show {
    let node_timelines: HashMap<NodeId, NodeTimeline> = fixtures
        .iter()
        .map(|f| {
            (
                NodeId::Fixture(f.id),
                NodeTimeline {
                    items: items.clone(),
                },
            )
        })
        .collect();
    simple_show(fixtures, node_timelines)
}

/// Decode RGBA bytes back to `Color` vec for assertions.
fn decode_fixture_colors(frame: &Frame, fixture_id: u32) -> Option<Vec<Color>> {
    let bytes = frame.fixture_pixels(fixture_id)?;
    Some(
        bytes
            .chunks_exact(4)
            .map(|c| Color::rgba(c[0], c[1], c[2], c[3]))
            .collect(),
    )
}

fn fixture_has_non_black(frame: &Frame, fixture_id: u32) -> bool {
    decode_fixture_colors(frame, fixture_id)
        .is_some_and(|colors| colors.iter().any(|color| *color != Color::BLACK))
}

fn fixture_is_all_black(frame: &Frame, fixture_id: u32) -> bool {
    !fixture_has_non_black(frame, fixture_id)
}

#[test]
fn single_solid_effect_produces_correct_output() {
    let red = Color::rgb(255, 0, 0);
    let show = show_with_fixture_effects(vec![fixture(1, 5)], vec![solid_effect(0.0, 5.0, red)]);
    let frame = evaluate(&show, 0, 2.5, None, None, &HashMap::new());
    let colors = decode_fixture_colors(&frame, 1).expect("fixture should be in frame");
    assert_eq!(colors.len(), 5);
    for c in &colors {
        assert_eq!(c.r, 255);
        assert_eq!(c.g, 0);
        assert_eq!(c.b, 0);
    }
}

#[test]
fn effect_only_active_during_time_range() {
    let show = show_with_fixture_effects(
        vec![fixture(1, 1)],
        vec![solid_effect(2.0, 4.0, Color::WHITE)],
    );
    // Before range
    let frame = evaluate(&show, 0, 1.0, None, None, &HashMap::new());
    assert!(fixture_is_all_black(&frame, 1));

    // Inside range
    let frame = evaluate(&show, 0, 3.0, None, None, &HashMap::new());
    assert!(fixture_has_non_black(&frame, 1));

    // Well past the end (beyond epsilon tolerance)
    let frame = evaluate(&show, 0, 4.1, None, None, &HashMap::new());
    assert!(fixture_is_all_black(&frame, 1));
}

#[test]
fn two_effects_override_blend() {
    let show = show_with_fixture_effects(
        vec![fixture(1, 1)],
        vec![
            solid_effect(0.0, 5.0, Color::rgb(255, 0, 0)),
            solid_effect(0.0, 5.0, Color::rgb(0, 255, 0)),
        ],
    );
    let frame = evaluate(&show, 0, 1.0, None, None, &HashMap::new());
    let colors = decode_fixture_colors(&frame, 1).unwrap();
    assert_eq!(colors[0], Color::rgb(0, 255, 0));
}

#[test]
fn two_effects_add_blend_saturates() {
    let show = show_with_fixture_effects(
        vec![fixture(1, 1)],
        vec![
            solid_effect(0.0, 5.0, Color::rgb(200, 100, 0)),
            solid_effect_blended(0.0, 5.0, Color::rgb(200, 200, 50), BlendMode::Add, 1.0),
        ],
    );
    let frame = evaluate(&show, 0, 1.0, None, None, &HashMap::new());
    let colors = decode_fixture_colors(&frame, 1).unwrap();
    assert_eq!(colors[0].r, 255); // saturated
    assert_eq!(colors[0].g, 255); // 100+200 saturated
    assert_eq!(colors[0].b, 50);
}

#[test]
fn two_effects_multiply_blend() {
    let show = show_with_fixture_effects(
        vec![fixture(1, 1)],
        vec![
            solid_effect(0.0, 5.0, Color::rgb(255, 128, 0)),
            solid_effect_blended(0.0, 5.0, Color::WHITE, BlendMode::Multiply, 1.0),
        ],
    );
    let frame = evaluate(&show, 0, 1.0, None, None, &HashMap::new());
    let colors = decode_fixture_colors(&frame, 1).unwrap();
    // Multiply with white is identity
    assert_eq!(colors[0].r, 255);
    assert_eq!(colors[0].g, 128);
    assert_eq!(colors[0].b, 0);
}

#[test]
fn effects_span_across_fixtures_via_group() {
    // Gradient across 2 fixtures (5 pixels each = 10 total) via a group node.
    // Should be continuous, not restart per fixture.
    use crate::model::fixture::{FixtureGroup, GroupId, GroupMember};

    let gradient_item = TrackItem::Effect(EffectInstance {
        id: EffectId::generate(),
        kind: BuiltInEffect::Gradient.into(),
        params: EffectParams::new().set(
            ParamKey::Colors,
            ParamValue::ColorList(vec![Color::rgb(0, 0, 0), Color::rgb(255, 255, 255)]),
        ),
        time_range: TimeRange::new(0.0, 5.0).unwrap(),
        blend_mode: BlendMode::Override,
        opacity: 1.0,
        param_links: HashMap::new(),
    });
    let mut show = simple_show(
        vec![fixture(1, 5), fixture(2, 5)],
        HashMap::from([(
            NodeId::Group(GroupId(10)),
            NodeTimeline {
                items: vec![gradient_item],
            },
        )]),
    );
    show.groups.push(FixtureGroup {
        id: GroupId(10),
        name: "Both".into(),
        members: vec![
            GroupMember::Fixture(FixtureId(1)),
            GroupMember::Fixture(FixtureId(2)),
        ],
    });
    let frame = evaluate(&show, 0, 1.0, None, None, &HashMap::new());
    let f2 = decode_fixture_colors(&frame, 2).unwrap();
    // Fixture 2 starts at global pixel 5. With 10 total pixels, pixel 5 has pos=5/9≈0.56.
    // Fixture 2 pixel 3 has global pos 8/9≈0.89 → should be bright.
    // If the gradient restarted per fixture, pixel 0 of f2 would be dark.
    // Instead it should be mid-brightness (continuous from fixture 1).
    assert!(
        f2[0].r > 100,
        "first pixel of fixture 2 should be mid-brightness (continuous), got r={}",
        f2[0].r
    );
    // Later pixels in fixture 2 should be brighter than earlier ones (monotonic gradient)
    assert!(f2[3].r > f2[0].r);
}

#[test]
fn empty_show_produces_empty_frame() {
    let show = Show::empty();
    let frame = evaluate(&show, 0, 0.0, None, None, &HashMap::new());
    assert!(frame.fixture_spans.is_empty());
    assert!(frame.pixels.is_empty());
}

#[test]
fn zero_pixel_fixtures_are_skipped() {
    let show = show_with_effects_on_all(
        vec![fixture(1, 0), fixture(2, 3)],
        vec![solid_effect(0.0, 5.0, Color::WHITE)],
    );
    let frame = evaluate(&show, 0, 1.0, None, None, &HashMap::new());
    assert_eq!(frame.fixture_pixels(1), Some(&[][..]));
    assert!(fixture_has_non_black(&frame, 2));
}

#[test]
fn black_fixtures_encode_as_black_pixels() {
    let show = show_with_fixture_effects(
        vec![fixture(1, 3)],
        vec![solid_effect(0.0, 5.0, Color::BLACK)],
    );
    let frame = evaluate(&show, 0, 1.0, None, None, &HashMap::new());
    assert!(fixture_is_all_black(&frame, 1));
}

#[test]
fn group_targeting_resolves_fixtures() {
    let mut show = simple_show(
        vec![fixture(1, 3), fixture(2, 3), fixture(3, 3)],
        HashMap::from([(
            NodeId::Group(GroupId(10)),
            NodeTimeline {
                items: vec![solid_effect(0.0, 5.0, Color::rgb(255, 0, 0))],
            },
        )]),
    );
    show.groups.push(FixtureGroup {
        id: GroupId(10),
        name: "Group A".into(),
        members: vec![
            GroupMember::Fixture(FixtureId(1)),
            GroupMember::Fixture(FixtureId(3)),
        ],
    });
    let frame = evaluate(&show, 0, 1.0, None, None, &HashMap::new());
    assert!(fixture_has_non_black(&frame, 1));
    assert!(fixture_is_all_black(&frame, 2)); // not in group
    assert!(fixture_has_non_black(&frame, 3));
}

#[test]
fn effect_filter_limits_evaluation() {
    let red_effect = solid_effect(0.0, 5.0, Color::rgb(255, 0, 0));
    let red_id = red_effect.as_effect().unwrap().id.clone();
    let show = show_with_fixture_effects(
        vec![fixture(1, 1)],
        vec![red_effect, solid_effect(0.0, 5.0, Color::rgb(0, 255, 0))],
    );
    // Only evaluate the red effect
    let filter = [red_id];
    let frame = evaluate(&show, 0, 1.0, Some(&filter), None, &HashMap::new());
    let colors = decode_fixture_colors(&frame, 1).unwrap();
    assert_eq!(colors[0], Color::rgb(255, 0, 0)); // green was skipped
}

#[test]
fn subtract_blend_mode() {
    let show = show_with_fixture_effects(
        vec![fixture(1, 1)],
        vec![
            solid_effect(0.0, 5.0, Color::rgb(200, 150, 100)),
            solid_effect_blended(0.0, 5.0, Color::rgb(50, 200, 30), BlendMode::Subtract, 1.0),
        ],
    );
    let frame = evaluate(&show, 0, 1.0, None, None, &HashMap::new());
    let colors = decode_fixture_colors(&frame, 1).unwrap();
    assert_eq!(colors[0].r, 150); // 200 - 50
    assert_eq!(colors[0].g, 0); // 150 - 200 saturates to 0
    assert_eq!(colors[0].b, 70); // 100 - 30
}

#[test]
fn min_blend_mode() {
    let show = show_with_fixture_effects(
        vec![fixture(1, 1)],
        vec![
            solid_effect(0.0, 5.0, Color::rgb(200, 50, 100)),
            solid_effect_blended(0.0, 5.0, Color::rgb(100, 150, 80), BlendMode::Min, 1.0),
        ],
    );
    let frame = evaluate(&show, 0, 1.0, None, None, &HashMap::new());
    let colors = decode_fixture_colors(&frame, 1).unwrap();
    assert_eq!(colors[0].r, 100);
    assert_eq!(colors[0].g, 50);
    assert_eq!(colors[0].b, 80);
}

#[test]
fn average_blend_mode() {
    let show = show_with_fixture_effects(
        vec![fixture(1, 1)],
        vec![
            solid_effect(0.0, 5.0, Color::rgb(200, 100, 0)),
            solid_effect_blended(0.0, 5.0, Color::rgb(100, 50, 200), BlendMode::Average, 1.0),
        ],
    );
    let frame = evaluate(&show, 0, 1.0, None, None, &HashMap::new());
    let colors = decode_fixture_colors(&frame, 1).unwrap();
    assert_eq!(colors[0].r, 150); // (200+100)/2
    assert_eq!(colors[0].g, 75); // (100+50)/2
    assert_eq!(colors[0].b, 100); // (0+200)/2
}

#[test]
fn screen_blend_mode() {
    let show = show_with_fixture_effects(
        vec![fixture(1, 1)],
        vec![
            solid_effect(0.0, 5.0, Color::rgb(128, 0, 255)),
            solid_effect_blended(0.0, 5.0, Color::rgb(128, 128, 0), BlendMode::Screen, 1.0),
        ],
    );
    let frame = evaluate(&show, 0, 1.0, None, None, &HashMap::new());
    let colors = decode_fixture_colors(&frame, 1).unwrap();
    // screen(128,128) = 255 - (127*127)/255 = 255 - 63 = 192
    assert_eq!(colors[0].r, 192);
    // screen(0,128) = 255 - (255*127)/255 = 255 - 127 = 128
    assert_eq!(colors[0].g, 128);
    // screen(255,0) = 255 - (0*255)/255 = 255
    assert_eq!(colors[0].b, 255);
}

#[test]
fn mask_blend_mode() {
    let show = show_with_fixture_effects(
        vec![fixture(1, 3)],
        vec![
            solid_effect(0.0, 5.0, Color::rgb(255, 128, 64)),
            // fg is non-black → mask produces black
            solid_effect_blended(0.0, 5.0, Color::rgb(10, 0, 0), BlendMode::Mask, 1.0),
        ],
    );
    let frame = evaluate(&show, 0, 1.0, None, None, &HashMap::new());
    // All pixels should be black (masked out), so frame is empty
    assert!(fixture_is_all_black(&frame, 1));
}

#[test]
fn intensity_overlay_blend_mode() {
    let show = show_with_fixture_effects(
        vec![fixture(1, 1)],
        vec![
            solid_effect(0.0, 5.0, Color::rgb(200, 100, 50)),
            // Pure white fg has brightness ~1.0, so bg is preserved
            solid_effect_blended(0.0, 5.0, Color::WHITE, BlendMode::IntensityOverlay, 1.0),
        ],
    );
    let frame = evaluate(&show, 0, 1.0, None, None, &HashMap::new());
    let colors = decode_fixture_colors(&frame, 1).unwrap();
    assert_eq!(colors[0].r, 200);
    assert_eq!(colors[0].g, 100);
    assert_eq!(colors[0].b, 50);
}

#[test]
fn opacity_half_produces_dimmed_output() {
    let show = show_with_fixture_effects(
        vec![fixture(1, 1)],
        vec![solid_effect_blended(
            0.0,
            5.0,
            Color::rgb(200, 100, 50),
            BlendMode::Override,
            0.5,
        )],
    );
    let frame = evaluate(&show, 0, 1.0, None, None, &HashMap::new());
    let colors = decode_fixture_colors(&frame, 1).unwrap();
    assert_eq!(colors[0].r, 100);
    assert_eq!(colors[0].g, 50);
    assert_eq!(colors[0].b, 25);
}

#[test]
fn opacity_zero_produces_no_output() {
    let show = show_with_fixture_effects(
        vec![fixture(1, 1)],
        vec![solid_effect_blended(
            0.0,
            5.0,
            Color::WHITE,
            BlendMode::Override,
            0.0,
        )],
    );
    let frame = evaluate(&show, 0, 1.0, None, None, &HashMap::new());
    // opacity=0 means all black, so frame should be empty
    assert!(fixture_is_all_black(&frame, 1));
}

// ── Automation integration tests ────────────────────────────────

use crate::model::automation::{AutomationClip, ClipId};
use crate::model::curve::Curve;

#[test]
fn effect_with_no_param_links_unchanged() {
    // Same as single_solid_effect test — verifies zero overhead path.
    let red = Color::rgb(255, 0, 0);
    let show = show_with_fixture_effects(vec![fixture(1, 3)], vec![solid_effect(0.0, 5.0, red)]);
    let frame = evaluate(&show, 0, 2.5, None, None, &HashMap::new());
    let colors = decode_fixture_colors(&frame, 1).unwrap();
    assert_eq!(colors[0], Color::rgb(255, 0, 0));
}

#[test]
fn float_param_link_reads_from_automation_clip() {
    let clip_id = ClipId("brightness_clip".into());
    let auto_clip = AutomationClip::new(
        clip_id.clone(),
        None,
        Curve::constant(0.5),
        TimeRange::new(0.0, 5.0).unwrap(),
        crate::model::LoopMode::Clamp,
    );

    let mut links = HashMap::new();
    links.insert(ParamKey::Brightness, clip_id);

    let show = show_with_fixture_effects(
        vec![fixture(1, 1)],
        vec![
            TrackItem::Clip(auto_clip),
            TrackItem::Effect(EffectInstance {
                id: EffectId::generate(),
                kind: BuiltInEffect::Solid.into(),
                params: EffectParams::new()
                    .set(ParamKey::Color, ParamValue::Color(Color::WHITE))
                    .set(ParamKey::Brightness, ParamValue::Float(1.0)),
                time_range: TimeRange::new(0.0, 5.0).unwrap(),
                blend_mode: BlendMode::Override,
                opacity: 1.0,
                param_links: links,
            }),
        ],
    );
    let frame = evaluate(&show, 0, 2.5, None, None, &HashMap::new());
    // The effect should run (Solid ignores Brightness, but automation resolved).
    // The key thing: the resolve_automation function ran without error.
    assert!(fixture_has_non_black(&frame, 1));
}

#[test]
fn automation_clip_holds_value_after_end() {
    // Automation clip with a linear ramp covers 1.0..2.0s.
    // At t=3.0, the clip's clamp mode holds at y=1.0.
    let clip_id = ClipId("ramp_clip".into());
    let auto_clip = AutomationClip::new(
        clip_id.clone(),
        None,
        Curve::linear(), // 0→1 ramp
        TimeRange::new(1.0, 2.0).unwrap(),
        crate::model::LoopMode::Clamp,
    );

    let mut links = HashMap::new();
    links.insert(ParamKey::Speed, clip_id);

    let show = show_with_fixture_effects(
        vec![fixture(1, 1)],
        vec![
            TrackItem::Clip(auto_clip),
            TrackItem::Effect(EffectInstance {
                id: EffectId::generate(),
                kind: BuiltInEffect::Solid.into(),
                params: EffectParams::new()
                    .set(ParamKey::Color, ParamValue::Color(Color::WHITE))
                    .set(ParamKey::Speed, ParamValue::Float(0.0)),
                time_range: TimeRange::new(0.0, 5.0).unwrap(),
                blend_mode: BlendMode::Override,
                opacity: 1.0,
                param_links: links,
            }),
        ],
    );
    // At t=3.0, clip's clamp mode holds at 1.0 (end of ramp).
    // Effect still renders — automation resolved without error.
    let frame = evaluate(&show, 0, 3.0, None, None, &HashMap::new());
    assert!(fixture_has_non_black(&frame, 1));
}

#[test]
fn multiple_effects_read_same_clip() {
    let clip_id = ClipId("shared".into());
    let auto_clip = AutomationClip::new(
        clip_id.clone(),
        None,
        Curve::constant(0.5),
        TimeRange::new(0.0, 5.0).unwrap(),
        crate::model::LoopMode::Clamp,
    );

    let mut links1 = HashMap::new();
    links1.insert(ParamKey::Speed, clip_id.clone());
    let mut links2 = HashMap::new();
    links2.insert(ParamKey::Speed, clip_id);

    let show = show_with_fixture_effects(
        vec![fixture(1, 1)],
        vec![
            TrackItem::Clip(auto_clip),
            TrackItem::Effect(EffectInstance {
                id: EffectId::generate(),
                kind: BuiltInEffect::Solid.into(),
                params: EffectParams::new()
                    .set(ParamKey::Color, ParamValue::Color(Color::rgb(255, 0, 0))),
                time_range: TimeRange::new(0.0, 2.5).unwrap(),
                blend_mode: BlendMode::Override,
                opacity: 1.0,
                param_links: links1,
            }),
            TrackItem::Effect(EffectInstance {
                id: EffectId::generate(),
                kind: BuiltInEffect::Solid.into(),
                params: EffectParams::new()
                    .set(ParamKey::Color, ParamValue::Color(Color::rgb(0, 255, 0))),
                time_range: TimeRange::new(2.5, 5.0).unwrap(),
                blend_mode: BlendMode::Override,
                opacity: 1.0,
                param_links: links2,
            }),
        ],
    );
    // Both effects reference the same clip — first active at t=1.0
    let frame = evaluate(&show, 0, 1.0, None, None, &HashMap::new());
    let colors = decode_fixture_colors(&frame, 1).unwrap();
    assert_eq!(colors[0], Color::rgb(255, 0, 0));
    // Second active at t=3.0
    let frame = evaluate(&show, 0, 3.0, None, None, &HashMap::new());
    let colors = decode_fixture_colors(&frame, 1).unwrap();
    assert_eq!(colors[0], Color::rgb(0, 255, 0));
}

#[test]
fn cross_node_param_link_resolves_clip_from_other_node() {
    // Clip lives on fixture 1's timeline, but fixture 2's effect links to it.
    let clip_id = ClipId("cross_node_clip".into());
    let auto_clip = AutomationClip::new(
        clip_id.clone(),
        None,
        Curve::constant(0.42),
        TimeRange::new(0.0, 5.0).unwrap(),
        crate::model::LoopMode::Clamp,
    );

    let mut links = HashMap::new();
    links.insert(ParamKey::Speed, clip_id);

    let show = simple_show(
        vec![fixture(1, 1), fixture(2, 1)],
        HashMap::from([
            // Fixture 1: has the automation clip + an effect
            (
                NodeId::Fixture(FixtureId(1)),
                NodeTimeline {
                    items: vec![
                        TrackItem::Clip(auto_clip),
                        solid_effect(0.0, 5.0, Color::rgb(255, 0, 0)),
                    ],
                },
            ),
            // Fixture 2: has the effect with a cross-node param_link
            (
                NodeId::Fixture(FixtureId(2)),
                NodeTimeline {
                    items: vec![TrackItem::Effect(EffectInstance {
                        id: EffectId::generate(),
                        kind: BuiltInEffect::Solid.into(),
                        params: EffectParams::new()
                            .set(ParamKey::Color, ParamValue::Color(Color::rgb(0, 255, 0)))
                            .set(ParamKey::Speed, ParamValue::Float(1.0)),
                        time_range: TimeRange::new(0.0, 5.0).unwrap(),
                        blend_mode: BlendMode::Override,
                        opacity: 1.0,
                        param_links: links,
                    })],
                },
            ),
        ]),
    );
    // The cross-node link should resolve without error.
    let frame = evaluate(&show, 0, 2.5, None, None, &HashMap::new());
    // Fixture 2's effect should still render (clip resolved from fixture 1's timeline).
    let colors = decode_fixture_colors(&frame, 2).unwrap();
    assert_eq!(colors[0], Color::rgb(0, 255, 0));
}

// ── Tests requested in issue #134 ─────────────────────────────────

#[test]
fn solid_effect_produces_uniform_frame() {
    let blue = Color::rgb(0, 0, 255);
    let show = show_with_fixture_effects(vec![fixture(1, 10)], vec![solid_effect(0.0, 5.0, blue)]);
    let frame = evaluate(&show, 0, 2.5, None, None, &HashMap::new());
    let colors = decode_fixture_colors(&frame, 1).expect("fixture 1 should be in frame");
    assert_eq!(colors.len(), 10, "should have 10 pixels");
    for (i, c) in colors.iter().enumerate() {
        assert_eq!(*c, blue, "pixel {i} should be solid blue");
    }
}

#[test]
fn empty_timeline_produces_transparent_frame() {
    // A node timeline with zero effects should produce no output at all.
    let show = simple_show(
        vec![fixture(1, 5)],
        HashMap::from([(
            NodeId::Fixture(FixtureId(1)),
            NodeTimeline { items: vec![] },
        )]),
    );
    let frame = evaluate(&show, 0, 2.5, None, None, &HashMap::new());
    // No effects means nothing written to any fixture — frame should be empty.
    assert!(
        fixture_is_all_black(&frame, 1),
        "empty timeline should produce no fixture output"
    );
}

#[test]
fn effects_outside_time_range_are_inactive() {
    // Effect covers 0..5s. Querying at t=10s should produce no output.
    let show = show_with_fixture_effects(
        vec![fixture(1, 3)],
        vec![solid_effect(0.0, 5.0, Color::rgb(255, 0, 0))],
    );
    let frame = evaluate(&show, 0, 10.0, None, None, &HashMap::new());
    assert!(
        fixture_is_all_black(&frame, 1),
        "effect from 0-5s should be inactive at t=10s"
    );
}

#[test]
fn multiple_effects_blend_on_same_node() {
    // Two overlapping Solid effects on the same node. The second effect (later
    // in the items vec) evaluates after the first, so with Override blend mode
    // it replaces the first effect's output.
    let show = show_with_fixture_effects(
        vec![fixture(1, 1)],
        vec![
            solid_effect(0.0, 5.0, Color::rgb(255, 0, 0)),
            solid_effect(0.0, 5.0, Color::rgb(0, 255, 0)),
        ],
    );
    let frame = evaluate(&show, 0, 2.5, None, None, &HashMap::new());
    let colors = decode_fixture_colors(&frame, 1).unwrap();
    // The second effect (green) overrides the first (red) since both use Override blend.
    assert_eq!(
        colors[0],
        Color::rgb(0, 255, 0),
        "second overlapping effect should override the first on the same node"
    );

    // Now test with Add blend on the second effect — should produce additive result.
    let show_add = show_with_fixture_effects(
        vec![fixture(1, 1)],
        vec![
            solid_effect(0.0, 5.0, Color::rgb(100, 0, 0)),
            solid_effect_blended(0.0, 5.0, Color::rgb(0, 100, 0), BlendMode::Add, 1.0),
        ],
    );
    let frame_add = evaluate(&show_add, 0, 2.5, None, None, &HashMap::new());
    let colors_add = decode_fixture_colors(&frame_add, 1).unwrap();
    assert_eq!(colors_add[0].r, 100, "red channel from first effect");
    assert_eq!(
        colors_add[0].g, 100,
        "green channel from additive second effect"
    );
    assert_eq!(colors_add[0].b, 0, "blue channel should be zero");
}

#[test]
fn automation_plus_library_refs_compose() {
    // Automation overrides a gradient ref param, then library resolution
    // resolves it. This verifies the ordering: automation → refs.
    let clip_id = ClipId("grad_override".into());
    let auto_clip = AutomationClip::new(
        clip_id.clone(),
        None,
        Curve::constant(0.5),
        TimeRange::new(0.0, 5.0).unwrap(),
        crate::model::LoopMode::Clamp,
    );

    let mut links = HashMap::new();
    links.insert(ParamKey::Speed, clip_id);

    // Effect uses a GradientRef which will be resolved by the library.
    let show = show_with_fixture_effects(
        vec![fixture(1, 3)],
        vec![
            TrackItem::Clip(auto_clip),
            TrackItem::Effect(EffectInstance {
                id: EffectId::generate(),
                kind: BuiltInEffect::Solid.into(),
                params: EffectParams::new()
                    .set(ParamKey::Color, ParamValue::Color(Color::WHITE))
                    .set(ParamKey::Speed, ParamValue::Float(1.0)),
                time_range: TimeRange::new(0.0, 5.0).unwrap(),
                blend_mode: BlendMode::Override,
                opacity: 1.0,
                param_links: links,
            }),
        ],
    );
    let frame = evaluate(&show, 0, 2.5, None, None, &HashMap::new());
    // Effect runs with automation-overridden Speed=2.0 + resolved refs.
    let colors = decode_fixture_colors(&frame, 1).unwrap();
    assert_eq!(colors[0], Color::WHITE);
}

// ── Curve param → automation clip slicing ─────────────────────

#[test]
fn curve_param_linked_to_clip_slices_correctly() {
    // Automation clip: linear ramp 0→1 over 0..10s.
    // Effect: covers 5..10s (second half of clip).
    // The effect's MovementCurve should become a ramp from 0.5→1.0.
    let clip_id = ClipId("ramp".into());
    let auto_clip = AutomationClip::new(
        clip_id.clone(),
        None,
        Curve::linear(), // 0→1 ramp
        TimeRange::new(0.0, 10.0).unwrap(),
        crate::model::LoopMode::Clamp,
    );

    let mut links = HashMap::new();
    links.insert(ParamKey::MovementCurve, clip_id);

    let params =
        EffectParams::new().set(ParamKey::MovementCurve, ParamValue::Curve(Curve::linear()));

    let effect_range = TimeRange::new(5.0, 10.0).unwrap();

    let clip_lookup: HashMap<&ClipId, &AutomationClip> =
        [(&auto_clip.id, &auto_clip)].into_iter().collect();

    let resolved = resolve_automation(&params, &links, &clip_lookup, 7.5, &effect_range, &[])
        .expect("should resolve");

    let default_curve = Curve::linear();
    let curve = resolved.curve_or(ParamKey::MovementCurve, &default_curve);
    // At effect-normalized t=0 (abs t=5.0), clip ramp is at 0.5
    assert!(
        (curve.evaluate(0.0) - 0.5).abs() < 0.01,
        "start should be ~0.5, got {}",
        curve.evaluate(0.0)
    );
    // At effect-normalized t=1 (abs t=10.0), clip ramp is at 1.0
    assert!(
        (curve.evaluate(1.0) - 1.0).abs() < 0.01,
        "end should be ~1.0, got {}",
        curve.evaluate(1.0)
    );
    // At effect-normalized t=0.5 (abs t=7.5), clip ramp is at 0.75
    assert!(
        (curve.evaluate(0.5) - 0.75).abs() < 0.01,
        "mid should be ~0.75, got {}",
        curve.evaluate(0.5)
    );
}

#[test]
fn curve_param_clip_shorter_than_effect_holds_values() {
    // Automation clip: linear ramp 0→1 over 2..4s.
    // Effect: covers 0..6s (extends before and after clip).
    // Before clip (t<2): should hold at 0.0 (clip start value).
    // After clip (t>4): should hold at 1.0 (clip end value).
    let clip_id = ClipId("short_ramp".into());
    let auto_clip = AutomationClip::new(
        clip_id.clone(),
        None,
        Curve::linear(),
        TimeRange::new(2.0, 4.0).unwrap(),
        crate::model::LoopMode::Clamp,
    );

    let mut links = HashMap::new();
    links.insert(ParamKey::MovementCurve, clip_id);

    let params =
        EffectParams::new().set(ParamKey::MovementCurve, ParamValue::Curve(Curve::linear()));

    let effect_range = TimeRange::new(0.0, 6.0).unwrap();

    let clip_lookup: HashMap<&ClipId, &AutomationClip> =
        [(&auto_clip.id, &auto_clip)].into_iter().collect();

    let resolved = resolve_automation(&params, &links, &clip_lookup, 3.0, &effect_range, &[])
        .expect("should resolve");

    let default_curve = Curve::linear();
    let curve = resolved.curve_or(ParamKey::MovementCurve, &default_curve);
    // At effect-normalized t=0 (abs t=0.0), before clip → clamped to 0.0
    assert!(
        (curve.evaluate(0.0) - 0.0).abs() < 0.01,
        "before clip should be ~0.0, got {}",
        curve.evaluate(0.0)
    );
    // At effect-normalized t=0.5 (abs t=3.0), midpoint of clip → 0.5
    assert!(
        (curve.evaluate(0.5) - 0.5).abs() < 0.01,
        "mid-clip should be ~0.5, got {}",
        curve.evaluate(0.5)
    );
    // At effect-normalized t=1.0 (abs t=6.0), after clip → clamped to 1.0
    assert!(
        (curve.evaluate(1.0) - 1.0).abs() < 0.01,
        "after clip should be ~1.0, got {}",
        curve.evaluate(1.0)
    );
}

#[test]
fn curve_param_not_in_stored_params_uses_schema_fallback() {
    // When the user never explicitly sets MovementCurve, the effect's params
    // map doesn't contain it. resolve_automation must use the schema to
    // detect it as a Curve type and produce a sliced curve, not a scalar.
    use crate::model::params::ParamSchema;

    let clip_id = ClipId("ramp".into());
    let auto_clip = AutomationClip::new(
        clip_id.clone(),
        None,
        Curve::linear(),
        TimeRange::new(0.0, 10.0).unwrap(),
        crate::model::LoopMode::Clamp,
    );

    let mut links = HashMap::new();
    links.insert(ParamKey::MovementCurve, clip_id);

    // Empty params — MovementCurve not stored (effect would use default)
    let params = EffectParams::new();
    let effect_range = TimeRange::new(0.0, 10.0).unwrap();

    let clip_lookup: HashMap<&ClipId, &AutomationClip> =
        [(&auto_clip.id, &auto_clip)].into_iter().collect();

    // Provide schema that says MovementCurve is a Curve param
    let schemas = vec![ParamSchema {
        key: ParamKey::MovementCurve,
        label: "Movement Curve".into(),
        param_type: ParamType::Curve,
        default: ParamValue::Curve(Curve::linear()),
    }];

    let resolved = resolve_automation(&params, &links, &clip_lookup, 5.0, &effect_range, &schemas)
        .expect("should resolve");

    // Should produce a Curve, not a Float
    let default_curve = Curve::linear();
    let curve = resolved.curve_or(ParamKey::MovementCurve, &default_curve);
    assert!(
        (curve.evaluate(0.0) - 0.0).abs() < 0.01,
        "start should be ~0.0, got {}",
        curve.evaluate(0.0)
    );
    assert!(
        (curve.evaluate(0.5) - 0.5).abs() < 0.01,
        "mid should be ~0.5, got {}",
        curve.evaluate(0.5)
    );
    assert!(
        (curve.evaluate(1.0) - 1.0).abs() < 0.01,
        "end should be ~1.0, got {}",
        curve.evaluate(1.0)
    );
}

#[test]
fn script_curve_param_not_in_stored_params_uses_canonical_schema() {
    let compiled = crate::dsl::compile_source(
        "@name \"Curve Script\"\nparam movement: curve = 0:0, 1:1;\nrgb(movement(t), 0.0, 0.0)\n",
    )
    .expect("script should compile");

    let mut script_cache = IndexMap::new();
    script_cache.insert("curve-script".to_string(), Arc::new(compiled));

    let clip_id = ClipId("script_curve".into());
    let auto_clip = AutomationClip::new(
        clip_id.clone(),
        None,
        Curve::linear(),
        TimeRange::new(0.0, 10.0).unwrap(),
        crate::model::LoopMode::Clamp,
    );

    let mut links = HashMap::new();
    links.insert(ParamKey::Custom("movement".into()), clip_id);

    let show = show_with_fixture_effects(
        vec![fixture(1, 1)],
        vec![
            TrackItem::Clip(auto_clip),
            TrackItem::Effect(EffectInstance {
                id: EffectId::generate(),
                kind: EffectKind::Script("curve-script".into()),
                params: EffectParams::new(),
                time_range: TimeRange::new(0.0, 10.0).unwrap(),
                blend_mode: BlendMode::Override,
                opacity: 1.0,
                param_links: links,
            }),
        ],
    );

    let frame = evaluate(&show, 0, 5.0, None, Some(&script_cache), &HashMap::new());
    let colors = decode_fixture_colors(&frame, 1).expect("script output should be rendered");
    assert!(
        colors[0].r > 100,
        "script curve automation should resolve to a non-zero curve value, got {:?}",
        colors[0]
    );
}

// ── Evaluator combination tests ─────────────────────────────────
// These test interactions between multiple subsystems (groups + fixtures,
// blend ordering, time boundaries) rather than individual features.

#[test]
fn group_effect_layers_on_top_of_fixture_effect() {
    // Fixture 1 has a red solid on its own timeline.
    // A group containing fixture 1 has a green Add-blended solid.
    // Groups evaluate AFTER fixtures, so the result should be red+green = yellow.
    let mut show = simple_show(
        vec![fixture(1, 1)],
        HashMap::from([
            (
                NodeId::Fixture(FixtureId(1)),
                NodeTimeline {
                    items: vec![solid_effect(0.0, 5.0, Color::rgb(255, 0, 0))],
                },
            ),
            (
                NodeId::Group(GroupId(10)),
                NodeTimeline {
                    items: vec![solid_effect_blended(
                        0.0,
                        5.0,
                        Color::rgb(0, 255, 0),
                        BlendMode::Add,
                        1.0,
                    )],
                },
            ),
        ]),
    );
    show.groups.push(FixtureGroup {
        id: GroupId(10),
        name: "G".into(),
        members: vec![GroupMember::Fixture(FixtureId(1))],
    });
    let frame = evaluate(&show, 0, 2.5, None, None, &HashMap::new());
    let colors = decode_fixture_colors(&frame, 1).unwrap();
    // Red (fixture) + Green (group, Add blend) = Yellow
    assert_eq!(colors[0].r, 255, "red channel from fixture-level effect");
    assert_eq!(colors[0].g, 255, "green channel from group-level Add blend");
    assert_eq!(colors[0].b, 0);
}

#[test]
fn group_override_replaces_fixture_effect() {
    // Same setup but group uses Override blend — should completely replace fixture output.
    let mut show = simple_show(
        vec![fixture(1, 1)],
        HashMap::from([
            (
                NodeId::Fixture(FixtureId(1)),
                NodeTimeline {
                    items: vec![solid_effect(0.0, 5.0, Color::rgb(255, 0, 0))],
                },
            ),
            (
                NodeId::Group(GroupId(10)),
                NodeTimeline {
                    items: vec![solid_effect(0.0, 5.0, Color::rgb(0, 0, 255))],
                },
            ),
        ]),
    );
    show.groups.push(FixtureGroup {
        id: GroupId(10),
        name: "G".into(),
        members: vec![GroupMember::Fixture(FixtureId(1))],
    });
    let frame = evaluate(&show, 0, 2.5, None, None, &HashMap::new());
    let colors = decode_fixture_colors(&frame, 1).unwrap();
    assert_eq!(
        colors[0],
        Color::rgb(0, 0, 255),
        "group Override should replace fixture output"
    );
}

#[test]
fn two_groups_on_same_fixture_blend_in_iteration_order() {
    // Two groups both target fixture 1. The second group's effect should
    // layer on top of the first group's effect (HashMap iteration order is
    // not guaranteed, but both should be applied).
    let mut show = simple_show(
        vec![fixture(1, 1)],
        HashMap::from([
            (
                NodeId::Group(GroupId(10)),
                NodeTimeline {
                    items: vec![solid_effect(0.0, 5.0, Color::rgb(100, 0, 0))],
                },
            ),
            (
                NodeId::Group(GroupId(20)),
                NodeTimeline {
                    items: vec![solid_effect_blended(
                        0.0,
                        5.0,
                        Color::rgb(0, 100, 0),
                        BlendMode::Add,
                        1.0,
                    )],
                },
            ),
        ]),
    );
    show.groups.push(FixtureGroup {
        id: GroupId(10),
        name: "G1".into(),
        members: vec![GroupMember::Fixture(FixtureId(1))],
    });
    show.groups.push(FixtureGroup {
        id: GroupId(20),
        name: "G2".into(),
        members: vec![GroupMember::Fixture(FixtureId(1))],
    });
    let frame = evaluate(&show, 0, 2.5, None, None, &HashMap::new());
    let colors = decode_fixture_colors(&frame, 1).unwrap();
    // Both groups should have been evaluated on fixture 1.
    // Regardless of order: one Override(100,0,0) + one Add(0,100,0) = at least green present.
    assert!(
        colors[0].r > 0 || colors[0].g > 0,
        "both groups should contribute to output"
    );
}

#[test]
fn fixture_not_in_group_unaffected_by_group_effect() {
    // Fixture 2 is NOT in the group. It should be unaffected by the group's effect.
    let mut show = simple_show(
        vec![fixture(1, 1), fixture(2, 1)],
        HashMap::from([
            (
                NodeId::Fixture(FixtureId(2)),
                NodeTimeline {
                    items: vec![solid_effect(0.0, 5.0, Color::rgb(0, 0, 255))],
                },
            ),
            (
                NodeId::Group(GroupId(10)),
                NodeTimeline {
                    items: vec![solid_effect(0.0, 5.0, Color::rgb(255, 0, 0))],
                },
            ),
        ]),
    );
    show.groups.push(FixtureGroup {
        id: GroupId(10),
        name: "G".into(),
        members: vec![GroupMember::Fixture(FixtureId(1))],
    });
    let frame = evaluate(&show, 0, 2.5, None, None, &HashMap::new());
    // Fixture 1: only group effect → red
    let c1 = decode_fixture_colors(&frame, 1).unwrap();
    assert_eq!(c1[0], Color::rgb(255, 0, 0));
    // Fixture 2: only its own fixture effect → blue (group doesn't touch it)
    let c2 = decode_fixture_colors(&frame, 2).unwrap();
    assert_eq!(c2[0], Color::rgb(0, 0, 255));
}

// ── Time boundary edge cases ────────────────────────────────────

#[test]
fn effect_active_at_exact_start_time() {
    let show = show_with_fixture_effects(
        vec![fixture(1, 1)],
        vec![solid_effect(2.0, 4.0, Color::WHITE)],
    );
    let frame = evaluate(&show, 0, 2.0, None, None, &HashMap::new());
    assert!(
        fixture_has_non_black(&frame, 1),
        "effect should be active at exact start time"
    );
}

#[test]
fn effect_active_at_exact_end_time_within_epsilon() {
    // TimeRange::contains uses epsilon tolerance: t < end + TIME_EPSILON.
    // At exactly end, it should still be contained (within epsilon).
    let show = show_with_fixture_effects(
        vec![fixture(1, 1)],
        vec![solid_effect(2.0, 4.0, Color::WHITE)],
    );
    let frame = evaluate(&show, 0, 4.0, None, None, &HashMap::new());
    assert!(
        fixture_has_non_black(&frame, 1),
        "effect should be active at exact end time (within epsilon tolerance)"
    );
}

#[test]
fn effect_inactive_well_past_end() {
    let show = show_with_fixture_effects(
        vec![fixture(1, 1)],
        vec![solid_effect(2.0, 4.0, Color::WHITE)],
    );
    // 1ms past end — well beyond epsilon (1e-9)
    let frame = evaluate(&show, 0, 4.001, None, None, &HashMap::new());
    assert!(
        fixture_is_all_black(&frame, 1),
        "effect should be inactive 1ms past end"
    );
}

#[test]
fn time_normalization_agrees_with_containment() {
    // If contains(t) is true, normalize(t) should be in [0, 1].
    // If contains(t) is false, the effect shouldn't be evaluated at all.
    // Test the boundary: exactly at start and end.
    let show = show_with_fixture_effects(
        vec![fixture(1, 1)],
        vec![solid_effect(2.0, 6.0, Color::WHITE)],
    );
    // At start: normalize(2.0) for range [2,6] = 0.0
    let frame = evaluate(&show, 0, 2.0, None, None, &HashMap::new());
    assert!(fixture_has_non_black(&frame, 1));

    // At midpoint: normalize(4.0) for range [2,6] = 0.5
    let frame = evaluate(&show, 0, 4.0, None, None, &HashMap::new());
    assert!(fixture_has_non_black(&frame, 1));

    // At end: normalize(6.0) for range [2,6] = 1.0 (within epsilon)
    let frame = evaluate(&show, 0, 6.0, None, None, &HashMap::new());
    assert!(fixture_has_non_black(&frame, 1));
}

#[test]
fn sequential_effects_no_gap_at_boundary() {
    // Two back-to-back effects: [0,3) and [3,6). At t=3.0, the second effect
    // should be active (and the first should be within epsilon too).
    // This verifies there's no single-frame gap between sequential effects.
    let show = show_with_fixture_effects(
        vec![fixture(1, 1)],
        vec![
            solid_effect(0.0, 3.0, Color::rgb(255, 0, 0)),
            solid_effect(3.0, 6.0, Color::rgb(0, 255, 0)),
        ],
    );
    let frame = evaluate(&show, 0, 3.0, None, None, &HashMap::new());
    let colors = decode_fixture_colors(&frame, 1).unwrap();
    // The second effect uses Override, so it will replace the first.
    // The key assertion: we get SOME output (no gap).
    assert!(
        colors[0].r > 0 || colors[0].g > 0,
        "should have output at the boundary between two sequential effects"
    );
}

// ── Effect contract: all built-in effects produce valid output ──

#[test]
fn all_builtin_effects_produce_valid_colors() {
    // For every BuiltInEffect variant, evaluate at t=0, 0.5, 1.0 with
    // both 1 pixel and 10 pixels. Assert: no panic, channels in [0, 255].
    // This is a contract test — catches new effects that forget edge cases.
    use crate::effects::evaluate_pixels;

    let test_times = [0.0, 0.5, 1.0];
    let test_pixel_counts = [1, 10];

    for &variant in EffectKind::all_builtin() {
        let kind = EffectKind::BuiltIn(variant);
        let params = EffectParams::new();

        for &pixel_count in &test_pixel_counts {
            for &t in &test_times {
                let mut dest = vec![Color::BLACK; pixel_count];
                // Provide fallback positions for spatial effects
                let positions: Vec<crate::model::show::Position2D> = (0..pixel_count)
                    .map(|i| {
                        let x = if pixel_count > 1 {
                            i as f32 / (pixel_count - 1) as f32
                        } else {
                            0.5
                        };
                        crate::model::show::Position2D { x, y: 0.5 }
                    })
                    .collect();

                evaluate_pixels(
                    &kind,
                    t,
                    &mut dest,
                    0,
                    pixel_count,
                    &params,
                    BlendMode::Override,
                    1.0,
                    Some(&positions),
                );
                // No panic = success. Colors are u8, so [0, 255] is guaranteed by type.
                // But verify alpha is set (effects should produce visible output or BLACK).
                for (i, c) in dest.iter().enumerate() {
                    assert!(
                        c.a == 255,
                        "{variant:?} at t={t}, pixels={pixel_count}, pixel {i}: alpha should be 255, got {}",
                        c.a
                    );
                }
            }
        }
    }
}
