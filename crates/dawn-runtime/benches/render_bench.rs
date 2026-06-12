use camino::Utf8PathBuf;
use dawn_language::values::Color;
use dawn_project_io::load_project;
use dawn_runtime::{PreparedSequenceRenderer, RenderedFrame};
use std::env;
use std::error::Error;
use std::time::{Duration, Instant};

#[derive(Clone, Debug)]
struct Config {
    project: Utf8PathBuf,
    frames: Vec<u64>,
    iterations: usize,
    warmup: usize,
}

fn main() {
    if env::args().len() == 1 {
        return;
    }
    if let Err(error) = run() {
        eprintln!("render_bench: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn Error>> {
    let config = parse_args()?;
    let session = load_project(&config.project)?;
    let setup_id = &session.project.root.setup;
    let sequence_id = session
        .project
        .root
        .sequences
        .first()
        .ok_or("project root has no sequences")?;

    let warm_renderer = PreparedSequenceRenderer::prepare(&session.project, setup_id, sequence_id)
        .map_err(|error| format!("prepare failed: {error:?}"))?;
    for frame in &config.frames {
        let _ = warm_renderer
            .render_frame(*frame)
            .map_err(|error| format!("render frame {frame} failed: {error:?}"))?;
    }

    for _ in 0..config.warmup {
        let renderer = PreparedSequenceRenderer::prepare(&session.project, setup_id, sequence_id)
            .map_err(|error| format!("prepare failed: {error:?}"))?;
        for frame in &config.frames {
            let _ = renderer
                .render_frame(*frame)
                .map_err(|error| format!("render frame {frame} failed: {error:?}"))?;
        }
    }

    let mut prepare_times = Vec::with_capacity(config.iterations);
    let mut frame_times = config
        .frames
        .iter()
        .map(|frame| (*frame, Vec::with_capacity(config.iterations)))
        .collect::<Vec<_>>();
    let mut checksums = Vec::new();
    let mut active_effects = Vec::new();

    for iteration in 0..config.iterations {
        let prepare_start = Instant::now();
        let renderer = PreparedSequenceRenderer::prepare(&session.project, setup_id, sequence_id)
            .map_err(|error| format!("prepare failed: {error:?}"))?;
        prepare_times.push(prepare_start.elapsed());

        if iteration == 0 {
            for frame in &config.frames {
                active_effects.push((
                    *frame,
                    renderer
                        .active_effect_names(*frame)
                        .into_iter()
                        .map(str::to_string)
                        .collect::<Vec<_>>(),
                ));
            }
        }

        for (frame, times) in &mut frame_times {
            let frame_start = Instant::now();
            let rendered = renderer
                .render_frame(*frame)
                .map_err(|error| format!("render frame {frame} failed: {error:?}"))?;
            times.push(frame_start.elapsed());
            if iteration == 0 {
                checksums.push((*frame, checksum_frame(&rendered)));
            }
        }
    }

    println!("project: {}", config.project);
    println!("iterations: {}", config.iterations);
    println!("warmup: {}", config.warmup);
    println!(
        "prepare: p50={} p95={}",
        format_duration(percentile(&mut prepare_times.clone(), 50)),
        format_duration(percentile(&mut prepare_times, 95))
    );
    for (frame, mut times) in frame_times {
        println!(
            "frame {frame}: p50={} p95={}",
            format_duration(percentile(&mut times.clone(), 50)),
            format_duration(percentile(&mut times, 95))
        );
    }
    println!("checksums:");
    for (frame, checksum) in checksums {
        println!("  frame {frame}: {checksum:016x}");
    }
    println!("active effects:");
    for (frame, names) in active_effects {
        println!("  frame {frame}: {}", names.join(", "));
    }

    Ok(())
}

fn parse_args() -> Result<Config, Box<dyn Error>> {
    let mut project = None;
    let mut frames = None;
    let mut iterations = None;
    let mut warmup = None;
    let mut args = env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--bench" => {}
            "--project" => {
                project = Some(resolve_project_path(&next_value(&mut args, "--project")?));
            }
            "--frames" => {
                let value = next_value(&mut args, "--frames")?;
                frames = Some(parse_frames(&value)?);
            }
            "--iterations" => {
                let value = next_value(&mut args, "--iterations")?;
                iterations = Some(value.parse()?);
            }
            "--warmup" => {
                let value = next_value(&mut args, "--warmup")?;
                warmup = Some(value.parse()?);
            }
            _ => return Err(format!("unknown argument `{arg}`").into()),
        }
    }

    Ok(Config {
        project: project.ok_or("missing --project")?,
        frames: frames.ok_or("missing --frames")?,
        iterations: iterations.ok_or("missing --iterations")?,
        warmup: warmup.ok_or("missing --warmup")?,
    })
}

fn next_value(
    args: &mut impl Iterator<Item = String>,
    flag: &'static str,
) -> Result<String, Box<dyn Error>> {
    args.next()
        .ok_or_else(|| format!("missing value for {flag}").into())
}

fn parse_frames(value: &str) -> Result<Vec<u64>, Box<dyn Error>> {
    value
        .split(',')
        .map(|frame| frame.parse::<u64>().map_err(|error| error.into()))
        .collect()
}

fn resolve_project_path(value: &str) -> Utf8PathBuf {
    let path = Utf8PathBuf::from(value);
    if path.is_absolute() {
        return path;
    }
    Utf8PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(path)
}

fn percentile(times: &mut [Duration], percentile: usize) -> Duration {
    times.sort_unstable();
    let index = ((times.len().saturating_sub(1)) * percentile).div_ceil(100);
    times[index]
}

fn format_duration(duration: Duration) -> String {
    format!("{:.3}ms", duration.as_secs_f64() * 1000.0)
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
