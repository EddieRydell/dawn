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
