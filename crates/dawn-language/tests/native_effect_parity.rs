use dawn_language::dsl::{
    GeneratorContext, Identifier, RunContext, TargetItemValue, TargetPixelValue, TargetValue,
    Value, compile_effects,
};
use dawn_language::effect::BuiltinEffect;
use dawn_language::native_effect::{self, BoundNativeEffect};
use dawn_language::values::{Color, Curve, CurvePoint, DawnTime, Gradient, GradientStop, Marks};
use indexmap::IndexMap;
use std::sync::Arc;
use std::time::Duration;

fn id(value: &str) -> Identifier {
    Identifier::new(value.to_string()).unwrap()
}
fn curve() -> Arc<Curve> {
    Arc::new(Curve {
        points: vec![
            CurvePoint {
                position: 0.0,
                value: 0.0,
            },
            CurvePoint {
                position: 0.2,
                value: 1.0,
            },
            CurvePoint {
                position: 1.0,
                value: 0.0,
            },
        ],
    })
}
fn gradient() -> Arc<Gradient> {
    Arc::new(Gradient {
        stops: vec![
            GradientStop {
                position: 0.0,
                color: Color {
                    red: 255,
                    green: 30,
                    blue: 10,
                },
            },
            GradientStop {
                position: 1.0,
                color: Color {
                    red: 10,
                    green: 80,
                    blue: 255,
                },
            },
        ],
    })
}
fn target() -> Arc<TargetValue> {
    Arc::new(TargetValue {
        groups: vec![Arc::new(TargetItemValue {
            pixels: Arc::new(
                (0..24)
                    .map(|pixel| TargetPixelValue {
                        element_index: 0,
                        element_cell_index: pixel,
                        pixel_index: pixel,
                        pixel_count: 24,
                        pixel_fraction: pixel as f64 / 23.0,
                    })
                    .collect(),
            ),
        })],
    })
}

#[test]
fn mark_pulse_matches_reference_schedule_and_samples() {
    let marks = Arc::new(Marks {
        marks: vec![
            DawnTime(Duration::from_secs_f64(0.5)),
            DawnTime(Duration::from_secs_f64(1.25)),
        ],
    });
    let params = IndexMap::from([
        (id("beats"), Value::Marks(marks)),
        (
            id("base"),
            Value::Color(Color {
                red: 2,
                green: 3,
                blue: 4,
            }),
        ),
        (id("accent"), Value::Gradient(gradient())),
        (id("hue"), Value::Curve(curve())),
        (id("hue_mix"), Value::Float(0.35)),
        (id("offset_seconds"), Value::Float(0.1)),
        (id("decay_seconds"), Value::Float(0.3)),
        (id("section_width_pixels"), Value::Int(5)),
        (id("section_edge_fade_pixels"), Value::Float(1.0)),
        (id("sections_per_mark"), Value::Int(3)),
        (id("seed"), Value::Float(29.0)),
    ]);
    let effects =
        compile_effects(include_str!("fixtures/native_effect_reference.effect.dawn")).unwrap();
    let generator = effects
        .iter()
        .find(|effect| effect.name().as_str() == "MarkPulse")
        .unwrap();
    let child = effects
        .iter()
        .find(|effect| effect.name().as_str() == "MarkPulseChild")
        .unwrap();
    let context = GeneratorContext {
        duration: 4.0,
        target: target(),
    };
    let reference = generator
        .generate_bound(
            &generator.bind_params(&params),
            &context,
            &mut Default::default(),
        )
        .unwrap();
    let BoundNativeEffect::MarkPulse(native) =
        native_effect::bind(BuiltinEffect::MarkPulse, &params).unwrap()
    else {
        panic!()
    };
    let generated = BoundNativeEffect::MarkPulse(native)
        .generate(&context)
        .unwrap();
    assert_eq!(generated.len(), reference.len());
    for (native, reference) in generated.iter().zip(&reference) {
        assert_eq!(native.start_seconds, reference.start_seconds);
        assert_eq!(native.duration_seconds, reference.duration_seconds);
        assert_eq!(native.target, reference.target);
        let bound = child.bind_params(&reference.params);
        for pixel in native.target.pixels.iter() {
            for progress in [0.0, 0.25, 0.75, 1.0] {
                let context = RunContext {
                    progress,
                    seconds: progress * native.duration_seconds,
                    duration: native.duration_seconds,
                    pixel_index: pixel.pixel_index,
                    pixel_count: pixel.pixel_count,
                    pixel_fraction: pixel.pixel_fraction,
                    global_marks: Marks { marks: Vec::new() },
                };
                assert_eq!(
                    native.sample.sample(&context).unwrap(),
                    child
                        .sample_bound(&bound, &context, &mut Default::default())
                        .unwrap()
                );
            }
        }
    }
}
