use criterion::{Criterion, criterion_group, criterion_main};
use dawn_language::dsl::{
    BoundParams, CompiledEffect, DslVmScratch, Identifier, RunContext, Value, compile_effects,
};
use dawn_language::values::{Color, Curve, CurvePoint, Gradient, GradientStop, SampleDuration};
use indexmap::IndexMap;
use std::hint::black_box;
use std::sync::Arc;
use std::time::Duration;

const PIXEL_BATCH: i32 = 512;
const SCAN_SWEEP_SOURCE: &str =
    include_str!("../../../examples/starter/effects/scan-sweep.effect.dawn");
const IMPACT_BURST_SOURCE: &str =
    include_str!("../../../examples/starter/effects/impact-burst.effect.dawn");
const SPARKLE_COMET_SOURCE: &str =
    include_str!("../../../examples/starter/effects/sparkle-comet.effect.dawn");
const SHIMMER_FIELD_SOURCE: &str =
    include_str!("../../../examples/starter/effects/shimmer-field.effect.dawn");

fn bench_effect_vm(c: &mut Criterion) {
    pin_benchmark_thread();
    let effects = [
        prepared_effect("ScanSweep", SCAN_SWEEP_SOURCE, scan_sweep_params()),
        prepared_effect("ImpactBurst", IMPACT_BURST_SOURCE, impact_burst_params()),
        prepared_effect("SparkleComet", SPARKLE_COMET_SOURCE, sparkle_comet_params()),
        prepared_effect("ShimmerField", SHIMMER_FIELD_SOURCE, shimmer_field_params()),
    ];
    let contexts = sample_contexts();
    let mut scratches = std::array::from_fn::<_, 4, _>(|_| DslVmScratch::default());

    c.bench_function("dsl_effect_suite_4x512_pixels", |b| {
        b.iter(|| {
            for ((effect, bound), scratch) in effects.iter().zip(&mut scratches) {
                for context in &contexts {
                    black_box(
                        effect
                            .sample_bound(black_box(bound), black_box(context), scratch)
                            .expect("sample benchmark effect should run"),
                    );
                }
            }
        });
    });
}

#[cfg(windows)]
fn pin_benchmark_thread() {
    use std::ffi::c_void;

    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn GetCurrentThread() -> *mut c_void;
        fn SetThreadAffinityMask(thread: *mut c_void, affinity_mask: usize) -> usize;
        fn SetThreadPriority(thread: *mut c_void, priority: i32) -> i32;
    }

    // Logical CPU 0 commonly handles extra OS work. A fixed nonzero CPU also prevents
    // migrations between unlike cores; smaller systems fall back to their final CPU.
    let cpu = std::thread::available_parallelism()
        .map(|count| 2.min(count.get().saturating_sub(1)))
        .unwrap_or(0);
    let thread = unsafe { GetCurrentThread() };
    let previous = unsafe { SetThreadAffinityMask(thread, 1usize << cpu) };
    assert_ne!(previous, 0, "benchmark thread affinity should be set");
    assert_ne!(
        unsafe { SetThreadPriority(thread, 2) },
        0,
        "benchmark thread priority should be raised"
    );
}

#[cfg(not(windows))]
fn pin_benchmark_thread() {}

fn prepared_effect(
    effect_name: &str,
    source: &str,
    params: IndexMap<Identifier, Value>,
) -> (CompiledEffect, BoundParams) {
    let effect = sample_effect(effect_name, source);
    let bound = effect.bind_params(&params).expect("valid params");
    (effect, bound)
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
        time: SampleDuration::from_ticks(3_250_000),
        duration: SampleDuration::from_ticks(8_000_000),
        pixel_index: 47,
        pixel_count: 180,
        pixel_fraction: 47.0 / 179.0,
    }
}

fn sample_contexts() -> Vec<RunContext> {
    (0..PIXEL_BATCH)
        .map(|pixel_index| RunContext {
            pixel_index,
            pixel_count: PIXEL_BATCH,
            pixel_fraction: pixel_index as f32 / (PIXEL_BATCH - 1) as f32,
            ..sample_context()
        })
        .collect()
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

fn params<const N: usize>(items: [(&str, Value); N]) -> IndexMap<Identifier, Value> {
    items
        .into_iter()
        .map(|(key, value)| (identifier(key), value))
        .collect()
}

fn enum_value(value: &str) -> Value {
    Value::Enum(identifier(value))
}

fn identifier(value: &str) -> Identifier {
    Identifier::new(value.to_string()).expect("identifier should be valid")
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

fn color_point(position: f32, red: u8, green: u8, blue: u8) -> GradientStop {
    GradientStop {
        position,
        color: Color { red, green, blue },
    }
}

fn criterion_config() -> Criterion {
    Criterion::default()
        .warm_up_time(Duration::from_secs(8))
        .measurement_time(Duration::from_secs(10))
        .noise_threshold(0.05)
}

criterion_group! {
    name = benches;
    config = criterion_config();
    targets = bench_effect_vm
}
criterion_main!(benches);
