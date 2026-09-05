use dawn_language::dsl::{
    Color, GeneratedEffectRef, GeneratorContext, Identifier, OperatorRunContext, RuntimeError,
    SignalSampler, TargetValue, Value, VmWorkspace, compile_effects, compile_operators,
};
use dawn_language::effect::BuiltinEffect;
use dawn_language::values::{SampleDuration, SampleTime};
use indexmap::IndexMap;
use std::sync::Arc;

#[test]
fn declaration_kinds_are_source_specific() {
    assert!(
        compile_effects(
            "operator Gain { input Signal source; color sample() { return source.at(seconds()); } }"
        )
        .is_err()
    );
    assert!(compile_operators("effect Solid { color sample() { return #ffffff; } }").is_err());
}

#[test]
fn constant_and_calculated_arrays_preserve_nested_values_and_assignment() {
    let effect = compile_effects(
        "effect Arrays {
            color sample() {
                array<array<float>> table = [[0.1, 0.2], [0.3, 0.4, 0.5]];
                array<float> values = [progress(), table[1][2]];
                array<float> saved = values;
                values = [0.9];
                return rgb(saved[0], saved[1], values[0]);
            }
        }",
    )
    .unwrap()
    .remove(0);
    let params = effect.bind_params(&IndexMap::new()).unwrap();
    let context = OperatorRunContext {
        progress: 0.25,
        time: SampleDuration::from_ticks(250_000),
        duration: SampleDuration::from_ticks(1_000_000),
        pixel_index: 0,
        pixel_count: 1,
        pixel_fraction: 0.0,
    };
    let mut workspace = VmWorkspace::default();
    for _ in 0..3 {
        assert_eq!(
            effect
                .sample_bound(&params, &context, &mut workspace)
                .unwrap(),
            Color {
                red: 64,
                green: 128,
                blue: 230
            },
        );
    }
}

#[test]
fn operator_requires_a_signal_input() {
    let diagnostics = compile_operators("operator Empty { color sample() { return #000000; } }")
        .expect_err("operator without inputs must fail");
    assert!(
        diagnostics
            .iter()
            .any(|diagnostic| diagnostic.message.contains("at least one Signal input"))
    );
}

#[test]
fn array_aliases_survive_loops_nested_reassignment_and_workspace_reuse() {
    let effect = compile_effects(include_str!("fixtures/array-lifetimes.effect.dawn"))
        .unwrap()
        .remove(0);
    let small = compile_effects("effect Small { color sample() { return rgb(0.0, 0.0, 0.0); } }")
        .unwrap()
        .remove(0);
    let small_params = small.bind_params(&IndexMap::new()).unwrap();
    let mut workspace = VmWorkspace::default();
    for (progress, iterations, expected) in [
        (
            0.25,
            4,
            Color {
                red: 64,
                green: 89,
                blue: 230,
            },
        ),
        (
            0.5,
            256,
            Color {
                red: 128,
                green: 153,
                blue: 230,
            },
        ),
        (
            0.0,
            2,
            Color {
                red: 0,
                green: 26,
                blue: 230,
            },
        ),
    ] {
        let context = OperatorRunContext {
            progress,
            time: SampleDuration::from_ticks(0),
            duration: SampleDuration::from_ticks(1_000_000),
            pixel_index: 0,
            pixel_count: 1,
            pixel_fraction: 0.0,
        };
        let mut values = IndexMap::from([
            (
                Identifier::new("iterations".into()).unwrap(),
                Value::Int(iterations),
            ),
            (Identifier::new("fail".into()).unwrap(), Value::Bool(true)),
        ]);
        let failing = effect.bind_params(&values).unwrap();
        let error = effect
            .sample_bound(&failing, &context, &mut workspace)
            .unwrap_err();
        assert!(
            error
                .message
                .contains("integer arithmetic overflow or division by zero")
        );
        values.insert(Identifier::new("fail".into()).unwrap(), Value::Bool(false));
        let params = effect.bind_params(&values).unwrap();
        // Reuse after an error, then after a program with a different register layout.
        assert_eq!(
            effect
                .sample_bound(&params, &context, &mut workspace)
                .unwrap(),
            expected
        );
        assert_eq!(
            small
                .sample_bound(&small_params, &context, &mut workspace)
                .unwrap(),
            Color {
                red: 0,
                green: 0,
                blue: 0
            }
        );
        assert_eq!(
            effect
                .sample_bound(&params, &context, &mut workspace)
                .unwrap(),
            expected
        );
    }
}

#[test]
fn enum_identity_survives_subset_assignment_arrays_and_program_reuse() {
    let mut workspace = VmWorkspace::default();
    let context = OperatorRunContext {
        progress: 0.0,
        time: SampleDuration::from_ticks(0),
        duration: SampleDuration::from_ticks(1_000_000),
        pixel_index: 0,
        pixel_count: 1,
        pixel_fraction: 0.0,
    };
    for options in ["alpha, beta, gamma", "gamma, alpha, beta"] {
        let effect = compile_effects(&format!(
            "effect EnumIdentity {{
                param enum wide {{ {options} }} = alpha;
                param enum subset {{ gamma, beta }} = beta;
                color sample() {{
                    wide = subset;
                    if (wide != subset || wide != beta || wide == alpha) {{ return rgb(1.0, 0.0, 0.0); }}
                    subset = gamma;
                    if (wide != beta || subset != gamma) {{ return rgb(0.0, 1.0, 0.0); }}
                    wide = [wide, subset][1];
                    if (wide != gamma) {{ return rgb(0.0, 0.0, 1.0); }}
                    return rgb(0.25, 0.5, 0.75);
                }}
            }}"
        )).unwrap().remove(0);
        let params = effect.bind_params(&IndexMap::new()).unwrap();
        for _ in 0..3 {
            assert_eq!(
                effect
                    .sample_bound(&params, &context, &mut workspace)
                    .unwrap(),
                Color {
                    red: 64,
                    green: 128,
                    blue: 191
                }
            );
        }
    }
}

#[test]
fn generator_emitted_arrays_and_enums_outlive_vm_registers() {
    let effect = compile_effects(
        "effect EmitValues {
            param enum mode { alpha, beta } = beta;
            void generate() {
                for (int i = 0; i < 3; i = i + 1) {
                    timeline.emit Child {
                        start: 0.0, duration: 1.0, target: target,
                        mode: mode, values: [[i], [i + 1, i + 2]]
                    };
                    mode = alpha;
                }
            }
        }",
    )
    .unwrap()
    .remove(0);
    let params = effect.bind_params(&IndexMap::new()).unwrap();
    let context = GeneratorContext {
        start_time: SampleTime::from_ticks(0),
        duration: SampleDuration::from_ticks(1_000_000),
        target: Arc::new(TargetValue { groups: Vec::new() }),
    };
    let mut workspace = VmWorkspace::default();
    let first = effect
        .generate_bound(&params, &context, &mut workspace)
        .unwrap();
    let second = effect
        .generate_bound(&params, &context, &mut workspace)
        .unwrap();
    assert_eq!(first, second);
    assert_eq!(first.len(), 3);
    for (index, child) in first.iter().enumerate() {
        let index = index as i32;
        assert_eq!(
            child.params,
            vec![
                (
                    Identifier::new("mode".into()).unwrap(),
                    Value::Enum(
                        Identifier::new(if index == 0 { "beta" } else { "alpha" }.into()).unwrap()
                    )
                ),
                (
                    Identifier::new("values".into()).unwrap(),
                    Value::Array(Arc::from([
                        Value::Array(Arc::from([Value::Int(index)])),
                        Value::Array(Arc::from([Value::Int(index + 1), Value::Int(index + 2)])),
                    ]))
                ),
            ]
        );
    }
}

#[test]
fn builtin_operator_names_are_reserved() {
    assert!(compile_operators("operator Delay { input Signal source; color sample() { return source.at(seconds()); } }").is_err());
    assert!(compile_operators("operator intensity_modulate { input Signal source; color sample() { return source.at(seconds()); } }").is_err());
}

#[test]
fn enum_params_require_an_option() {
    let diagnostics =
        compile_effects("effect Bad { param enum mode {}; color sample() { return #000000; } }")
            .expect_err("empty enum must fail");
    assert!(
        diagnostics
            .iter()
            .any(|diagnostic| diagnostic.message.contains("must declare an option"))
    );
}

#[test]
fn signal_is_only_valid_as_an_operator_input() {
    assert!(compile_operators("operator Bad { input Signal source; param Signal stored; color sample() { return source.at(seconds()); } }").is_err());
    assert!(compile_operators("operator Bad { input Signal source; param array<Signal> stored; color sample() { return source.at(seconds()); } }").is_err());
    assert!(compile_operators("operator Bad { input Signal source; color sample() { Signal local; return source.at(seconds()); } }").is_err());
}

#[test]
fn signal_sampling_and_color_operations_execute() {
    let operator = compile_operators(
        "operator Colors { input Signal source; color sample() { color sampled = source.at(seconds()); return max(invert(sampled) * 0.5 + #010203, sampled * #ffffff); } }",
    )
    .expect("operator compiles")
    .into_iter()
    .next()
    .expect("one operator");
    let params = operator.bind_params(&IndexMap::new()).unwrap();
    let context = OperatorRunContext {
        progress: 0.25,
        time: SampleDuration::from_ticks(1_000_000),
        duration: SampleDuration::from_ticks(4_000_000),
        pixel_index: 0,
        pixel_count: 1,
        pixel_fraction: 0.0,
    };
    let mut sampler = ConstantSignal(Color {
        red: 10,
        green: 20,
        blue: 30,
    });
    let color = operator
        .sample_bound(&params, &context, &mut sampler, &mut VmWorkspace::default())
        .expect("operator samples");
    assert_eq!(
        color,
        Color {
            red: 124,
            green: 120,
            blue: 116
        }
    );
}

#[test]
fn generator_emit_references_preserve_builtin_and_local_identity() {
    let effect = compile_effects(
        "effect EmitAll {
          void generate() {
            timeline.emit builtins.pulse { start: 0.0, duration: 1.0, target: target };
            timeline.emit builtins.chase { start: 0.0, duration: 1.0, target: target };
            timeline.emit builtins.spin { start: 0.0, duration: 1.0, target: target };
            timeline.emit builtins.mark_pulse { start: 0.0, duration: 1.0, target: target };
            timeline.emit builtins.mark_chase { start: 0.0, duration: 1.0, target: target };
            timeline.emit LocalChild { start: 0.0, duration: 1.0, target: target };
          }
        }",
    )
    .expect("generator compiles")
    .into_iter()
    .next()
    .expect("one effect");
    let generated = effect
        .generate_bound(
            &effect.bind_params(&IndexMap::new()).unwrap(),
            &GeneratorContext {
                start_time: SampleTime::from_ticks(2_000_000),
                duration: SampleDuration::from_ticks(1_000_000),
                target: Arc::new(TargetValue { groups: Vec::new() }),
            },
            &mut VmWorkspace::default(),
        )
        .expect("generator runs");

    assert_eq!(
        generated
            .iter()
            .map(|effect| effect.definition.clone())
            .collect::<Vec<_>>(),
        vec![
            GeneratedEffectRef::Builtin(BuiltinEffect::Pulse),
            GeneratedEffectRef::Builtin(BuiltinEffect::Chase),
            GeneratedEffectRef::Builtin(BuiltinEffect::Spin),
            GeneratedEffectRef::Builtin(BuiltinEffect::MarkPulse),
            GeneratedEffectRef::Builtin(BuiltinEffect::MarkChase),
            GeneratedEffectRef::Local(Identifier::new("LocalChild".to_string()).unwrap()),
        ]
    );
    assert!(
        generated.iter().all(|effect| {
            effect.start_time == SampleTime::from_ticks(2_000_000)
                && effect.duration == SampleDuration::from_ticks(1_000_000)
        }),
        "emitted timing should be converted once to the portable clock"
    );
}

#[test]
fn qualified_generator_emit_references_report_specific_diagnostics() {
    for (reference, expected) in [
        ("builtins.unknown", "unknown built-in effect `unknown`"),
        (
            "effects.pulse",
            "unsupported generated effect namespace `effects`",
        ),
        (
            "builtins.pulse.extra",
            "generated effect reference must contain exactly two segments",
        ),
    ] {
        let source = format!(
            "effect Bad {{ void generate() {{ timeline.emit {reference} {{ start: 0.0, duration: 1.0, target: target }}; }} }}"
        );
        let diagnostics = compile_effects(&source).expect_err("invalid reference must fail");
        assert!(
            diagnostics
                .iter()
                .any(|diagnostic| diagnostic.message.contains(expected)),
            "missing `{expected}` in {diagnostics:?}"
        );
    }
}

#[test]
fn source_numeric_overflow_and_integer_division_report_diagnostics() {
    let integer_overflow = compile_effects(
        "effect Bad { color sample() { int value = 999999999999999999999999999999; return #000000; } }",
    )
    .expect_err("out-of-range integer literals must fail");
    assert!(integer_overflow.iter().any(|diagnostic| {
        diagnostic
            .message
            .contains("integer literal is out of range")
    }));

    let integer_division =
        compile_effects("effect Bad { color sample() { int value = 4 / 2; return #000000; } }")
            .expect_err("integer division produces a float and cannot initialize an int");
    assert!(!integer_division.is_empty());
}

#[test]
fn required_parameters_and_integer_remainder_fail_without_panicking() {
    let effect = compile_effects(
        "effect Required { param float amount; color sample() { int value = 1 % 0; return #000000; } }",
    )
    .expect("effect compiles")
    .into_iter()
    .next()
    .expect("one effect");
    let missing = effect
        .bind_params(&IndexMap::new())
        .expect_err("required parameters must not synthesize a default");
    assert!(
        missing
            .message
            .contains("missing required parameter `amount`")
    );

    let params = IndexMap::from([(
        Identifier::new("amount".to_string()).unwrap(),
        dawn_language::dsl::Value::Float(1.0),
    )]);
    let bound = effect
        .bind_params(&params)
        .expect("required parameter binds");
    let error = effect
        .sample_bound(
            &bound,
            &dawn_language::dsl::RunContext {
                progress: 0.0,
                time: SampleDuration::from_ticks(0),
                duration: SampleDuration::from_ticks(1_000_000),
                pixel_index: 0,
                pixel_count: 1,
                pixel_fraction: 0.0,
            },
            &mut VmWorkspace::default(),
        )
        .expect_err("integer remainder by zero must become a runtime error");
    assert!(
        error
            .message
            .contains("integer arithmetic overflow or division by zero")
    );
}

struct ConstantSignal(Color);

impl SignalSampler for ConstantSignal {
    fn sample_signal(
        &mut self,
        _input: usize,
        _sample_time: SampleTime,
    ) -> Result<Color, RuntimeError> {
        Ok(self.0)
    }
}
