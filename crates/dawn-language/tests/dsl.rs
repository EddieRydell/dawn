use dawn_language::dsl::{
    Color, DslVmScratch, OperatorRunContext, RuntimeError, SignalSampler, compile_effects,
    compile_operators,
};
use indexmap::IndexMap;

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
    let params = operator.bind_params(&IndexMap::new());
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
