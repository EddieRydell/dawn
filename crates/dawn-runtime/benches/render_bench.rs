use camino::Utf8PathBuf;
use criterion::{Criterion, criterion_group, criterion_main};
use dawn_language::values::Color;
use dawn_project_io::load_project;
use dawn_runtime::{PreparedSequenceRenderer, RenderedFrame};
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
        checksum: 0x4185_ac9a_0c21_168f,
        active_effect_count: 181,
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
        checksum: 0xc10d_4f6e_9ab4_e5f8,
        active_effect_count: 301,
    },
    RenderScenario {
        frame: 25934,
        checksum: 0xb987_4f83_3a52_d549,
        active_effect_count: 211,
    },
];

#[derive(Clone, Copy)]
struct RenderScenario {
    frame: u64,
    checksum: u64,
    active_effect_count: usize,
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
        for color in &fixture.pixels {
            hash = checksum_color(hash, *color);
        }
    }
    hash
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
