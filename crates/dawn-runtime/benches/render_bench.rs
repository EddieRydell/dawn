use camino::Utf8PathBuf;
use criterion::{Criterion, criterion_group, criterion_main};
use dawn_language::effect::EffectInstId;
use dawn_language::values::Color;
use dawn_project_io::load_project;
use dawn_runtime::{PreparedEffectRasterRenderer, PreparedSequenceRenderer, RenderedFrame};
use std::hint::black_box;

const SCENARIOS: [RenderScenario; 7] = [
    RenderScenario {
        frame: 144,
        checksum: 0x3f76_8cdc_06a4_bd8e,
        active_effect_count: 1,
    },
    RenderScenario {
        frame: 2088,
        checksum: 0x2faa_cffb_1da1_fde8,
        active_effect_count: 30,
    },
    RenderScenario {
        frame: 5904,
        checksum: 0x2cd4_8e9a_6687_58d7,
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
        checksum: 0xc8d2_ca17_c905_e8f8,
        active_effect_count: 301,
    },
    RenderScenario {
        frame: 25934,
        checksum: 0xb987_4f83_3a52_d549,
        active_effect_count: 211,
    },
];

const RASTER_SCENARIOS: [RasterScenario; 2] = [
    RasterScenario {
        name: "raster_sample_shimmer_field_75",
        effect_id: 75,
        columns: 256,
        rows: 50,
        checksum: 0x5185_944c_8772_1aee,
    },
    RasterScenario {
        name: "raster_generator_mark_pulse_76",
        effect_id: 76,
        columns: 256,
        rows: 50,
        checksum: 0x9e95_b38b_5f33_f35d,
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

    assert_scenarios(&renderer);
    let raster_renderers = prepare_raster_renderers(&session.project, setup_id, sequence_id);
    assert_raster_scenarios(&raster_renderers);

    c.bench_function("prepare_thirty_output_controller", |b| {
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
        c.bench_function(&format!("render_frame_{}", scenario.frame), |b| {
            b.iter(|| {
                black_box(
                    renderer
                        .render_frame(black_box(scenario.frame))
                        .expect("benchmark frame should render"),
                )
            });
        });
    }

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
    let duration_frames = renderer.duration_seconds() * f64::from(renderer.frame_rate());
    let sample_step_frames = (duration_frames / scenario.columns as f64).max(4.0);
    let mut raster = Vec::with_capacity(scenario.columns * scenario.rows);
    for column in 0..scenario.columns {
        let sample_seconds = renderer.start_seconds()
            + (column as f64 * sample_step_frames) / f64::from(renderer.frame_rate());
        let colors = renderer
            .render_sampled_raster_column(sample, sample_seconds)
            .expect("benchmark raster column should render");
        assert_eq!(colors.len(), scenario.rows);
        raster.extend(colors);
    }
    raster
}

fn project_path() -> Utf8PathBuf {
    Utf8PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join("examples/thirty-output-controller/project.dawn")
}

fn checksum_frame(frame: &RenderedFrame) -> u64 {
    let mut hash = 0xcbf2_9ce4_8422_2325u64;
    hash = checksum_u64(hash, frame.frame_index);
    for fixture in &frame.fixtures {
        hash = checksum_u32(hash, fixture.fixture_id.0);
        hash = checksum_colors_with_seed(hash, &fixture.pixels);
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
