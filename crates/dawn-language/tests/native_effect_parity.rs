use dawn_language::dsl::{
    GeneratorContext, Identifier, RunContext, TargetItemValue, TargetPixelValue, TargetValue,
    Value, compile_effects,
};
use dawn_language::effect::BuiltinEffect;
use dawn_language::native_effect::{self, BoundNativeEffect};
use dawn_language::values::{
    Color, Curve, CurvePoint, Gradient, GradientStop, Marks, SampleDuration, SampleTime,
};
use indexmap::IndexMap;
use std::sync::Arc;

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
            pixels: Arc::from(
                (0..24)
                    .map(|pixel| TargetPixelValue {
                        element_index: 0,
                        element_cell_index: pixel,
                        pixel_index: pixel,
                        pixel_count: 24,
                        pixel_fraction: pixel as f32 / 23.0,
                    })
                    .collect::<Vec<_>>(),
            ),
        })],
    })
}

#[test]
fn mark_pulse_matches_reference_schedule_and_samples() {
    let marks = Arc::new(Marks {
        marks: vec![
            SampleDuration::from_ticks(500_000),
            SampleDuration::from_ticks(1_250_000),
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
        start_time: SampleTime::from_ticks(1_000_000),
        duration: SampleDuration::from_ticks(4_000_000),
        target: target(),
    };
    let reference = generator
        .generate_bound(
            &generator.bind_params(&params).unwrap(),
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
        assert_eq!(native.start_time, reference.start_time);
        assert_eq!(native.duration, reference.duration);
        assert_eq!(native.target, reference.target);
        let bound = child.bind_params_pairs(&reference.params).unwrap();
        for pixel in native.target.pixels.iter() {
            for progress in [0.0, 0.25, 0.75, 1.0] {
                let context = RunContext {
                    progress,
                    time: SampleDuration::from_ticks(
                        (progress * native.duration.ticks() as f32).round() as u32,
                    ),
                    duration: native.duration,
                    pixel_index: pixel.pixel_index,
                    pixel_count: pixel.pixel_count,
                    pixel_fraction: pixel.pixel_fraction,
                };
                let sample_time = native
                    .start_time
                    .checked_add_duration(context.time)
                    .unwrap();
                assert_eq!(
                    native.sample.sample(&context, sample_time).unwrap(),
                    child
                        .sample_bound(&bound, &context, &mut Default::default())
                        .unwrap()
                );
            }
        }
    }
}
