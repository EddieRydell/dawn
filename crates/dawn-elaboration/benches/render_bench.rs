use camino::Utf8PathBuf;
use criterion::{BatchSize, Criterion, criterion_group, criterion_main};
use dawn_elaboration::{
    PreparedSequence, PreparedSequenceOutput, RenderedFrame, elaborate_sequence,
};
use dawn_language::values::{Color, sample_time_from_frame};
use dawn_project_io::load_package;
use std::hint::black_box;
use std::time::Duration;

const BENCHMARK_SEQUENCE_DOCUMENT: &str = "sequences/layer_test.sequence.dawn";
const BENCHMARK_SEQUENCE_OBJECT: &str = "layer_test";
const PLAYBACK_START_FRAME: u32 = 8420;
const PLAYBACK_FRAME_COUNT: u32 = 60;

const SCENARIOS: [RenderScenario; 7] = [
    RenderScenario {
        frame: 8398,
        checksum: 0x8bb5_7d05_87a6_9ae8,
        active_effect_count: 15,
    },
    RenderScenario {
        frame: 8450,
        checksum: 0x5bee_7460_eba9_0468,
        active_effect_count: 30,
    },
    RenderScenario {
        frame: 8494,
        checksum: 0xadc5_9683_e46e_175f,
        active_effect_count: 32,
    },
    RenderScenario {
        frame: 8530,
        checksum: 0xfe52_aa76_b372_c103,
        active_effect_count: 3,
    },
    RenderScenario {
        frame: 9270,
        checksum: 0x520a_4dfc_5977_d97a,
        active_effect_count: 2,
    },
    RenderScenario {
        frame: 9504,
        checksum: 0x9dca_f48a_50e4_f8df,
        active_effect_count: 1,
    },
    RenderScenario {
        frame: 9650,
        checksum: 0x2319_7d72_88c3_6a09,
        active_effect_count: 2,
    },
];

#[derive(Clone, Copy)]
struct RenderScenario {
    frame: u32,
    checksum: u64,
    active_effect_count: usize,
}

fn bench_render(c: &mut Criterion) {
    pin_benchmark_thread();
    let session = load_package(&project_path())
        .expect("benchmark project should load")
        .session;
    let setup_id = &session.project.root.setup;
    let sequence_id = session
        .project
        .root
        .sequences
        .iter()
        .find(|id| {
            id.0.document().as_str() == BENCHMARK_SEQUENCE_DOCUMENT
                && id.0.object() == BENCHMARK_SEQUENCE_OBJECT
        })
        .expect("benchmark project should include the layer_test sequence");
    let renderer = elaborate_sequence(&session.project, setup_id, sequence_id)
        .expect("benchmark project should prepare");
    let output = PreparedSequenceOutput::prepare(&session.project, setup_id, sequence_id)
        .expect("benchmark controller output should prepare");
    assert_scenarios(&renderer);

    c.bench_function("prepare_starter", |b| {
        b.iter(|| {
            black_box(
                elaborate_sequence(
                    black_box(&session.project),
                    black_box(setup_id),
                    black_box(sequence_id),
                )
                .expect("benchmark project should prepare"),
            )
        });
    });

    let mut scenario_workspace = renderer.workspace();
    c.bench_function("render_representative_frames", |b| {
        b.iter(|| {
            for scenario in SCENARIOS {
                black_box(
                    renderer
                        .evaluate_frame_with_workspace(
                            black_box(scenario.frame),
                            &mut scenario_workspace,
                        )
                        .expect("benchmark frame should render"),
                );
            }
        });
    });

    let mut playback_workspace = renderer.workspace();
    c.bench_function("render_playback_dense_60_frames", |b| {
        b.iter(|| {
            for frame in PLAYBACK_START_FRAME..PLAYBACK_START_FRAME + PLAYBACK_FRAME_COUNT {
                black_box(
                    renderer
                        .evaluate_frame_with_workspace(black_box(frame), &mut playback_workspace)
                        .expect("benchmark playback frame should render"),
                );
            }
        });
    });

    c.bench_function("render_playback_dense_cold_60_frames", |b| {
        b.iter_batched(
            || renderer.workspace(),
            |mut workspace| {
                for frame in PLAYBACK_START_FRAME..PLAYBACK_START_FRAME + PLAYBACK_FRAME_COUNT {
                    black_box(
                        renderer
                            .evaluate_frame_with_workspace(black_box(frame), &mut workspace)
                            .expect("benchmark cold dense playback frame should render"),
                    );
                }
            },
            BatchSize::SmallInput,
        );
    });

    let mut output_workspace = output.workspace();
    c.bench_function("controller_output_dense_60_frames", |b| {
        b.iter(|| {
            for frame in PLAYBACK_START_FRAME..PLAYBACK_START_FRAME + PLAYBACK_FRAME_COUNT {
                let sample_time = sample_time_from_frame(frame, output.frame_rate())
                    .expect("benchmark frame should fit the controller clock");
                black_box(
                    output
                        .sample_into(black_box(sample_time), &mut output_workspace)
                        .expect("benchmark controller frame should render"),
                );
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

fn assert_scenarios(renderer: &PreparedSequence) {
    for scenario in SCENARIOS {
        let rendered = renderer
            .evaluate_frame(scenario.frame)
            .expect("benchmark frame should render");
        assert_eq!(checksum_frame(&rendered), scenario.checksum);
        assert_eq!(
            renderer.active_effect_count_at_frame(scenario.frame),
            scenario.active_effect_count
        );
    }
}

fn project_path() -> Utf8PathBuf {
    Utf8PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join("examples/starter")
}

fn checksum_frame(frame: &RenderedFrame) -> u64 {
    let mut hash = 0xcbf2_9ce4_8422_2325u64;
    hash = checksum_u64(hash, u64::from(frame.frame_index));
    for element in &frame.elements {
        hash = checksum_u32(hash, element.element_id);
        hash = checksum_colors_with_seed(hash, &element.pixels);
    }
    hash
}

fn checksum_colors_with_seed(hash: u64, colors: &[Color]) -> u64 {
    colors
        .iter()
        .fold(hash, |hash, color| checksum_color(hash, *color))
}

fn checksum_color(hash: u64, color: Color) -> u64 {
    [color.red, color.green, color.blue]
        .into_iter()
        .fold(hash, checksum_u8)
}

fn checksum_u64(hash: u64, value: u64) -> u64 {
    value.to_le_bytes().into_iter().fold(hash, checksum_u8)
}

fn checksum_u32(hash: u64, value: u32) -> u64 {
    value.to_le_bytes().into_iter().fold(hash, checksum_u8)
}

fn checksum_u8(hash: u64, value: u8) -> u64 {
    (hash ^ u64::from(value)).wrapping_mul(0x0000_0100_0000_01b3)
}

fn criterion_config() -> Criterion {
    Criterion::default()
        .warm_up_time(Duration::from_secs(3))
        .measurement_time(Duration::from_secs(5))
        .noise_threshold(0.05)
}

criterion_group! {
    name = benches;
    config = criterion_config();
    targets = bench_render
}
criterion_main!(benches);
