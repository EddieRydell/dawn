use std::hint::black_box;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::{Duration, Instant};

use camino::Utf8PathBuf;
use clap::{Parser, Subcommand};
use dawn_app_core::output_runtime::{
    evaluate_sequence_frame, pixel_context_for_effect, runtime_params_from_document,
};
use dawn_project::analysis::{
    analyze_project_with_overlays, DiagnosticCode, DiagnosticSeverity, ProjectAnalysis,
    ProjectDiagnostic, TextRange,
};
use dawn_project::document::{get_sequence_document, SequenceDocument, SequenceEffectDocument};
use dawn_project::effect_script::FixtureContext;
use dawn_project::fs::WorkspaceFs;
use dawn_project::model::DawnObject;
use dawn_project::path::{canonicalize_path, utf8_path, PathStringExt};
use serde::Serialize;

#[derive(Debug, Parser)]
#[command(name = "dawn")]
#[command(about = "Dawn project tooling")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    Analyze {
        project_path_or_directory: PathBuf,
        #[arg(long)]
        json: bool,
    },
    BenchEffect {
        project_path_or_directory: PathBuf,
        #[arg(long)]
        sequence: Option<String>,
        #[arg(long)]
        time: f64,
        #[arg(long, default_value_t = 300)]
        iterations: usize,
        #[arg(long, default_value_t = 30)]
        warmup: usize,
        #[arg(long)]
        json: bool,
    },
}

fn main() -> ExitCode {
    match run() {
        Ok(exit_code) => exit_code,
        Err(message) => {
            eprintln!("{message}");
            ExitCode::from(2)
        }
    }
}

fn run() -> Result<ExitCode, String> {
    match Cli::parse().command {
        Command::Analyze {
            project_path_or_directory,
            json,
        } => analyze(&project_path_or_directory, json),
        Command::BenchEffect {
            project_path_or_directory,
            sequence,
            time,
            iterations,
            warmup,
            json,
        } => bench_effect(
            &project_path_or_directory,
            sequence.as_deref(),
            time,
            iterations,
            warmup,
            json,
        ),
    }
}

fn analyze(path: &Path, json: bool) -> Result<ExitCode, String> {
    let input = project_input(path)?;
    let fs = WorkspaceFs::open(&input.root).map_err(|error| error.to_string())?;
    let analysis = analyze_project_with_overlays(&fs, input.project_file.clone(), None, Vec::new());

    if json {
        let report = AnalysisReport::from_analysis(&analysis, &input);
        serde_json::to_writer_pretty(std::io::stdout(), &report)
            .map_err(|error| error.to_string())?;
        println!();
    } else {
        print_human_report(&analysis);
    }

    if analysis.has_errors() {
        Ok(ExitCode::from(1))
    } else {
        Ok(ExitCode::SUCCESS)
    }
}

fn bench_effect(
    path: &Path,
    sequence: Option<&str>,
    time_seconds: f64,
    iterations: usize,
    warmup: usize,
    json: bool,
) -> Result<ExitCode, String> {
    if !time_seconds.is_finite() {
        return Err("time must be finite".to_string());
    }
    if iterations == 0 {
        return Err("iterations must be greater than zero".to_string());
    }

    let input = project_input(path)?;
    let fs = WorkspaceFs::open(&input.root).map_err(|error| error.to_string())?;
    let analysis = analyze_project_with_overlays(&fs, input.project_file.clone(), None, Vec::new());
    if analysis.has_errors() {
        print_human_report(&analysis);
        return Ok(ExitCode::from(1));
    }

    let sequence_target = sequence_target(&analysis, sequence)?;
    let document = get_sequence_document(
        &fs,
        sequence_target.path,
        &sequence_target.object_key,
        input.project_file.clone(),
        Vec::new(),
    )?;

    let report = EffectBenchReport::run(
        &input,
        &analysis,
        &document,
        time_seconds,
        iterations,
        warmup,
    );

    if json {
        serde_json::to_writer_pretty(std::io::stdout(), &report)
            .map_err(|error| error.to_string())?;
        println!();
    } else {
        print_effect_bench_report(&report);
    }

    Ok(ExitCode::SUCCESS)
}

fn sequence_target(
    analysis: &ProjectAnalysis,
    sequence: Option<&str>,
) -> Result<SequenceTarget, String> {
    let mut sequences = Vec::new();
    for analyzed_file in analysis.files.values() {
        let Some(file) = &analyzed_file.file else {
            continue;
        };
        for (object_key, object) in file {
            if matches!(object, DawnObject::Sequence(_)) {
                sequences.push(SequenceTarget {
                    path: analyzed_file.path.clone(),
                    object_key: object_key.clone(),
                });
            }
        }
    }

    if let Some(sequence) = sequence {
        let matches = sequences
            .into_iter()
            .filter(|target| target.object_key == sequence)
            .collect::<Vec<_>>();
        return match matches.len() {
            0 => Err(format!("sequence object `{sequence}` was not found")),
            1 => Ok(matches.into_iter().next().expect("single sequence match")),
            _ => Err(format!(
                "sequence object `{sequence}` exists in multiple files; use a unique sequence key"
            )),
        };
    }

    match sequences.len() {
        0 => Err("project does not contain a sequence".to_string()),
        1 => Ok(sequences.remove(0)),
        _ => {
            let keys = sequences
                .iter()
                .map(|target| format!("{}:{}", display_path(&target.path), target.object_key))
                .collect::<Vec<_>>()
                .join(", ");
            Err(format!(
                "project contains multiple sequences; pass --sequence. sequences: {keys}"
            ))
        }
    }
}

#[derive(Debug)]
struct SequenceTarget {
    path: Utf8PathBuf,
    object_key: String,
}

fn project_input(path: &Path) -> Result<ProjectInput, String> {
    if path.is_dir() {
        let project_path = path.join("project.dawn");
        if !project_path.is_file() {
            return Err(format!(
                "project file was not found: {}",
                project_path.display()
            ));
        }
        return Ok(ProjectInput {
            root: utf8_path(path)?,
            project_file: Utf8PathBuf::from("project.dawn"),
            project_path: utf8_path(project_path)?,
        });
    }

    if path.is_file() {
        let file_name = path
            .file_name()
            .ok_or_else(|| "project file has no file name".to_string())?;
        let root = path
            .parent()
            .ok_or_else(|| "project file has no parent".to_string())?;
        return Ok(ProjectInput {
            root: utf8_path(root)?,
            project_file: utf8_path(PathBuf::from(file_name))?,
            project_path: utf8_path(path)?,
        });
    }

    Err(format!("path was not found: {}", path.display()))
}

fn print_human_report(analysis: &ProjectAnalysis) {
    if analysis.diagnostics.is_empty() {
        println!(
            "OK project={} resolved={} reachable_files={} objects={}",
            analysis.project_key,
            analysis.is_resolved(),
            analysis.reachable_file_count(),
            analysis.object_count()
        );
        return;
    }

    for diagnostic in &analysis.diagnostics {
        println!("{}", human_diagnostic(diagnostic));
    }
}

fn human_diagnostic(diagnostic: &ProjectDiagnostic) -> String {
    let severity = match diagnostic.severity {
        DiagnosticSeverity::Error => "ERROR",
        DiagnosticSeverity::Warning => "WARNING",
    };
    let code = diagnostic_code(diagnostic.code);
    let path = display_path(&diagnostic.path);

    if let Some(range) = diagnostic.range {
        format!(
            "{severity} {path}:{}:{} [{code}] {}",
            range.start.line + 1,
            range.start.character + 1,
            diagnostic.message
        )
    } else {
        format!("{severity} {path} [{code}] {}", diagnostic.message)
    }
}

fn diagnostic_code(code: DiagnosticCode) -> &'static str {
    match code {
        DiagnosticCode::Io => "io",
        DiagnosticCode::Yaml => "yaml",
        DiagnosticCode::Import => "import",
        DiagnosticCode::Lower => "lower",
        DiagnosticCode::ProjectKey => "project_key",
        DiagnosticCode::Sequence => "sequence",
        DiagnosticCode::Script => "script",
    }
}

fn display_path(path: &Utf8PathBuf) -> String {
    clean_display_path(path.to_slash_string())
}

fn clean_display_path(path: String) -> String {
    if let Some(path) = path.strip_prefix("//?/UNC/") {
        format!("//{path}")
    } else if let Some(path) = path.strip_prefix("//?/") {
        path.to_string()
    } else {
        path
    }
}

#[derive(Debug)]
struct ProjectInput {
    root: Utf8PathBuf,
    project_file: Utf8PathBuf,
    project_path: Utf8PathBuf,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct EffectBenchReport {
    project_path: String,
    project_root: String,
    sequence: String,
    time_seconds: f64,
    iterations: usize,
    warmup: usize,
    active_effect_count: usize,
    whole_frame: TimingStatsReport,
    effects: Vec<EffectBenchItemReport>,
}

impl EffectBenchReport {
    fn run(
        input: &ProjectInput,
        analysis: &ProjectAnalysis,
        document: &SequenceDocument,
        time_seconds: f64,
        iterations: usize,
        warmup: usize,
    ) -> Self {
        for generation in 0..warmup {
            black_box(evaluate_sequence_frame(
                analysis,
                document,
                time_seconds,
                generation as u64,
            ));
        }

        let mut whole_frame_samples = Vec::with_capacity(iterations);
        for generation in 0..iterations {
            let start = Instant::now();
            black_box(evaluate_sequence_frame(
                analysis,
                document,
                time_seconds,
                generation as u64,
            ));
            whole_frame_samples.push(start.elapsed());
        }

        let effects = document
            .effects
            .iter()
            .filter(|effect| effect_is_active(effect, time_seconds))
            .filter_map(|effect| {
                EffectBenchItemReport::run(analysis, document, effect, time_seconds, iterations)
            })
            .collect::<Vec<_>>();

        Self {
            project_path: display_path(&canonicalize_path(&input.project_path)),
            project_root: display_path(&canonicalize_path(&input.root)),
            sequence: document.object_key.clone(),
            time_seconds,
            iterations,
            warmup,
            active_effect_count: effects.len(),
            whole_frame: TimingStatsReport::from_durations(whole_frame_samples),
            effects,
        }
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct EffectBenchItemReport {
    effect_id: u32,
    effect_index: usize,
    script: String,
    script_key: String,
    scope: String,
    target_label: String,
    target_pixels: usize,
    params: usize,
    effect: TimingStatsReport,
    per_sample: TimingStatsReport,
}

impl EffectBenchItemReport {
    fn run(
        analysis: &ProjectAnalysis,
        document: &SequenceDocument,
        effect: &SequenceEffectDocument,
        time_seconds: f64,
        iterations: usize,
    ) -> Option<Self> {
        let render = effect.render.as_ref()?;
        let local_seconds = time_seconds - effect.start_seconds;
        let progress = if effect.duration_seconds == 0.0 {
            0.0
        } else {
            (local_seconds / effect.duration_seconds).clamp(0.0, 1.0)
        };
        let params = runtime_params_from_document(
            &render.params,
            &document.mark_collections,
            effect.start_seconds,
        );
        let target_pixel_count = render.target_pixels.len();
        let mut effect_samples = Vec::with_capacity(iterations);
        let mut per_sample_samples = Vec::with_capacity(iterations);

        for _ in 0..iterations {
            let start = Instant::now();
            for (target_pixel_index, pixel) in render.target_pixels.iter().enumerate() {
                let pixel_context = pixel_context_for_effect(
                    effect.scope,
                    target_pixel_index,
                    target_pixel_count,
                    pixel.pixel_index,
                    pixel.pixel_count,
                );
                let _ = black_box(analysis.sample_effect_script_key(
                    &render.script_key,
                    progress,
                    local_seconds,
                    FixtureContext {
                        index: pixel.fixture_index,
                    },
                    pixel_context,
                    &params,
                ));
            }
            let elapsed = start.elapsed();
            effect_samples.push(elapsed);
            per_sample_samples.push(divide_duration(elapsed, target_pixel_count));
        }

        Some(Self {
            effect_id: effect.id,
            effect_index: effect.index,
            script: effect.script.clone(),
            script_key: render.script_key.clone(),
            scope: format!("{:?}", effect.scope),
            target_label: effect.target_label.clone(),
            target_pixels: target_pixel_count,
            params: params.len(),
            effect: TimingStatsReport::from_durations(effect_samples),
            per_sample: TimingStatsReport::from_durations(per_sample_samples),
        })
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct TimingStatsReport {
    min_ms: f64,
    p50_ms: f64,
    p95_ms: f64,
    avg_ms: f64,
    max_ms: f64,
    hz_at_p50: f64,
}

impl TimingStatsReport {
    fn from_durations(mut samples: Vec<Duration>) -> Self {
        samples.sort_unstable();
        let total_nanos = samples
            .iter()
            .map(|duration| duration.as_nanos())
            .sum::<u128>();
        let count = samples.len() as u128;
        let avg = duration_from_nanos(total_nanos / count);
        let p50 = percentile_duration(&samples, 0.50);
        Self {
            min_ms: duration_ms(samples[0]),
            p50_ms: duration_ms(p50),
            p95_ms: duration_ms(percentile_duration(&samples, 0.95)),
            avg_ms: duration_ms(avg),
            max_ms: duration_ms(samples[samples.len() - 1]),
            hz_at_p50: hz_from_duration(p50),
        }
    }
}

fn effect_is_active(effect: &SequenceEffectDocument, time_seconds: f64) -> bool {
    effect.render.is_some()
        && time_seconds >= effect.start_seconds
        && time_seconds < effect.start_seconds + effect.duration_seconds
}

fn divide_duration(duration: Duration, divisor: usize) -> Duration {
    if divisor == 0 {
        return Duration::ZERO;
    }
    duration_from_nanos(duration.as_nanos() / divisor as u128)
}

fn percentile_duration(samples: &[Duration], percentile: f64) -> Duration {
    let index = ((samples.len() - 1) as f64 * percentile).round() as usize;
    samples[index]
}

fn duration_from_nanos(nanos: u128) -> Duration {
    let nanos = u64::try_from(nanos).unwrap_or(u64::MAX);
    Duration::from_nanos(nanos)
}

fn duration_ms(duration: Duration) -> f64 {
    duration.as_secs_f64() * 1000.0
}

fn hz_from_duration(duration: Duration) -> f64 {
    let seconds = duration.as_secs_f64();
    if seconds == 0.0 {
        0.0
    } else {
        1.0 / seconds
    }
}

fn print_effect_bench_report(report: &EffectBenchReport) {
    println!(
        "project={} sequence={} time={:.3}s iterations={} warmup={}",
        report.project_path, report.sequence, report.time_seconds, report.iterations, report.warmup
    );
    print_timing_stats("whole frame", &report.whole_frame);
    if report.effects.is_empty() {
        println!("active effects=0");
        return;
    }

    println!("active effects={}", report.active_effect_count);
    for effect in &report.effects {
        println!(
            "effect id={} index={} script={} target={} pixels={} params={} scope={}",
            effect.effect_id,
            effect.effect_index,
            effect.script,
            effect.target_label,
            effect.target_pixels,
            effect.params,
            effect.scope
        );
        print_timing_stats("  effect", &effect.effect);
        print_timing_stats("  per sample", &effect.per_sample);
    }
}

fn print_timing_stats(label: &str, stats: &TimingStatsReport) {
    println!(
        "{label}: p50={:.3}ms p95={:.3}ms avg={:.3}ms min={:.3}ms max={:.3}ms p50_hz={:.1}",
        stats.p50_ms, stats.p95_ms, stats.avg_ms, stats.min_ms, stats.max_ms, stats.hz_at_p50
    );
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct AnalysisReport {
    project_path: String,
    project_root: String,
    project_key: String,
    resolved: bool,
    error_count: usize,
    warning_count: usize,
    reachable_file_count: usize,
    object_count: usize,
    diagnostics: Vec<DiagnosticReport>,
}

impl AnalysisReport {
    fn from_analysis(analysis: &ProjectAnalysis, input: &ProjectInput) -> Self {
        Self {
            project_path: display_path(&canonicalize_path(&input.project_path)),
            project_root: display_path(&canonicalize_path(&input.root)),
            project_key: analysis.project_key.clone(),
            resolved: analysis.is_resolved(),
            error_count: analysis
                .diagnostics
                .iter()
                .filter(|diagnostic| diagnostic.severity == DiagnosticSeverity::Error)
                .count(),
            warning_count: analysis
                .diagnostics
                .iter()
                .filter(|diagnostic| diagnostic.severity == DiagnosticSeverity::Warning)
                .count(),
            reachable_file_count: analysis.reachable_file_count(),
            object_count: analysis.object_count(),
            diagnostics: analysis
                .diagnostics
                .iter()
                .map(DiagnosticReport::from_diagnostic)
                .collect(),
        }
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct DiagnosticReport {
    path: String,
    range: Option<TextRangeReport>,
    severity: DiagnosticSeverityReport,
    code: &'static str,
    message: String,
}

impl DiagnosticReport {
    fn from_diagnostic(diagnostic: &ProjectDiagnostic) -> Self {
        Self {
            path: display_path(&diagnostic.path),
            range: diagnostic.range.map(TextRangeReport::from_range),
            severity: DiagnosticSeverityReport::from_severity(diagnostic.severity),
            code: diagnostic_code(diagnostic.code),
            message: diagnostic.message.clone(),
        }
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "lowercase")]
enum DiagnosticSeverityReport {
    Error,
    Warning,
}

impl DiagnosticSeverityReport {
    fn from_severity(severity: DiagnosticSeverity) -> Self {
        match severity {
            DiagnosticSeverity::Error => Self::Error,
            DiagnosticSeverity::Warning => Self::Warning,
        }
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct TextRangeReport {
    start: TextPositionReport,
    end: TextPositionReport,
}

impl TextRangeReport {
    fn from_range(range: TextRange) -> Self {
        Self {
            start: TextPositionReport {
                line: range.start.line,
                character: range.start.character,
            },
            end: TextPositionReport {
                line: range.end.line,
                character: range.end.character,
            },
        }
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct TextPositionReport {
    line: u32,
    character: u32,
}
