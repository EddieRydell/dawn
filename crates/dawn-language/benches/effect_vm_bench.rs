use criterion::{Criterion, criterion_group, criterion_main};
use dawn_language::dsl::{
    CompiledEffect, DslBindCache, DslVmScratch, GeneratorContext, Identifier, RunContext,
    SignalSampler, TargetItemValue, TargetPixelValue, TargetValue, Value, compile_effects,
    compile_operators,
};
use dawn_language::effect::BuiltinEffect;
use dawn_language::native_effect::{self, BoundNativeEffect};
use dawn_language::values::{Color, Curve, CurvePoint, DawnTime, Gradient, GradientStop, Marks};
use indexmap::IndexMap;
use std::hint::black_box;
use std::sync::Arc;
use std::time::Duration;

const PULSE_SOURCE: &str = include_str!("../tests/fixtures/native_effect_reference.effect.dawn");
const SCAN_SWEEP_SOURCE: &str =
    include_str!("../../../examples/thirty-output-controller/effects/scan-sweep.effect.dawn");
const IMPACT_BURST_SOURCE: &str =
    include_str!("../../../examples/thirty-output-controller/effects/impact-burst.effect.dawn");
const SPARKLE_COMET_SOURCE: &str =
    include_str!("../../../examples/thirty-output-controller/effects/sparkle-comet.effect.dawn");
const SHIMMER_FIELD_SOURCE: &str =
    include_str!("../../../examples/thirty-output-controller/effects/shimmer-field.effect.dawn");

fn bench_effect_vm(c: &mut Criterion) {
    let constant = sample_effect(
        "ConstantColor",
        "effect ConstantColor { color sample() { return #336699; } }",
    );
    let constant_params = IndexMap::new();
    let constant_bound = constant.bind_params(&constant_params);

    c.bench_function("constant_color_floor", |b| {
        let context = sample_context();
        let mut scratch = DslVmScratch::default();
        b.iter(|| {
            black_box(
                constant
                    .sample_bound(
                        black_box(&constant_bound),
                        black_box(&context),
                        &mut scratch,
                    )
                    .expect("constant color sample should run"),
            )
        });
    });

    bench_native_sample(
        c,
        "pulse_curve_sample",
        BuiltinEffect::Pulse,
        pulse_params(),
    );
    bench_native_sample(c, "chase_sample", BuiltinEffect::Chase, chase_params());
    bench_native_sample(c, "spin_sample", BuiltinEffect::Spin, spin_params());
    bench_sample(
        c,
        "scan_sweep_branch_sample",
        "ScanSweep",
        SCAN_SWEEP_SOURCE,
        scan_sweep_params(),
    );
    bench_sample(
        c,
        "impact_burst_branch_sample",
        "ImpactBurst",
        IMPACT_BURST_SOURCE,
        impact_burst_params(),
    );
    bench_sample(
        c,
        "sparkle_comet_rand_trig_sample",
        "SparkleComet",
        SPARKLE_COMET_SOURCE,
        sparkle_comet_params(),
    );
    bench_sample(
        c,
        "shimmer_field_rand_trig_sample",
        "ShimmerField",
        SHIMMER_FIELD_SOURCE,
        shimmer_field_params(),
    );
    bench_native_generated_sample(
        c,
        "mark_chase_child_dense_sample",
        BuiltinEffect::MarkChase,
        mark_chase_params(),
    );
    bench_native_generated_sample(
        c,
        "mark_pulse_child_sample",
        BuiltinEffect::MarkPulse,
        mark_pulse_params(),
    );
    bench_native_generator(
        c,
        "mark_pulse_generator",
        BuiltinEffect::MarkPulse,
        mark_pulse_params(),
    );
    bench_native_generator(
        c,
        "mark_chase_generator",
        BuiltinEffect::MarkChase,
        mark_chase_params(),
    );
    bench_operator(c);
    bench_binding(c);
}

fn bench_native_sample(
    c: &mut Criterion,
    name: &str,
    builtin: BuiltinEffect,
    params: IndexMap<Identifier, Value>,
) {
    let BoundNativeEffect::Sample(sample) =
        native_effect::bind(builtin, &params).expect("native sample should bind")
    else {
        panic!("sample builtin should bind as sample")
    };
    let context = sample_context();
    c.bench_function(name, |b| {
        b.iter(|| {
            black_box(
                sample
                    .sample(black_box(&context))
                    .expect("native sample should run"),
            )
        })
    });
}

fn bench_native_generator(
    c: &mut Criterion,
    name: &str,
    builtin: BuiltinEffect,
    params: IndexMap<Identifier, Value>,
) {
    let bound = native_effect::bind(builtin, &params).expect("native generator should bind");
    let context = generator_context();
    c.bench_function(name, |b| {
        b.iter(|| {
            black_box(
                bound
                    .generate(black_box(&context))
                    .expect("native generator should run"),
            )
        })
    });
}

fn bench_native_generated_sample(
    c: &mut Criterion,
    name: &str,
    builtin: BuiltinEffect,
    params: IndexMap<Identifier, Value>,
) {
    let bound = native_effect::bind(builtin, &params).expect("native generator should bind");
    let sample = bound
        .generate(&generator_context())
        .expect("native generator should run")
        .into_iter()
        .next()
        .expect("native generator should emit")
        .sample;
    let context = sample_context();
    c.bench_function(name, |b| {
        b.iter(|| {
            black_box(
                sample
                    .sample(black_box(&context))
                    .expect("native child should sample"),
            )
        })
    });
}

fn bench_operator(c: &mut Criterion) {
    let operator = compile_operators(include_str!(
        "../../../examples/thirty-output-controller/operators/gain.operator.dawn"
    ))
    .expect("Gain operator source should compile")
    .into_iter()
    .next()
    .expect("Gain operator should exist");
    let bound = operator.bind_params(&params([("amount", Value::Float(0.65))]));
    let context = sample_context();
    let mut sampler = ConstantSignalSampler;
    let mut scratch = DslVmScratch::default();

    c.bench_function("operator_gain_signal_sample", |b| {
        b.iter(|| {
            black_box(
                operator
                    .sample_bound(
                        black_box(&bound),
                        black_box(&context),
                        &mut sampler,
                        &mut scratch,
                    )
                    .expect("Gain operator sample should run"),
            )
        });
    });

    let contexts = (0..512)
        .map(|pixel_index| RunContext {
            pixel_index,
            pixel_count: 512,
            pixel_fraction: pixel_index as f64 / 511.0,
            ..sample_context()
        })
        .collect::<Vec<_>>();
    c.bench_function("operator_gain_signal_512_pixels", |b| {
        b.iter(|| {
            for context in &contexts {
                black_box(
                    operator
                        .sample_bound(
                            black_box(&bound),
                            black_box(context),
                            &mut sampler,
                            &mut scratch,
                        )
                        .expect("Gain operator sample should run"),
                );
            }
        });
    });
}

struct ConstantSignalSampler;

impl SignalSampler for ConstantSignalSampler {
    fn sample_signal(
        &mut self,
        _input: usize,
        _seconds: f64,
        _pixel_index: usize,
    ) -> Result<Color, dawn_language::dsl::RuntimeError> {
        Ok(Color {
            red: 120,
            green: 80,
            blue: 40,
        })
    }
}

fn bench_sample(
    c: &mut Criterion,
    bench_name: &str,
    effect_name: &str,
    source: &str,
    params: IndexMap<Identifier, Value>,
) {
    let effect = sample_effect(effect_name, source);
    let bound = effect.bind_params(&params);
    let context = sample_context();
    let mut scratch = DslVmScratch::default();
    effect
        .sample_bound(&bound, &context, &mut scratch)
        .expect("sample benchmark effect should run");

    c.bench_function(bench_name, |b| {
        b.iter(|| {
            black_box(
                effect
                    .sample_bound(black_box(&bound), black_box(&context), &mut scratch)
                    .expect("sample benchmark effect should run"),
            )
        });
    });
}

fn bench_binding(c: &mut Criterion) {
    let effect = sample_effect("Pulse", PULSE_SOURCE);
    let params = pulse_params();

    c.bench_function("bind_curve_params_uncached", |b| {
        b.iter(|| black_box(effect.bind_params(black_box(&params))));
    });

    c.bench_function("bind_curve_params_cached", |b| {
        let mut cache = DslBindCache::default();
        b.iter(|| black_box(effect.bind_params_cached(black_box(&params), &mut cache)));
    });
}

fn sample_effect(effect_name: &str, source: &str) -> CompiledEffect {
    compile_effects(source)
        .expect("effect source should compile")
        .into_iter()
        .find(|effect| effect.name().as_str() == effect_name)
        .expect("compiled effect should exist")
}

fn sample_context() -> RunContext {
    RunContext {
        progress: 0.58,
        seconds: 3.25,
        duration: 8.0,
        pixel_index: 47,
        pixel_count: 180,
        pixel_fraction: 47.0 / 179.0,
        global_marks: marks(),
    }
}

fn generator_context() -> GeneratorContext {
    GeneratorContext {
        duration: 24.0,
        target: Arc::new(TargetValue {
            groups: target_groups(24, 12),
        }),
    }
}

fn target_groups(group_count: i64, pixels_per_group: i64) -> Vec<Arc<TargetItemValue>> {
    let pixel_count = group_count * pixels_per_group;
    (0..group_count)
        .map(|fixture_index| {
            Arc::new(TargetItemValue {
                pixels: Arc::new(
                    (0..pixels_per_group)
                        .map(|fixture_pixel_index| {
                            let pixel_index =
                                fixture_index * pixels_per_group + fixture_pixel_index;
                            TargetPixelValue {
                                fixture_index,
                                fixture_pixel_index,
                                pixel_index,
                                pixel_count,
                                pixel_fraction: pixel_index as f64 / (pixel_count - 1) as f64,
                            }
                        })
                        .collect(),
                ),
            })
        })
        .collect()
}

fn pulse_params() -> IndexMap<Identifier, Value> {
    params([
        ("gradient", Value::Gradient(gradient())),
        ("pulse_shape", Value::Curve(curve())),
    ])
}

fn chase_params() -> IndexMap<Identifier, Value> {
    params([
        ("gradient", Value::Gradient(gradient())),
        ("gradient_mode", enum_value("per_pulse")),
        ("pulse_overlap", Value::Float(8.0)),
        ("section_width_pixels", Value::Int(5)),
        ("chase_position", Value::Curve(alternate_curve())),
        ("reverse", Value::Bool(false)),
        ("extend_to_start", Value::Bool(false)),
        ("extend_to_end", Value::Bool(false)),
        ("pulse_shape", Value::Curve(curve())),
    ])
}

fn spin_params() -> IndexMap<Identifier, Value> {
    let mut params = chase_params();
    params.insert(identifier("revolutions"), Value::Int(3));
    params
}

fn scan_sweep_params() -> IndexMap<Identifier, Value> {
    params([
        ("gradient", Value::Gradient(gradient())),
        ("intensity", Value::Curve(curve())),
        ("direction", enum_value("center_out")),
        ("color_mode", enum_value("scan_head")),
        ("repeats", Value::Float(3.0)),
        ("width", Value::Float(0.18)),
        ("edge_width", Value::Float(0.08)),
        ("background_level", Value::Float(0.05)),
        ("section_width_pixels", Value::Float(6.0)),
    ])
}

fn impact_burst_params() -> IndexMap<Identifier, Value> {
    params([
        ("gradient", Value::Gradient(gradient())),
        ("intensity", Value::Curve(curve())),
        ("direction", enum_value("outward")),
        ("color_mode", enum_value("from_edge")),
        ("center_position", Value::Float(0.5)),
        ("start_radius", Value::Float(0.0)),
        ("end_radius", Value::Float(0.75)),
        ("edge_width", Value::Float(0.2)),
        ("glow_width", Value::Float(0.25)),
        ("glow_level", Value::Float(0.35)),
        ("mirror", Value::Bool(false)),
    ])
}

fn sparkle_comet_params() -> IndexMap<Identifier, Value> {
    params([
        ("gradient", Value::Gradient(gradient())),
        ("position", Value::Curve(curve())),
        ("intensity", Value::Curve(curve())),
        ("color_mode", enum_value("rainbow_hue")),
        (
            "head_color",
            Value::Color(Color {
                red: 255,
                green: 255,
                blue: 255,
            }),
        ),
        ("reverse", Value::Bool(false)),
        ("wrap", Value::Bool(true)),
        ("tail_width", Value::Float(0.22)),
        ("head_width", Value::Float(0.04)),
        ("sparkle_density", Value::Float(0.45)),
        ("sparkle_speed", Value::Float(18.0)),
        ("sparkle_gain", Value::Float(0.8)),
        ("seed", Value::Float(13.0)),
    ])
}

fn shimmer_field_params() -> IndexMap<Identifier, Value> {
    params([
        (
            "base",
            Value::Color(Color {
                red: 0,
                green: 0,
                blue: 0,
            }),
        ),
        ("palette", Value::Gradient(gradient())),
        ("color_mode", enum_value("from_palette")),
        (
            "sparkle_color",
            Value::Color(Color {
                red: 255,
                green: 255,
                blue: 255,
            }),
        ),
        ("density", Value::Float(0.35)),
        ("shimmer_level", Value::Float(0.4)),
        ("sparkle_level", Value::Float(1.0)),
        ("wave_speed", Value::Float(5.0)),
        ("wave_scale", Value::Float(21.0)),
        ("sparkle_speed", Value::Float(10.0)),
        ("sparkle_hold", Value::Float(0.35)),
        ("section_width_pixels", Value::Float(5.0)),
        ("seed", Value::Float(17.0)),
    ])
}

fn mark_pulse_params() -> IndexMap<Identifier, Value> {
    params([
        ("beats", Value::Marks(Arc::new(marks()))),
        (
            "base",
            Value::Color(Color {
                red: 0,
                green: 0,
                blue: 0,
            }),
        ),
        ("accent", Value::Gradient(gradient())),
        ("hue", Value::Curve(curve())),
        ("hue_mix", Value::Float(0.35)),
        ("offset_seconds", Value::Float(0.0)),
        ("decay_seconds", Value::Float(0.24)),
        ("section_width_pixels", Value::Int(5)),
        ("section_edge_fade_pixels", Value::Float(1.0)),
        ("sections_per_mark", Value::Int(3)),
        ("seed", Value::Float(29.0)),
    ])
}

fn mark_chase_params() -> IndexMap<Identifier, Value> {
    params([
        ("beats", Value::Marks(Arc::new(marks()))),
        (
            "base",
            Value::Color(Color {
                red: 0,
                green: 0,
                blue: 0,
            }),
        ),
        ("gradient_mode", enum_value("per_pulse")),
        (
            "gradients",
            gradient_array([gradient(), alternate_gradient()]),
        ),
        ("hue", Value::Curve(curve())),
        ("hue_mix", Value::Float(0.35)),
        ("offset_seconds", Value::Float(0.0)),
        ("chase_seconds", Value::Float(0.5)),
        ("pulse_overlap", Value::Float(8.0)),
        ("section_width_pixels", Value::Int(5)),
        ("chase_positions", curve_array([curve(), alternate_curve()])),
        ("pulse_shape", Value::Curve(curve())),
    ])
}

fn params<const N: usize>(items: [(&str, Value); N]) -> IndexMap<Identifier, Value> {
    items
        .into_iter()
        .map(|(key, value)| (identifier(key), value))
        .collect()
}

fn curve_array<const N: usize>(curves: [Arc<Curve>; N]) -> Value {
    Value::Array(Arc::new(
        curves.into_iter().map(Value::Curve).collect::<Vec<_>>(),
    ))
}

fn gradient_array<const N: usize>(gradients: [Arc<Gradient>; N]) -> Value {
    Value::Array(Arc::new(
        gradients
            .into_iter()
            .map(Value::Gradient)
            .collect::<Vec<_>>(),
    ))
}

fn enum_value(value: &str) -> Value {
    Value::Enum(identifier(value))
}

fn identifier(value: &str) -> Identifier {
    Identifier::new(value.to_string()).expect("identifier should be valid")
}

fn marks() -> Marks {
    Marks {
        marks: (0..16)
            .map(|index| DawnTime(Duration::from_secs_f64(index as f64 * 0.375)))
            .collect(),
    }
}

fn curve() -> Arc<Curve> {
    Arc::new(Curve {
        points: vec![
            CurvePoint {
                position: 0.0,
                value: 0.0,
            },
            CurvePoint {
                position: 0.12,
                value: 1.0,
            },
            CurvePoint {
                position: 0.45,
                value: 0.35,
            },
            CurvePoint {
                position: 0.78,
                value: 0.8,
            },
            CurvePoint {
                position: 1.0,
                value: 0.0,
            },
        ],
    })
}

fn alternate_curve() -> Arc<Curve> {
    Arc::new(Curve {
        points: vec![
            CurvePoint {
                position: 0.0,
                value: 1.0,
            },
            CurvePoint {
                position: 0.25,
                value: 0.15,
            },
            CurvePoint {
                position: 0.65,
                value: 0.9,
            },
            CurvePoint {
                position: 1.0,
                value: 0.2,
            },
        ],
    })
}

fn gradient() -> Arc<Gradient> {
    Arc::new(Gradient {
        stops: vec![
            color_point(0.0, 255, 32, 16),
            color_point(0.28, 255, 184, 24),
            color_point(0.58, 24, 220, 255),
            color_point(1.0, 160, 64, 255),
        ],
    })
}

fn alternate_gradient() -> Arc<Gradient> {
    Arc::new(Gradient {
        stops: vec![
            color_point(0.0, 0, 20, 255),
            color_point(0.35, 0, 255, 180),
            color_point(0.7, 255, 255, 64),
            color_point(1.0, 255, 64, 96),
        ],
    })
}

fn color_point(position: f64, red: u8, green: u8, blue: u8) -> GradientStop {
    GradientStop {
        position,
        color: Color { red, green, blue },
    }
}

criterion_group!(benches, bench_effect_vm);
criterion_main!(benches);
