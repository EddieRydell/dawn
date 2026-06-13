use std::time::Duration;

use dawn_language::effect::{
    CurveDefinition, CurveId, CurveSource, EffectDefinition, EffectDefinitionId, EffectInst,
    EffectInstId, EffectParamValue, EffectScope, EffectTarget,
};
use dawn_language::effect_dsl::{compile_effects, Identifier};
use dawn_language::model::{DawnProject, ProjectDefinitionStores, ProjectId, ProjectRoot};
use dawn_language::sequence::{
    AutomationClip, AutomationClipId, MarkCollectionKey, Sequence, SequenceAudio, SequenceId,
};
use dawn_language::setup::{
    ControllerDefinitionStore, FixtureDefinition, FixtureDefinitionId, FixtureDefinitionStore,
    FixtureGroup, FixtureGroupId, FixtureInst, FixtureInstanceId, Geometry, Layout, LayoutId,
    PatchId, Setup, SetupId,
};
use dawn_language::values::{
    Color, Curve, CurvePoint, CurveValue, DawnDuration, DawnTime, DistanceSpan, Point3, Rotation3,
    Scale3,
};
use indexmap::IndexMap;

use super::*;

#[test]
fn frame_count_clamping_and_frame_start_sample_time_are_audio_clocked() {
    let renderer = renderer_for(sequence_with_effects(vec![constant_effect(
        1,
        0.0,
        10.0,
        3,
        EffectScope::WholeTarget,
        "FrameColor",
        IndexMap::new(),
    )]));

    assert_eq!(renderer.frame_count(), 30);
    assert_eq!(renderer.render_seconds(-1.0).unwrap().frame_index, 0);
    assert_eq!(renderer.render_seconds(10.0).unwrap().frame_index, 29);

    let frame = renderer.render_seconds(0.67).unwrap();
    assert_eq!(frame.frame_index, 2);
    assert_eq!(frame.clock_seconds, 0.67);
    assert_float(frame.sample_seconds, 2.0 / 3.0);
    assert_eq!(frame.fixtures[0].pixels[0], color(85, 0, 0));
}

#[test]
fn invalid_timing_is_rejected() {
    let mut sequence = sequence_with_effects(Vec::new());
    sequence.frame_rate = 0;
    assert!(matches!(
        prepare(sequence),
        Err(RenderError::InvalidTiming { .. })
    ));

    let mut sequence = sequence_with_effects(Vec::new());
    sequence.duration = DawnDuration(Duration::ZERO);
    assert!(matches!(
        prepare(sequence),
        Err(RenderError::InvalidTiming { .. })
    ));
}

#[test]
fn seconds_and_progress_are_effect_local() {
    let renderer = renderer_for(sequence_with_effects(vec![constant_effect(
        1,
        2.0,
        2.0,
        1,
        EffectScope::WholeTarget,
        "LocalColor",
        IndexMap::new(),
    )]));

    let frame = renderer.render_frame(9).unwrap();
    assert_eq!(frame.fixtures[0].pixels[0], color(128, 128, 0));
}

#[test]
fn effect_range_is_start_inclusive_and_end_exclusive() {
    let renderer = renderer_for(sequence_with_effects(vec![constant_effect(
        1,
        1.0,
        1.0,
        1,
        EffectScope::WholeTarget,
        "Green",
        IndexMap::new(),
    )]));

    assert_eq!(
        renderer.render_frame(2).unwrap().fixtures[0].pixels[0],
        black()
    );
    assert_eq!(
        renderer.render_frame(3).unwrap().fixtures[0].pixels[0],
        color(0, 255, 0)
    );
    assert_eq!(
        renderer.render_frame(6).unwrap().fixtures[0].pixels[0],
        black()
    );
}

#[test]
fn per_fixture_resets_pixel_index_and_whole_target_concatenates() {
    let per_fixture = renderer_for(sequence_with_effects(vec![constant_effect(
        1,
        0.0,
        10.0,
        3,
        EffectScope::PerFixture,
        "IndexColor",
        IndexMap::new(),
    )]));
    let frame = per_fixture.render_frame(0).unwrap();
    assert_eq!(
        frame.fixtures[0].pixels,
        vec![color(0, 0, 0), color(128, 0, 0)]
    );
    assert_eq!(
        frame.fixtures[1].pixels,
        vec![color(0, 0, 0), color(128, 0, 0)]
    );

    let whole_target = renderer_for(sequence_with_effects(vec![constant_effect(
        1,
        0.0,
        10.0,
        3,
        EffectScope::WholeTarget,
        "IndexColor",
        IndexMap::new(),
    )]));
    let frame = whole_target.render_frame(0).unwrap();
    assert_eq!(
        frame.fixtures[0].pixels,
        vec![color(0, 0, 0), color(43, 0, 0)]
    );
    assert_eq!(
        frame.fixtures[1].pixels,
        vec![color(85, 0, 0), color(128, 0, 0)]
    );
}

#[test]
fn group_order_follows_layout_fixture_order() {
    let renderer = renderer_for(sequence_with_effects(vec![constant_effect(
        1,
        0.0,
        10.0,
        2,
        EffectScope::WholeTarget,
        "IndexColor",
        IndexMap::new(),
    )]));
    let frame = renderer.render_frame(0).unwrap();

    assert_eq!(frame.fixtures[0].fixture_id, FixtureInstanceId(1));
    assert_eq!(frame.fixtures[1].fixture_id, FixtureInstanceId(2));
    assert_eq!(frame.fixtures[2].fixture_id, FixtureInstanceId(3));
    assert_eq!(frame.fixtures[1].pixels[0], color(0, 0, 0));
    assert_eq!(frame.fixtures[2].pixels[0], color(128, 0, 0));
}

#[test]
fn active_effects_compose_with_channel_max() {
    let renderer = renderer_for(sequence_with_effects(vec![
        constant_effect(
            1,
            0.0,
            10.0,
            1,
            EffectScope::WholeTarget,
            "Red",
            IndexMap::new(),
        ),
        constant_effect(
            2,
            0.0,
            10.0,
            1,
            EffectScope::WholeTarget,
            "Blue",
            IndexMap::new(),
        ),
    ]));

    assert_eq!(
        renderer.render_frame(0).unwrap().fixtures[0].pixels[0],
        color(200, 0, 210)
    );
}

#[test]
fn missing_references_and_unsupported_automation_fail() {
    let mut project = project(sequence_with_effects(vec![constant_effect(
        1,
        0.0,
        1.0,
        1,
        EffectScope::WholeTarget,
        "Missing",
        IndexMap::new(),
    )]));
    project.definitions.effects.definitions.clear();
    assert!(matches!(
        PreparedSequenceRenderer::prepare(&project, &SetupId("setup".to_string()), &seq_id()),
        Err(RenderError::MissingEffect { .. })
    ));

    let params = IndexMap::from([(
        ident("gradient"),
        EffectParamValue::Curve(CurveSource::Reference(CurveId("missing".to_string()))),
    )]);
    let sequence = sequence_with_effects(vec![constant_effect(
        1,
        0.0,
        1.0,
        1,
        EffectScope::WholeTarget,
        "CurveColor",
        params,
    )]);
    assert!(matches!(prepare(sequence), Err(RenderError::MissingCurve)));

    let params = IndexMap::from([(
        ident("beats"),
        EffectParamValue::Marks(MarkCollectionKey {
            name: "missing".to_string(),
        }),
    )]);
    let sequence = sequence_with_effects(vec![constant_effect(
        1,
        0.0,
        1.0,
        1,
        EffectScope::WholeTarget,
        "MarkCountColor",
        params,
    )]);
    assert!(matches!(
        prepare(sequence),
        Err(RenderError::MissingMarkCollection { .. })
    ));

    let mut sequence = sequence_with_effects(Vec::new());
    sequence.automation_clips = vec![AutomationClip {
        id: AutomationClipId(1),
        targets: vec![EffectInstId(1)],
        start: time(0.0),
        duration: duration(1.0),
        curve: CurveId("curve".to_string()),
    }];
    assert!(matches!(
        prepare(sequence),
        Err(RenderError::UnsupportedAutomation)
    ));
}

#[test]
fn no_arg_and_global_mark_builtins_are_rejected() {
    assert!(compile_effects(
        "effect Bad { color sample() { return rgb(mark_count() / 255.0, 0.0, 0.0); } }"
    )
    .is_err());
    assert!(compile_effects(
        "effect Bad { color sample() { return rgb(mark_global_count() / 255.0, 0.0, 0.0); } }"
    )
    .is_err());
}

#[test]
fn register_vm_evaluates_core_ops_and_builtins() {
    let mut project = project(sequence_with_effects(vec![constant_effect(
        1,
        0.0,
        10.0,
        1,
        EffectScope::WholeTarget,
        "RegisterCore",
        IndexMap::new(),
    )]));
    insert_effect(
        &mut project,
        "RegisterCore",
        "effect RegisterCore {
          color sample() {
            float value = 1.0;
            value = value + 2.0 * 3.0;
            bool skipped = false && rand(1.0) > 0.0;
            if (!skipped || value == 7.0) {
              value = clamp(sin(PI / 2.0) + cos(0.0) + abs(-1.0) + floor(1.9), 0.0, 4.0);
            }
            color mixed = mix(rgb(value / 4.0, section_position(2.0), pixel_fraction()), hsv(0.0, 1.0, 1.0), 0.0);
            return mixed;
          }
        }",
    );

    let frame =
        PreparedSequenceRenderer::prepare(&project, &SetupId("setup".to_string()), &seq_id())
            .unwrap()
            .render_frame(0)
            .unwrap();

    assert_eq!(frame.fixtures[0].pixels[0], color(255, 0, 0));
    assert_eq!(frame.fixtures[0].pixels[1], color(255, 128, 85));
}

#[test]
fn register_vm_uses_prepared_curve_ops() {
    let params = IndexMap::from([
        (
            ident("level"),
            EffectParamValue::Curve(CurveSource::Inline(Curve {
                points: vec![
                    CurvePoint {
                        position: 0.0,
                        value: CurveValue::Float(0.0),
                    },
                    CurvePoint {
                        position: 1.0,
                        value: CurveValue::Float(1.0),
                    },
                ],
            })),
        ),
        (
            ident("palette"),
            EffectParamValue::Curve(CurveSource::Inline(Curve {
                points: vec![
                    CurvePoint {
                        position: 0.0,
                        value: CurveValue::Color(color(0, 0, 0)),
                    },
                    CurvePoint {
                        position: 1.0,
                        value: CurveValue::Color(color(0, 200, 0)),
                    },
                ],
            })),
        ),
    ]);
    let mut project = project(sequence_with_effects(vec![constant_effect(
        1,
        0.0,
        10.0,
        1,
        EffectScope::WholeTarget,
        "CurveOps",
        params,
    )]));
    insert_effect(
        &mut project,
        "CurveOps",
        "effect CurveOps {
          param curve<float> level;
          param curve<color> palette;
          color sample() {
            float amount = curve_float_clamped(level, 0.5, 0.0, 1.0);
            float position = curve_crossing(level, 0.5, 0.0);
            color zero = curve_color_scaled(palette, position, 0.0);
            return mix(zero, curve_color_scaled(palette, position, amount), 1.0);
          }
        }",
    );

    let frame =
        PreparedSequenceRenderer::prepare(&project, &SetupId("setup".to_string()), &seq_id())
            .unwrap()
            .render_frame(0)
            .unwrap();

    assert_eq!(frame.fixtures[0].pixels[0], color(0, 50, 0));
}

#[test]
fn generator_emits_sample_child_for_selected_fixture_target() {
    let mut project = project(sequence_with_effects(vec![constant_effect(
        1,
        0.0,
        2.0,
        3,
        EffectScope::WholeTarget,
        "FixtureGenerator",
        IndexMap::new(),
    )]));
    insert_effect(
        &mut project,
        "FixtureGenerator",
        "effect FixtureGenerator {
          void generate() {
            TargetItems items = fixtures(target);
            timeline.emit Green { start: 0.0, duration: duration, target: pick(items, 1.0) };
          }
        }",
    );

    let frame =
        PreparedSequenceRenderer::prepare(&project, &SetupId("setup".to_string()), &seq_id())
            .unwrap()
            .render_frame(0)
            .unwrap();

    assert_eq!(frame.fixtures[0].pixels, vec![black(), black()]);
    assert_eq!(
        frame.fixtures[1].pixels,
        vec![color(0, 255, 0), color(0, 255, 0)]
    );
    assert_eq!(frame.fixtures[2].pixels, vec![black(), black()]);
}

#[test]
fn generated_child_validation_fails_during_prepare() {
    for (name, source) in [
        (
            "MissingChildGenerator",
            "effect MissingChildGenerator {
              void generate() {
                timeline.emit MissingChild { start: 0.0, duration: 1.0, target: target };
              }
            }",
        ),
        (
            "BadParamGenerator",
            "effect BadParamGenerator {
              void generate() {
                timeline.emit Green { start: 0.0, duration: 1.0, target: target, nope: 1.0 };
              }
            }",
        ),
        (
            "BadTypeGenerator",
            "effect BadTypeGenerator {
              void generate() {
                timeline.emit FloatChild { start: 0.0, duration: 1.0, target: target, value: #ffffff };
              }
            }",
        ),
    ] {
        let mut project = project(sequence_with_effects(vec![constant_effect(
            1,
            0.0,
            2.0,
            3,
            EffectScope::WholeTarget,
            name,
            IndexMap::new(),
        )]));
        insert_effect(
            &mut project,
            "FloatChild",
            "effect FloatChild { param float value; color sample() { return rgb(value, 0.0, 0.0); } }",
        );
        insert_effect(&mut project, name, source);

        assert!(matches!(
            PreparedSequenceRenderer::prepare(&project, &SetupId("setup".to_string()), &seq_id()),
            Err(RenderError::GeneratorPrepare { .. })
        ));
    }
}

#[test]
fn nested_generator_depth_is_bounded() {
    let mut project = project(sequence_with_effects(vec![constant_effect(
        1,
        0.0,
        2.0,
        3,
        EffectScope::WholeTarget,
        "LoopGenerator",
        IndexMap::new(),
    )]));
    insert_effect(
        &mut project,
        "LoopGenerator",
        "effect LoopGenerator {
          void generate() {
            timeline.emit LoopGenerator { start: 0.0, duration: 1.0, target: target };
          }
        }",
    );

    assert!(matches!(
        PreparedSequenceRenderer::prepare(&project, &SetupId("setup".to_string()), &seq_id()),
        Err(RenderError::GeneratorPrepare { .. })
    ));
}

fn renderer_for(sequence: Sequence) -> PreparedSequenceRenderer {
    prepare(sequence).unwrap()
}

fn prepare(sequence: Sequence) -> Result<PreparedSequenceRenderer, RenderError> {
    PreparedSequenceRenderer::prepare(&project(sequence), &SetupId("setup".to_string()), &seq_id())
}

fn project(sequence: Sequence) -> DawnProject {
    let mut setups = IndexMap::new();
    setups.insert(
        SetupId("setup".to_string()),
        Setup {
            id: SetupId("setup".to_string()),
            layout: LayoutId("layout".to_string()),
            patch: PatchId("patch".to_string()),
            controllers: Vec::new(),
        },
    );
    let mut layouts = IndexMap::new();
    layouts.insert(LayoutId("layout".to_string()), layout());

    DawnProject {
        root: ProjectRoot {
            id: ProjectId("project".to_string()),
            setup: SetupId("setup".to_string()),
            sequences: vec![seq_id()],
        },
        setups,
        layouts,
        patches: IndexMap::new(),
        controllers: IndexMap::new(),
        sequences: IndexMap::from([(seq_id(), sequence)]),
        definitions: definitions(),
    }
}

fn definitions() -> ProjectDefinitionStores {
    let mut fixtures = FixtureDefinitionStore::default();
    fixtures.insert(
        FixtureDefinitionId("two".to_string()),
        FixtureDefinition {
            bulb_radius: DistanceSpan::ZERO,
            geometry: Geometry::Points {
                points: vec![Point3::default(), Point3::default()],
            },
        },
    );

    let mut effects = dawn_language::effect::EffectDefinitionStore::default();
    for (name, source) in [
        (
            "FrameColor",
            "effect FrameColor { color sample() { return rgb(seconds() / 2.0, 0.0, 0.0); } }",
        ),
        (
            "LocalColor",
            "effect LocalColor { color sample() { return rgb(seconds() / 2.0, progress(), 0.0); } }",
        ),
        ("Green", "effect Green { color sample() { return rgb(0.0, 1.0, 0.0); } }"),
        (
            "IndexColor",
            "effect IndexColor { color sample() { return rgb(pixel_index() / pixel_count(), 0.0, 0.0); } }",
        ),
        ("Red", "effect Red { color sample() { return rgb(0.7843137255, 0.0, 0.0); } }"),
        ("Blue", "effect Blue { color sample() { return rgb(0.0, 0.0, 0.8235294118); } }"),
        (
            "MarkCountColor",
            "effect MarkCountColor { param marks beats; color sample() { return rgb(mark_count(beats) / 255.0, 0.0, 0.0); } }",
        ),
        (
            "CurveColor",
            "effect CurveColor { param curve<color> gradient; color sample() { return gradient[0.0]; } }",
        ),
    ] {
        let compiled = compile_effects(source)
            .unwrap()
            .into_iter()
            .next()
            .unwrap();
        effects.insert(
            EffectDefinitionId(name.to_string()),
            EffectDefinition { compiled },
        );
    }

    ProjectDefinitionStores {
        effects,
        fixtures,
        curves: dawn_language::effect::CurveDefinitionStore {
            definitions: IndexMap::from([(
                CurveId("curve".to_string()),
                CurveDefinition {
                    curve: Curve {
                        points: vec![CurvePoint {
                            position: 0.0,
                            value: CurveValue::Color(color(255, 0, 0)),
                        }],
                    },
                },
            )]),
        },
        controllers: ControllerDefinitionStore::default(),
    }
}

fn insert_effect(project: &mut DawnProject, name: &str, source: &str) {
    let compiled = compile_effects(source)
        .unwrap()
        .into_iter()
        .find(|effect| effect.name().as_str() == name)
        .unwrap();
    project.definitions.effects.insert(
        EffectDefinitionId(name.to_string()),
        EffectDefinition { compiled },
    );
}

fn layout() -> Layout {
    Layout {
        id: LayoutId("layout".to_string()),
        target_order: Vec::new(),
        fixtures: vec![fixture(1), fixture(2), fixture(3)],
        groups: vec![
            FixtureGroup {
                id: FixtureGroupId(1),
                name: "Out of order".to_string(),
                fixtures: vec![FixtureInstanceId(3), FixtureInstanceId(1)],
            },
            FixtureGroup {
                id: FixtureGroupId(2),
                name: "Layout order".to_string(),
                fixtures: vec![FixtureInstanceId(3), FixtureInstanceId(2)],
            },
            FixtureGroup {
                id: FixtureGroupId(3),
                name: "All".to_string(),
                fixtures: vec![
                    FixtureInstanceId(1),
                    FixtureInstanceId(2),
                    FixtureInstanceId(3),
                ],
            },
        ],
    }
}

fn fixture(id: u32) -> FixtureInst {
    FixtureInst {
        id: FixtureInstanceId(id),
        name: format!("Fixture {id}"),
        definition: FixtureDefinitionId("two".to_string()),
        position: Point3::default(),
        rotation: Rotation3::default(),
        scale: Scale3::default(),
    }
}

fn sequence_with_effects(effects: Vec<EffectInst>) -> Sequence {
    Sequence {
        id: seq_id(),
        duration: duration(10.0),
        frame_rate: 3,
        audio: SequenceAudio::None,
        mark_collections: Vec::new(),
        effects,
        automation_clips: Vec::new(),
    }
}

fn constant_effect(
    id: u32,
    start: f64,
    effect_duration: f64,
    group_id: u32,
    scope: EffectScope,
    definition: &str,
    param_overrides: IndexMap<Identifier, EffectParamValue>,
) -> EffectInst {
    EffectInst {
        id: EffectInstId(id),
        start: time(start),
        duration: duration(effect_duration),
        target: EffectTarget::Group(FixtureGroupId(group_id)),
        scope,
        definition: EffectDefinitionId(definition.to_string()),
        param_overrides,
    }
}

fn seq_id() -> SequenceId {
    SequenceId("seq".to_string())
}

fn ident(value: &str) -> Identifier {
    Identifier::new(value.to_string()).unwrap()
}

fn time(seconds: f64) -> DawnTime {
    DawnTime(Duration::from_secs_f64(seconds))
}

fn duration(seconds: f64) -> DawnDuration {
    DawnDuration(Duration::from_secs_f64(seconds))
}

fn color(red: u8, green: u8, blue: u8) -> Color {
    Color { red, green, blue }
}

fn assert_float(left: f64, right: f64) {
    assert!((left - right).abs() < 0.000001, "{left} != {right}");
}
