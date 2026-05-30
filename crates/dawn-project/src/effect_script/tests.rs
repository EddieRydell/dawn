use super::*;
use crate::model::{CurvePoint, CurveValue, CurveValueType};

fn fixture() -> FixtureContext {
    FixtureContext { index: 0 }
}

fn pixel() -> PixelContext {
    PixelContext { index: 0, count: 1 }
}

fn empty_params() -> BTreeMap<String, RuntimeValue> {
    BTreeMap::new()
}

fn fade_curve() -> RuntimeValue {
    RuntimeValue::Curve(Curve {
        value_type: CurveValueType::Float,
        points: vec![
            CurvePoint {
                time: 0.0,
                value: CurveValue::Float(0.0),
            },
            CurvePoint {
                time: 1.0,
                value: CurveValue::Float(1.0),
            },
        ],
    })
}

fn sample(script: &CompiledEffect) -> Result<Color, RuntimeError> {
    script.sample(0.25, 0.0, fixture(), pixel(), &empty_params())
}

#[test]
fn int_literals_promote_in_float_binary_contexts() {
    for expr in [
        "progress * speed * 9",
        "9 * progress * speed",
        "progress * 9.0",
    ] {
        let script = compile(&format!(
            r##"
effect Pulse {{
  param float speed = 0.75;

  color sample(float progress, float seconds, Fixture fixture, Pixel pixel) {{
float phase = (sin({expr}) + 1.0) / 2.0;
return rgb(phase * 255.0, 0, 0);
  }}
}}
"##
        ))
        .unwrap();

        sample(&script).unwrap();
    }
}

#[test]
fn int_literals_promote_in_float_call_contexts() {
    let script = compile(
        r##"
effect Calls {
  param color base = #000000;
  param color accent = #ffffff;
  param curve<float> fade;

  color sample(float progress, float seconds, Fixture fixture, Pixel pixel) {
float wave = sin(9);
color rgb_color = rgb(255, 0, 0);
color mixed = mix(base, accent, 1);
float faded = fade(1);
return mix(rgb_color, mixed, abs(wave) * 0.0 + faded);
  }
}
"##,
    )
    .unwrap();
    let mut params = BTreeMap::new();
    params.insert("fade".to_string(), fade_curve());

    let color = script
        .sample(0.0, 0.0, fixture(), pixel(), &params)
        .unwrap();
    assert_eq!(color, Color::new(255, 255, 255));
}

#[test]
fn int_can_initialize_float_local() {
    let script = compile(
        r##"
effect Local {
  param float defaulted = 1;

  color sample(float progress, float seconds, Fixture fixture, Pixel pixel) {
float x = 1;
return rgb(x + defaulted, x + defaulted, x + defaulted);
  }
}
"##,
    )
    .unwrap();

    assert_eq!(sample(&script).unwrap(), Color::new(2, 2, 2));
}

#[test]
fn int_division_truncates_toward_zero() {
    let script = compile(
        r##"
effect Divide {
  color sample(float progress, float seconds, Fixture fixture, Pixel pixel) {
int x = 5 / 2;
return rgb(x, x, x);
  }
}
"##,
    )
    .unwrap();

    assert_eq!(sample(&script).unwrap(), Color::new(2, 2, 2));
}

#[test]
fn float_cannot_initialize_int_local() {
    let errors = compile(
        r##"
effect Bad {
  color sample(float progress, float seconds, Fixture fixture, Pixel pixel) {
int x = 1.5;
return rgb(x, x, x);
  }
}
"##,
    )
    .unwrap_err();

    assert!(errors
        .iter()
        .any(|error| error.message.contains("declared as int")));
}

#[test]
fn int_divide_by_zero_returns_runtime_error() {
    let script = compile(
        r##"
effect Divide {
  color sample(float progress, float seconds, Fixture fixture, Pixel pixel) {
int x = 1 / 0;
return rgb(x, x, x);
  }
}
"##,
    )
    .unwrap();

    let error = sample(&script).unwrap_err();
    assert!(error.message.contains("divide by zero"));
}

#[test]
fn int_factor_scales_color() {
    let script = compile(
        r##"
effect Scale {
  color sample(float progress, float seconds, Fixture fixture, Pixel pixel) {
color left = #010203 * 2;
color right = 2 * #010203;
return mix(left, right, 0.5);
  }
}
"##,
    )
    .unwrap();

    assert_eq!(sample(&script).unwrap(), Color::new(2, 4, 6));
}

#[test]
fn if_else_and_else_if_execute_selected_branch() {
    let script = compile(
        r##"
effect Branch {
  color sample(float progress, float seconds, Fixture fixture, Pixel pixel) {
int value = 0;
if (progress < 0.25) {
  value = 10;
} else if (progress == 0.25) {
  value = 20;
} else {
  value = 30;
}
return rgb(value, value, value);
  }
}
"##,
    )
    .unwrap();

    assert_eq!(sample(&script).unwrap(), Color::new(20, 20, 20));
}

#[test]
fn branch_local_scope_can_assign_outer_locals() {
    let script = compile(
        r##"
effect Scope {
  color sample(float progress, float seconds, Fixture fixture, Pixel pixel) {
int value = 1;
if (true) {
  int branch_value = 7;
  value = branch_value;
} else {
  value = 2;
}
return rgb(value, value, value);
  }
}
"##,
    )
    .unwrap();

    assert_eq!(sample(&script).unwrap(), Color::new(7, 7, 7));

    let errors = compile(
        r##"
effect Scope {
  color sample(float progress, float seconds, Fixture fixture, Pixel pixel) {
if (true) {
  int branch_value = 7;
}
return rgb(branch_value, branch_value, branch_value);
  }
}
"##,
    )
    .unwrap_err();
    assert!(errors
        .iter()
        .any(|error| error.message.contains("unknown identifier `branch_value`")));
}

#[test]
fn return_inside_if_exits_sample_early() {
    let script = compile(
        r##"
effect EarlyReturn {
  color sample(float progress, float seconds, Fixture fixture, Pixel pixel) {
if (progress == 0.25) {
  return #010203;
}
return #ffffff;
  }
}
"##,
    )
    .unwrap();

    assert_eq!(sample(&script).unwrap(), Color::new(1, 2, 3));
}

#[test]
fn boolean_equality_and_logical_operators_work_with_short_circuiting() {
    let script = compile(
        r##"
effect Logic {
  color sample(float progress, float seconds, Fixture fixture, Pixel pixel) {
bool numeric_equal = 1 == 1.0;
bool numeric_not_equal = 2 != 3.0;
bool bool_equal = true == !false;
bool skipped_and = false && (1 / 0 == 0);
bool skipped_or = true || (1 / 0 == 0);
if (numeric_equal && numeric_not_equal && bool_equal && !skipped_and && skipped_or) {
  return #050607;
}
return #000000;
  }
}
"##,
    )
    .unwrap();

    assert_eq!(sample(&script).unwrap(), Color::new(5, 6, 7));
}

#[test]
fn conditions_must_be_bool() {
    let errors = compile(
        r##"
effect BadIf {
  color sample(float progress, float seconds, Fixture fixture, Pixel pixel) {
if (1) {
  return #ffffff;
}
return #000000;
  }
}
"##,
    )
    .unwrap_err();

    assert!(errors
        .iter()
        .any(|error| error.message.contains("if condition must be bool")));
}

#[test]
fn equality_rejects_unsupported_types() {
    let errors = compile(
        r##"
effect BadEquality {
  color sample(float progress, float seconds, Fixture fixture, Pixel pixel) {
bool same = #000000 == #ffffff;
return #000000;
  }
}
"##,
    )
    .unwrap_err();

    assert!(errors.iter().any(|error| error
        .message
        .contains("cannot apply binary operator to color and color")));
}

#[test]
fn for_loop_requires_parentheses() {
    let errors = compile(
        r##"
effect OldFor {
  color sample(float progress, float seconds, Fixture fixture, Pixel pixel) {
int value = 0;
for int i = 0; i < 3; i = i + 1 {
  value = value + 1;
}
return rgb(value, value, value);
  }
}
"##,
    )
    .unwrap_err();

    assert!(errors
        .iter()
        .any(|error| error.message.contains("expected `(`")));
}
