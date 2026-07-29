use dawn_language::dsl::{
    Color, DslVmScratch, GeneratedEffectRef, GeneratorContext, Identifier, OperatorRunContext,
    RuntimeError, SignalSampler, TargetValue, compile_effects, compile_operators,
};
use dawn_language::effect::BuiltinEffect;
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
        seconds: 1.0,
        duration: 4.0,
        pixel_index: 0,
        pixel_count: 1,
        pixel_fraction: 0.0,
        global_marks: dawn_language::dsl::Marks { marks: Vec::new() },
    };
    let mut sampler = ConstantSignal(Color {
        red: 10,
        green: 20,
        blue: 30,
    });
    let color = operator
        .sample_bound(
            &params,
            &context,
            &mut sampler,
            &mut DslVmScratch::default(),
        )
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
                duration: 1.0,
                target: Arc::new(TargetValue { groups: Vec::new() }),
            },
            &mut DslVmScratch::default(),
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
                seconds: 0.0,
                duration: 1.0,
                pixel_index: 0,
                pixel_count: 1,
                pixel_fraction: 0.0,
                global_marks: dawn_language::dsl::Marks { marks: Vec::new() },
            },
            &mut DslVmScratch::default(),
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
        _seconds: f64,
        _pixel_index: usize,
    ) -> Result<Color, RuntimeError> {
        Ok(self.0)
    }
}
