use std::time::Duration;

use dawn_language::dsl::{Identifier, compile_effects, compile_operators};
use dawn_language::effect::{
    CurveId, CurveSource, EffectDefinition, EffectDefinitionId, EffectInst, EffectInstId,
    EffectParamValue, EffectScope, EffectTarget, GradientDefinition, GradientId, GradientSource,
};
use dawn_language::identity::SourceIdentity;
use dawn_language::model::{DawnProject, ProjectDefinitionStores, ProjectId, ProjectRoot};
use dawn_language::operator::{
    BuiltinOperator, GraphOperatorNode, OperatorDefinitionId, OperatorRef,
    custom_operator_definition,
};
use dawn_language::sequence::{
    AutomationClip, AutomationClipId, CompositionGraphNode, CompositionGraphNodeId,
    CompositionGraphNodeKind, EffectGraphEdge, GraphNodePosition, GraphPortId, MarkCollection,
    MarkCollectionKey, Sequence, SequenceAudio, SequenceCompositionGraph, SequenceId,
    SequenceLayer, SequenceLayerId,
};
use dawn_language::setup::{
    ControllerDefinitionStore, FixtureDefinition, FixtureDefinitionId, FixtureDefinitionStore,
    FixtureGroup, FixtureGroupId, FixtureInst, FixtureInstanceId, Geometry, Layout, LayoutId,
    PatchId, Setup, SetupId,
};
use dawn_language::values::{
    Color, Curve, CurvePoint, DawnDuration, DawnTime, DistanceSpan, Gradient, GradientStop, Point3,
    Rotation3, Scale3,
};
use indexmap::IndexMap;

use super::*;

fn source_identity(object: &str) -> SourceIdentity {
    SourceIdentity::new("test.dawn".into(), object.to_string())
}

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
fn graph_max_renders_two_sources() {
    let renderer = renderer_for(sequence_with_graph(BuiltinOperator::Max, 1, 1));

    assert_eq!(
        renderer.render_frame(0).unwrap().fixtures[0].pixels[0],
        color(200, 0, 210)
    );
}

#[test]
fn graph_intensity_modulate_uses_max_rgb_channel() {
    let renderer = renderer_for(sequence_with_graph(
        BuiltinOperator::IntensityModulate,
        1,
        1,
    ));

    assert_eq!(
        renderer.render_frame(0).unwrap().fixtures[0].pixels[0],
        color(165, 0, 0)
    );
}

#[test]
fn composition_graph_mixes_mismatched_layer_targets() {
    let renderer = renderer_for(sequence_with_graph(BuiltinOperator::Add, 1, 2));
    let frame = renderer.render_frame(0).unwrap();

    assert_eq!(frame.fixtures[0].pixels[0], color(200, 0, 0));
    assert_eq!(frame.fixtures[1].pixels[0], color(0, 0, 210));
}

#[test]
fn composition_graph_output_with_no_inputs_renders_black() {
    let mut sequence = sequence_with_effects(vec![constant_effect(
        1,
        0.0,
        10.0,
        1,
        EffectScope::WholeTarget,
        "Red",
        IndexMap::new(),
    )]);
    sequence.composition_graph.edges.clear();
    let renderer = renderer_for(sequence);
    let frame = renderer.render_frame(0).unwrap();

    assert_eq!(frame.fixtures[0].pixels[0], color(0, 0, 0));
}

#[test]
fn missing_references_fail_and_automation_prepares() {
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
        PreparedSequenceRenderer::prepare(&project, &SetupId(source_identity("setup")), &seq_id()),
        Err(RenderError::MissingEffect { .. })
    ));

    let params = IndexMap::from([(
        ident("gradient"),
        EffectParamValue::Curve(CurveSource::Reference(CurveId(source_identity("missing")))),
    )]);
    let sequence = sequence_with_effects(vec![constant_effect(
        1,
        0.0,
        1.0,
        1,
        EffectScope::WholeTarget,
        "GradientColor",
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
        start: time(0.0),
        duration: duration(1.0),
        anchor_lane_index: 0,
        lane_index: 0,
        curve: Curve {
            points: vec![
                CurvePoint {
                    position: 0.0,
                    value: 0.0,
                },
                CurvePoint {
                    position: 1.0,
                    value: 1.0,
                },
            ],
        },
        bindings: Vec::new(),
    }];
    assert!(prepare(sequence).is_ok());
}

#[test]
fn no_arg_and_global_mark_builtins_are_rejected() {
    assert!(
        compile_effects(
            "effect Bad { color sample() { return rgb(mark_count() / 255.0, 0.0, 0.0); } }"
        )
        .is_err()
    );
    assert!(
        compile_effects(
            "effect Bad { color sample() { return rgb(mark_global_count() / 255.0, 0.0, 0.0); } }"
        )
        .is_err()
    );
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
        PreparedSequenceRenderer::prepare(&project, &SetupId(source_identity("setup")), &seq_id())
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
                        value: 0.0,
                    },
                    CurvePoint {
                        position: 1.0,
                        value: 1.0,
                    },
                ],
            })),
        ),
        (
            ident("palette"),
            EffectParamValue::Gradient(GradientSource::Inline(Gradient {
                stops: vec![
                    GradientStop {
                        position: 0.0,
                        color: color(0, 0, 0),
                    },
                    GradientStop {
                        position: 1.0,
                        color: color(0, 200, 0),
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
          param curve level;
          param gradient palette;
          color sample() {
            float amount = curve_clamped(level, 0.5, 0.0, 1.0);
            float position = curve_crossing(level, 0.5, 0.0);
            color zero = gradient_color_scaled(palette, position, 0.0);
            return mix(zero, gradient_color_scaled(palette, position, amount), 1.0);
          }
        }",
    );

    let frame =
        PreparedSequenceRenderer::prepare(&project, &SetupId(source_identity("setup")), &seq_id())
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
        PreparedSequenceRenderer::prepare(&project, &SetupId(source_identity("setup")), &seq_id())
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
fn generator_emits_native_sample_with_timing_target_name_and_color() {
    let params = IndexMap::from([
        (
            ident("palette"),
            EffectParamValue::Gradient(GradientSource::Inline(Gradient {
                stops: vec![GradientStop {
                    position: 0.0,
                    color: color(0, 200, 0),
                }],
            })),
        ),
        (
            ident("shape"),
            EffectParamValue::Curve(CurveSource::Inline(Curve {
                points: vec![CurvePoint {
                    position: 0.0,
                    value: 1.0,
                }],
            })),
        ),
    ]);
    let mut project = project(sequence_with_effects(vec![constant_effect(
        1,
        1.0,
        4.0,
        3,
        EffectScope::WholeTarget,
        "NativePulseGenerator",
        params,
    )]));
    insert_effect(
        &mut project,
        "NativePulseGenerator",
        "effect NativePulseGenerator {
          param gradient palette;
          param curve shape;
          void generate() {
            timeline.emit builtins.pulse {
              start: 0.5,
              duration: 2.0,
              target: pick(fixtures(target), 1.0),
              gradient: palette,
              pulse_shape: shape,
            };
          }
        }",
    );

    let renderer =
        PreparedSequenceRenderer::prepare(&project, &SetupId(source_identity("setup")), &seq_id())
            .unwrap();

    assert_eq!(renderer.effects.len(), 1);
    assert_eq!(renderer.effects[0].name, "Pulse");
    assert_float(renderer.effects[0].start_seconds, 1.5);
    assert_float(renderer.effects[0].duration_seconds, 2.0);
    assert!(
        renderer.effects[0]
            .target
            .iter()
            .all(|pixel| pixel.fixture_index == 1)
    );
    let frame = renderer.render_seconds(2.5).unwrap();
    assert_eq!(frame.fixtures[0].pixels, vec![black(), black()]);
    assert_eq!(
        frame.fixtures[1].pixels,
        vec![color(0, 200, 0), color(0, 200, 0)]
    );
    assert_eq!(frame.fixtures[2].pixels, vec![black(), black()]);
}

#[test]
fn generator_emits_native_mark_generator_and_renders_private_child() {
    let marks_key = MarkCollectionKey {
        name: "beats".to_string(),
    };
    let params = IndexMap::from([
        (ident("beats"), EffectParamValue::Marks(marks_key.clone())),
        (
            ident("accent"),
            EffectParamValue::Gradient(GradientSource::Inline(Gradient {
                stops: vec![GradientStop {
                    position: 0.0,
                    color: color(200, 0, 0),
                }],
            })),
        ),
        (
            ident("hue"),
            EffectParamValue::Curve(CurveSource::Inline(Curve {
                points: vec![CurvePoint {
                    position: 0.0,
                    value: 0.0,
                }],
            })),
        ),
    ]);
    let mut sequence = sequence_with_effects(vec![constant_effect(
        1,
        1.0,
        4.0,
        3,
        EffectScope::WholeTarget,
        "NativeMarkGenerator",
        params,
    )]);
    sequence.mark_collections.push(MarkCollection {
        key: marks_key,
        name: "Beats".to_string(),
        display_color: color(255, 255, 255),
        marks: vec![time(1.5)],
    });
    let mut project = project(sequence);
    insert_effect(
        &mut project,
        "NativeMarkGenerator",
        "effect NativeMarkGenerator {
          param marks beats;
          param gradient accent;
          param curve hue;
          void generate() {
            timeline.emit builtins.mark_pulse {
              start: 0.5,
              duration: 2.0,
              target: pick(fixtures(target), 1.0),
              beats: beats,
              accent: accent,
              hue: hue,
              hue_mix: 0.0,
              decay_seconds: 1.0,
              sections_per_mark: 1,
            };
          }
        }",
    );

    let renderer =
        PreparedSequenceRenderer::prepare(&project, &SetupId(source_identity("setup")), &seq_id())
            .unwrap();

    assert_eq!(renderer.effects.len(), 1);
    assert_eq!(renderer.effects[0].name, "Mark Pulse");
    assert_float(renderer.effects[0].start_seconds, 2.0);
    assert_float(renderer.effects[0].duration_seconds, 1.0);
    assert!(
        renderer.effects[0]
            .target
            .iter()
            .all(|pixel| pixel.fixture_index == 1)
    );
    let frame = renderer.render_seconds(2.0).unwrap();
    assert_eq!(frame.fixtures[0].pixels, vec![black(), black()]);
    assert_eq!(
        frame.fixtures[1].pixels,
        vec![color(200, 0, 0), color(200, 0, 0)]
    );
    assert_eq!(frame.fixtures[2].pixels, vec![black(), black()]);
}

#[test]
fn generated_builtin_parameter_errors_are_explicit() {
    for (name, fields, expected) in [
        ("Missing", "", "missing generated param `gradient`"),
        ("Unknown", "nope: 1.0,", "unknown generated param `nope`"),
        (
            "WrongType",
            "gradient: #ffffff,",
            "generated param `gradient` has wrong type",
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
            name,
            &format!(
                "effect {name} {{
                  void generate() {{
                    timeline.emit builtins.pulse {{
                      start: 0.0, duration: 1.0, target: target, {fields}
                    }};
                  }}
                }}"
            ),
        );

        let error = PreparedSequenceRenderer::prepare(
            &project,
            &SetupId(source_identity("setup")),
            &seq_id(),
        )
        .expect_err("invalid generated built-in params must fail");
        assert!(
            matches!(
                error,
                RenderError::GeneratorPrepare { ref message } if message == expected
            ),
            "unexpected error: {error:?}"
        );
    }
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
            PreparedSequenceRenderer::prepare(&project, &SetupId(source_identity("setup")), &seq_id()),
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
        PreparedSequenceRenderer::prepare(&project, &SetupId(source_identity("setup")), &seq_id()),
        Err(RenderError::GeneratorPrepare { .. })
    ));
}

fn renderer_for(sequence: Sequence) -> PreparedSequenceRenderer {
    prepare(sequence).unwrap()
}

fn custom_operator_renderer(
    source: &str,
    params: IndexMap<Identifier, EffectParamValue>,
) -> PreparedSequenceRenderer {
    let compiled = compile_operators(source)
        .unwrap()
        .into_iter()
        .next()
        .unwrap();
    let id = OperatorDefinitionId(source_identity(compiled.name().as_str()));
    let mut sequence = sequence_with_effects(vec![constant_effect(
        1,
        0.0,
        10.0,
        1,
        EffectScope::WholeTarget,
        "Red",
        IndexMap::new(),
    )]);
    sequence.composition_graph = SequenceCompositionGraph {
        nodes: vec![
            layer_node(1, 0, 0.0, 0.0),
            CompositionGraphNode {
                id: CompositionGraphNodeId(2),
                position: GraphNodePosition { x: 160.0, y: 0.0 },
                kind: CompositionGraphNodeKind::Operator(GraphOperatorNode {
                    operator: OperatorRef::Custom(id.clone()),
                    params,
                }),
            },
            CompositionGraphNode {
                id: CompositionGraphNodeId(3),
                position: GraphNodePosition { x: 320.0, y: 0.0 },
                kind: CompositionGraphNodeKind::Output,
            },
        ],
        edges: vec![
            node_edge(1, "output", 2, "source"),
            node_edge(2, "output", 3, "input"),
        ],
    };
    let mut project = project(sequence);
    project
        .definitions
        .operators
        .insert(id.clone(), custom_operator_definition(id, compiled));
    PreparedSequenceRenderer::prepare(&project, &SetupId(source_identity("setup")), &seq_id())
        .unwrap()
}

fn identifier(value: &str) -> Identifier {
    Identifier::new(value.to_string()).unwrap()
}

fn prepare(sequence: Sequence) -> Result<PreparedSequenceRenderer, RenderError> {
    PreparedSequenceRenderer::prepare(
        &project(sequence),
        &SetupId(source_identity("setup")),
        &seq_id(),
    )
}

fn project(sequence: Sequence) -> DawnProject {
    let mut setups = IndexMap::new();
    setups.insert(
        SetupId(source_identity("setup")),
        Setup {
            id: SetupId(source_identity("setup")),
            layout: LayoutId(source_identity("layout")),
            patch: PatchId(source_identity("patch")),
            controllers: Vec::new(),
        },
    );
    let mut layouts = IndexMap::new();
    layouts.insert(LayoutId(source_identity("layout")), layout());

    DawnProject {
        root: ProjectRoot {
            id: ProjectId(source_identity("project")),
            setup: SetupId(source_identity("setup")),
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
        FixtureDefinitionId(source_identity("two")),
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
        (
            "Green",
            "effect Green { color sample() { return rgb(0.0, 1.0, 0.0); } }",
        ),
        (
            "IndexColor",
            "effect IndexColor { color sample() { return rgb(pixel_index() / pixel_count(), 0.0, 0.0); } }",
        ),
        (
            "Red",
            "effect Red { color sample() { return rgb(0.7843137255, 0.0, 0.0); } }",
        ),
        (
            "Blue",
            "effect Blue { color sample() { return rgb(0.0, 0.0, 0.8235294118); } }",
        ),
        (
            "MarkCountColor",
            "effect MarkCountColor { param marks beats; color sample() { return rgb(mark_count(beats) / 255.0, 0.0, 0.0); } }",
        ),
        (
            "GradientColor",
            "effect GradientColor { param gradient gradient; color sample() { return gradient[0.0]; } }",
        ),
    ] {
        let compiled = compile_effects(source).unwrap().into_iter().next().unwrap();
        let id = EffectDefinitionId(source_identity(name));
        effects.insert(id.clone(), EffectDefinition::custom(id, compiled));
    }

    ProjectDefinitionStores {
        effects,
        fixtures,
        curves: dawn_language::effect::CurveDefinitionStore::default(),
        gradients: dawn_language::effect::GradientDefinitionStore {
            definitions: IndexMap::from([(
                GradientId(source_identity("gradient")),
                GradientDefinition {
                    gradient: Gradient {
                        stops: vec![GradientStop {
                            position: 0.0,
                            color: color(255, 0, 0),
                        }],
                    },
                },
            )]),
        },
        controllers: ControllerDefinitionStore::default(),
        operators: dawn_language::operator::OperatorDefinitionStore::default(),
    }
}

fn insert_effect(project: &mut DawnProject, name: &str, source: &str) {
    let compiled = compile_effects(source)
        .unwrap()
        .into_iter()
        .find(|effect| effect.name().as_str() == name)
        .unwrap();
    let id = EffectDefinitionId(source_identity(name));
    project
        .definitions
        .effects
        .insert(id.clone(), EffectDefinition::custom(id, compiled));
}

fn layout() -> Layout {
    Layout {
        id: LayoutId(source_identity("layout")),
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
        definition: FixtureDefinitionId(source_identity("two")),
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
        layers: default_layers(),
        effects,
        composition_graph: default_composition_graph(),
        automation_clips: Vec::new(),
    }
}

fn sequence_with_graph(
    operator: BuiltinOperator,
    left_group_id: u32,
    right_group_id: u32,
) -> Sequence {
    let input_ports = operator
        .definition()
        .inputs
        .iter()
        .map(|port| port.source_name.clone())
        .collect::<Vec<_>>();
    let mut left = constant_effect(
        1,
        0.0,
        10.0,
        left_group_id,
        EffectScope::WholeTarget,
        "Red",
        IndexMap::new(),
    );
    left.layer_id = SequenceLayerId(0);
    let mut right = constant_effect(
        2,
        0.0,
        10.0,
        right_group_id,
        EffectScope::WholeTarget,
        "Blue",
        IndexMap::new(),
    );
    right.layer_id = SequenceLayerId(1);
    let mut sequence = sequence_with_effects(vec![left, right]);
    sequence.layers.push(SequenceLayer {
        id: SequenceLayerId(1),
        name: "Layer 2".to_string(),
        color: Color {
            red: 246,
            green: 184,
            blue: 75,
        },
        enabled: true,
    });
    sequence.composition_graph = SequenceCompositionGraph {
        nodes: vec![
            layer_node(1, 0, 0.0, 0.0),
            layer_node(2, 1, 0.0, 120.0),
            CompositionGraphNode {
                id: CompositionGraphNodeId(3),
                position: GraphNodePosition { x: 160.0, y: 60.0 },
                kind: CompositionGraphNodeKind::Operator(GraphOperatorNode {
                    operator: OperatorRef::Builtin(operator),
                    params: IndexMap::new(),
                }),
            },
            CompositionGraphNode {
                id: CompositionGraphNodeId(4),
                position: GraphNodePosition { x: 320.0, y: 60.0 },
                kind: CompositionGraphNodeKind::Output,
            },
        ],
        edges: vec![
            node_edge(1, "output", 3, &input_ports[0]),
            node_edge(2, "output", 3, &input_ports[1]),
            node_edge(3, "output", 4, "input"),
        ],
    };
    sequence
}

fn default_layers() -> Vec<SequenceLayer> {
    vec![SequenceLayer {
        id: SequenceLayerId(0),
        name: "Default".to_string(),
        color: Color {
            red: 80,
            green: 160,
            blue: 255,
        },
        enabled: true,
    }]
}

#[test]
fn custom_operator_samples_current_signal_with_parameter_override() {
    let renderer = custom_operator_renderer(
        "operator Gain { input Signal source; param float amount = 1.0; color sample() { return source.at(seconds()) * amount; } }",
        IndexMap::from([(identifier("amount"), EffectParamValue::Float(0.5))]),
    );
    assert_eq!(
        renderer.render_seconds(1.0).unwrap().fixtures[0].pixels[0],
        color(100, 0, 0)
    );
}

#[test]
fn custom_operator_shifted_sampling_is_black_outside_sequence() {
    let renderer = custom_operator_renderer(
        "operator Shift { input Signal source; color sample() { return source.at(seconds() - 1.0); } }",
        IndexMap::new(),
    );
    assert_eq!(
        renderer.render_seconds(0.5).unwrap().fixtures[0].pixels[0],
        color(0, 0, 0)
    );
    assert_eq!(
        renderer.render_seconds(1.5).unwrap().fixtures[0].pixels[0],
        color(200, 0, 0)
    );
}

#[test]
fn graph_render_cache_reuses_shared_rgb_buffers() {
    let renderer = renderer_for(sequence_with_effects(vec![constant_effect(
        1,
        0.0,
        10.0,
        1,
        EffectScope::WholeTarget,
        "Red",
        IndexMap::new(),
    )]));
    let mut cache = HashMap::new();
    let first = render_graph_node(&renderer, 0, 1.0, &mut cache).unwrap();
    let second = render_graph_node(&renderer, 0, 1.0, &mut cache).unwrap();
    assert!(Arc::ptr_eq(&first, &second));
}

#[test]
fn builtin_delay_executes_embedded_operator_dsl() {
    assert!(matches!(
        &BuiltinOperator::Delay.definition().implementation,
        dawn_language::operator::OperatorImplementation::Dsl(_)
    ));
}

fn default_composition_graph() -> SequenceCompositionGraph {
    SequenceCompositionGraph {
        nodes: vec![
            layer_node(1, 0, 0.0, 0.0),
            CompositionGraphNode {
                id: CompositionGraphNodeId(2),
                position: GraphNodePosition { x: 200.0, y: 0.0 },
                kind: CompositionGraphNodeKind::Output,
            },
        ],
        edges: vec![node_edge(1, "output", 2, "input")],
    }
}

fn layer_node(id: u32, layer_id: u32, x: f64, y: f64) -> CompositionGraphNode {
    CompositionGraphNode {
        id: CompositionGraphNodeId(id),
        position: GraphNodePosition { x, y },
        kind: CompositionGraphNodeKind::Layer {
            layer_id: SequenceLayerId(layer_id),
        },
    }
}

fn node_edge(from_node: u32, from_port: &str, to_node: u32, to_port: &str) -> EffectGraphEdge {
    EffectGraphEdge {
        from: CompositionGraphNodeId(from_node),
        from_port: GraphPortId(from_port.to_string()),
        to: CompositionGraphNodeId(to_node),
        to_port: GraphPortId(to_port.to_string()),
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
        layer_id: SequenceLayerId(0),
        start: time(start),
        duration: duration(effect_duration),
        target: EffectTarget::Group(FixtureGroupId(group_id)),
        scope,
        definition: dawn_language::effect::EffectRef::Custom(EffectDefinitionId(source_identity(
            definition,
        ))),
        param_overrides,
    }
}

fn seq_id() -> SequenceId {
    SequenceId(source_identity("seq"))
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
