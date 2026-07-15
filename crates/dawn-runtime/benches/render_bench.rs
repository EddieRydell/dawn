use camino::Utf8PathBuf;
use criterion::{BatchSize, Criterion, criterion_group, criterion_main};
use dawn_language::effect::EffectInstId;
use dawn_language::values::Color;
use dawn_project_io::load_project;
use dawn_runtime::{
    EffectRasterRenderScratch, PreparedEffectRasterRenderer, PreparedSequenceRenderer,
    RenderedFrame, SequenceRenderScratch,
};
use std::hint::black_box;

const SCENARIOS: [RenderScenario; 7] = [
    RenderScenario {
        frame: 144,
        checksum: 0xe050_9990_c9eb_6c98,
        active_effect_count: 3,
    },
    RenderScenario {
        frame: 2088,
        checksum: 0x2faa_cffb_1da1_fde8,
        active_effect_count: 30,
    },
    RenderScenario {
        frame: 5904,
        checksum: 0x4f31_7d2c_e784_03b7,
        active_effect_count: 180,
    },
    RenderScenario {
        frame: 9504,
        checksum: 0x2d33_a136_667f_5bad,
        active_effect_count: 60,
    },
    RenderScenario {
        frame: 11520,
        checksum: 0x237b_56c5_a616_b3fb,
        active_effect_count: 360,
    },
    RenderScenario {
        frame: 19080,
        checksum: 0x0f6c_daeb_0baa_0142,
        active_effect_count: 301,
    },
    RenderScenario {
        frame: 7707,
        checksum: 0x94eb_b8f8_8b27_c8f3,
        active_effect_count: 212,
    },
];

const RASTER_SCENARIOS: [RasterScenario; 2] = [
    RasterScenario {
        name: "raster_sample_shimmer_field_75",
        effect_id: 75,
        columns: 256,
        rows: 50,
        checksum: 0x68d2_2650_911c_2f4d,
    },
    RasterScenario {
        name: "raster_generator_mark_pulse_77",
        effect_id: 77,
        columns: 256,
        rows: 50,
        checksum: 0x4802_5415_5f97_fc5b,
    },
];

#[derive(Clone, Copy)]
struct RenderScenario {
    frame: u64,
    checksum: u64,
    active_effect_count: usize,
}

#[derive(Clone, Copy)]
struct RasterScenario {
    name: &'static str,
    effect_id: u32,
    columns: usize,
    rows: usize,
    checksum: u64,
}

fn bench_render(c: &mut Criterion) {
    let session = load_project(&project_path()).expect("benchmark project should load");
    let setup_id = &session.project.root.setup;
    let sequence_id = session
        .project
        .root
        .sequences
        .first()
        .expect("benchmark project should have a root sequence");
    let renderer = PreparedSequenceRenderer::prepare(&session.project, setup_id, sequence_id)
        .expect("benchmark project should prepare");
    let operator_sequence_id = session
        .project
        .root
        .sequences
        .get(1)
        .expect("benchmark project should have an operator sequence");
    let operator_renderer =
        PreparedSequenceRenderer::prepare(&session.project, setup_id, operator_sequence_id)
            .expect("benchmark operator sequence should prepare");

    assert_scenarios(&renderer);
    let raster_renderers = prepare_raster_renderers(&session.project, setup_id, sequence_id);
    assert_raster_scenarios(&raster_renderers);
    let operator_frame = operator_renderer
        .render_frame(3594)
        .expect("benchmark operator frame should render");
    assert_eq!(checksum_frame(&operator_frame), 0xbedb_82fa_9d2f_65ae);

    c.bench_function("prepare_starter", |b| {
        b.iter(|| {
            black_box(
                PreparedSequenceRenderer::prepare(
                    black_box(&session.project),
                    black_box(setup_id),
                    black_box(sequence_id),
                )
                .expect("benchmark project should prepare"),
            )
        });
    });

    for scenario in SCENARIOS {
        let mut scratch = SequenceRenderScratch::default();
        c.bench_function(&format!("render_frame_{}", scenario.frame), |b| {
            b.iter(|| {
                black_box(
                    renderer
                        .render_frame_with_scratch(black_box(scenario.frame), &mut scratch)
                        .expect("benchmark frame should render"),
                )
            });
        });
    }

    let mut playback_scratch = SequenceRenderScratch::default();
    c.bench_function("render_playback_dense_60_frames", |b| {
        b.iter(|| {
            for frame in 19_050..19_110 {
                black_box(
                    renderer
                        .render_frame_with_scratch(black_box(frame), &mut playback_scratch)
                        .expect("benchmark playback frame should render"),
                );
            }
        });
    });

    c.bench_function("render_playback_dense_cold_60_frames", |b| {
        b.iter_batched(
            SequenceRenderScratch::default,
            |mut scratch| {
                for frame in 19_050..19_110 {
                    black_box(
                        renderer
                            .render_frame_with_scratch(black_box(frame), &mut scratch)
                            .expect("benchmark cold dense playback frame should render"),
                    );
                }
            },
            BatchSize::SmallInput,
        );
    });

    let mut operator_scratch = SequenceRenderScratch::default();
    c.bench_function("render_operator_graph_gain_echo", |b| {
        b.iter(|| {
            black_box(
                operator_renderer
                    .render_frame_with_scratch(black_box(3594), &mut operator_scratch)
                    .expect("benchmark operator frame should render"),
            )
        });
    });

    let mut operator_playback_scratch = SequenceRenderScratch::default();
    c.bench_function("render_operator_playback_60_frames", |b| {
        b.iter(|| {
            for frame in 3_570..3_630 {
                black_box(
                    operator_renderer
                        .render_frame_with_scratch(black_box(frame), &mut operator_playback_scratch)
                        .expect("benchmark operator playback frame should render"),
                );
            }
        });
    });

    c.bench_function("render_operator_playback_cold_60_frames", |b| {
        b.iter_batched(
            SequenceRenderScratch::default,
            |mut scratch| {
                for frame in 3_570..3_630 {
                    black_box(
                        operator_renderer
                            .render_frame_with_scratch(black_box(frame), &mut scratch)
                            .expect("benchmark cold operator playback frame should render"),
                    );
                }
            },
            BatchSize::SmallInput,
        );
    });

    for (scenario, renderer) in RASTER_SCENARIOS.into_iter().zip(raster_renderers.iter()) {
        let sample = renderer.prepare_sampled_raster(scenario.rows);
        c.bench_function(scenario.name, |b| {
            b.iter(|| {
                black_box(render_raster_columns(
                    black_box(renderer),
                    black_box(&sample),
                    black_box(scenario),
                ))
            });
        });
    }
}

fn assert_scenarios(renderer: &PreparedSequenceRenderer) {
    for scenario in SCENARIOS {
        let rendered = renderer
            .render_frame(scenario.frame)
            .expect("benchmark frame should render");
        assert_eq!(checksum_frame(&rendered), scenario.checksum);
        assert_eq!(
            renderer.active_effect_names(scenario.frame).len(),
            scenario.active_effect_count
        );
    }
}

fn prepare_raster_renderers(
    project: &dawn_language::model::DawnProject,
    setup_id: &dawn_language::setup::SetupId,
    sequence_id: &dawn_language::sequence::SequenceId,
) -> Vec<PreparedEffectRasterRenderer> {
    RASTER_SCENARIOS
        .iter()
        .map(|scenario| {
            PreparedEffectRasterRenderer::prepare(
                project,
                setup_id,
                sequence_id,
                &EffectInstId(scenario.effect_id),
            )
            .expect("benchmark raster should prepare")
        })
        .collect()
}

fn assert_raster_scenarios(renderers: &[PreparedEffectRasterRenderer]) {
    for (scenario, renderer) in RASTER_SCENARIOS.into_iter().zip(renderers.iter()) {
        let sample = renderer.prepare_sampled_raster(scenario.rows);
        let raster = render_raster_columns(renderer, &sample, scenario);
        assert_eq!(raster.len(), scenario.columns * scenario.rows);
        assert_eq!(checksum_colors(&raster), scenario.checksum);
    }
}

fn render_raster_columns(
    renderer: &PreparedEffectRasterRenderer,
    sample: &dawn_runtime::PreparedEffectRasterSample,
    scenario: RasterScenario,
) -> Vec<Color> {
    let mut raster = Vec::with_capacity(scenario.columns * scenario.rows);
    let mut scratch = EffectRasterRenderScratch::default();
    for column in 0..scenario.columns {
        let sample_seconds = renderer
            .sampled_raster_column_seconds(column, scenario.columns)
            .expect("benchmark raster column time should resolve");
        let colors = renderer
            .render_sampled_raster_column_with_scratch(sample, sample_seconds, &mut scratch)
            .expect("benchmark raster column should render");
        assert_eq!(colors.len(), scenario.rows);
        raster.extend(colors);
    }
    raster
}

fn project_path() -> Utf8PathBuf {
    Utf8PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join("examples/starter/project.dawn")
}

fn checksum_frame(frame: &RenderedFrame) -> u64 {
    let mut hash = 0xcbf2_9ce4_8422_2325u64;
    hash = checksum_u64(hash, frame.frame_index);
    for element in &frame.elements {
        hash = checksum_u32(hash, element.element_id.0);
        hash = checksum_colors_with_seed(hash, &element.pixels);
    }
    hash
}

fn checksum_colors(colors: &[Color]) -> u64 {
    checksum_colors_with_seed(0xcbf2_9ce4_8422_2325u64, colors)
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

criterion_group!(benches, bench_render);
criterion_main!(benches);
