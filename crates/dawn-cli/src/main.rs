use std::hint::black_box;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::{Duration, Instant};

use camino::Utf8PathBuf;
use clap::{Parser, Subcommand};
use dawn_app_core::output_runtime::{
    evaluate_sequence_frame, pixel_context_for_effect, prepare_params_from_document,
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
        #[arg(long)]
        synthetic_active_effects: Option<usize>,
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
            synthetic_active_effects,
        } => bench_effect(
            &project_path_or_directory,
            sequence.as_deref(),
            time,
            iterations,
            warmup,
            json,
            synthetic_active_effects,
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
    synthetic_active_effects: Option<usize>,
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

    let document = if let Some(active_count) = synthetic_active_effects {
        synthetic_active_effect_document(&document, time_seconds, active_count)?
    } else {
        document
    };

    let report = EffectBenchReport::run(
        &input,
        &analysis,
        &document,
        time_seconds,
        iterations,
        warmup,
        synthetic_active_effects,
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
    synthetic_active_effects: Option<usize>,
    total_effects: usize,
    active_effect_count: usize,
    target_pixel_samples_per_frame: usize,
    bytecode: BytecodeAggregateReport,
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
        synthetic_active_effects: Option<usize>,
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
        let target_pixel_samples_per_frame = effect_reports_target_pixel_samples(&effects);
        let bytecode = BytecodeAggregateReport::from_effects(&effects);

        Self {
            project_path: display_path(&canonicalize_path(&input.project_path)),
            project_root: display_path(&canonicalize_path(&input.root)),
            sequence: document.object_key.clone(),
            time_seconds,
            iterations,
            warmup,
            synthetic_active_effects,
            total_effects: document.effects.len(),
            active_effect_count: effects.len(),
            target_pixel_samples_per_frame,
            bytecode,
            whole_frame: TimingStatsReport::from_durations(whole_frame_samples),
            effects,
        }
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct BytecodeAggregateReport {
    instruction_count: usize,
    constant_count: usize,
    param_slots: usize,
    local_slots: usize,
    max_stack_depth: usize,
}

impl BytecodeAggregateReport {
    fn from_effects(effects: &[EffectBenchItemReport]) -> Self {
        Self {
            instruction_count: effects.iter().map(|effect| effect.instruction_count).sum(),
            constant_count: effects.iter().map(|effect| effect.constant_count).sum(),
            param_slots: effects.iter().map(|effect| effect.param_slots).sum(),
            local_slots: effects.iter().map(|effect| effect.local_slots).sum(),
            max_stack_depth: effects
                .iter()
                .map(|effect| effect.max_stack_depth)
                .max()
                .unwrap_or(0),
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
    instruction_count: usize,
    constant_count: usize,
    param_slots: usize,
    local_slots: usize,
    max_stack_depth: usize,
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
        let script = analysis.compiled_script_for_key(&render.script_key)?;
        let prepared_params = prepare_params_from_document(
            script,
            &render.params,
            &document.mark_collections,
            effect.start_seconds,
        )
        .ok()?;
        let stats = script.bytecode_stats();
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
                let _ = black_box(script.sample_prepared(
                    progress,
                    local_seconds,
                    FixtureContext {
                        index: pixel.fixture_index,
                    },
                    pixel_context,
                    &prepared_params,
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
            params: render.params.len(),
            instruction_count: stats.instruction_count,
            constant_count: stats.constant_count,
            param_slots: stats.param_slots,
            local_slots: stats.local_slots,
            max_stack_depth: stats.max_stack_depth,
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

fn synthetic_active_effect_document(
    document: &SequenceDocument,
    time_seconds: f64,
    active_effects: usize,
) -> Result<SequenceDocument, String> {
    let sources = document
        .effects
        .iter()
        .filter(|effect| effect_is_active(effect, time_seconds))
        .cloned()
        .collect::<Vec<_>>();
    if active_effects > 0 && sources.is_empty() {
        return Err(
            "cannot synthesize active effects because the benchmark time has no active effects"
                .to_string(),
        );
    }

    let mut synthetic = document.clone();
    synthetic
        .effects
        .retain(|effect| !effect_is_active(effect, time_seconds));
    synthetic.effects.reserve(active_effects);
    for index in 0..active_effects {
        let mut effect = sources[index % sources.len()].clone();
        effect.index = synthetic.effects.len();
        synthetic.effects.push(effect);
    }
    Ok(synthetic)
}

fn effect_reports_target_pixel_samples(effects: &[EffectBenchItemReport]) -> usize {
    effects
        .iter()
        .map(|effect| effect.target_pixels)
        .sum::<usize>()
}

#[cfg(test)]
fn active_target_pixel_samples(document: &SequenceDocument, time_seconds: f64) -> usize {
    document
        .effects
        .iter()
        .filter(|effect| effect_is_active(effect, time_seconds))
        .filter_map(|effect| effect.render.as_ref())
        .map(|render| render.target_pixels.len())
        .sum()
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
        "project={} sequence={} time={:.3}s iterations={} warmup={} synthetic_active_effects={}",
        report.project_path,
        report.sequence,
        report.time_seconds,
        report.iterations,
        report.warmup,
        report
            .synthetic_active_effects
            .map(|count| count.to_string())
            .unwrap_or_else(|| "none".to_string())
    );
    println!(
        "total effects={} active effects={} target pixel samples/frame={} bytecode=instructions:{} constants:{} param_slots:{} local_slots:{} max_stack:{}",
        report.total_effects,
        report.active_effect_count,
        report.target_pixel_samples_per_frame,
        report.bytecode.instruction_count,
        report.bytecode.constant_count,
        report.bytecode.param_slots,
        report.bytecode.local_slots,
        report.bytecode.max_stack_depth
    );
    print_timing_stats("whole frame", &report.whole_frame);
    if report.effects.is_empty() {
        println!("active effects=0");
        return;
    }

    println!("active effects={}", report.active_effect_count);
    let displayed_effects = if report.synthetic_active_effects.is_some() {
        report.effects.iter().take(5).collect::<Vec<_>>()
    } else {
        report.effects.iter().collect::<Vec<_>>()
    };
    for effect in displayed_effects {
        println!(
            "effect id={} index={} script={} target={} pixels={} params={} scope={} bytecode=instructions:{} constants:{} param_slots:{} local_slots:{} max_stack:{}",
            effect.effect_id,
            effect.effect_index,
            effect.script,
            effect.target_label,
            effect.target_pixels,
            effect.params,
            effect.scope,
            effect.instruction_count,
            effect.constant_count,
            effect.param_slots,
            effect.local_slots,
            effect.max_stack_depth
        );
        print_timing_stats("  effect", &effect.effect);
        print_timing_stats("  per sample", &effect.per_sample);
    }
    if report.synthetic_active_effects.is_some() && report.effects.len() > 5 {
        println!(
            "omitted {} synthetic per-effect reports",
            report.effects.len() - 5
        );
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

#[cfg(test)]
mod tests {
    use super::*;
    use dawn_project::document::{
        LayoutTargetDocument, SequenceEffectPixelDocument, SequenceEffectRenderDocument,
    };
    use dawn_project::model::LayoutTargetKind;
    use dawn_project::model::SequenceEffectScope;

    fn active_effect(target_pixels: usize) -> SequenceEffectDocument {
        SequenceEffectDocument {
            index: 0,
            id: 1,
            start_seconds: 40.0,
            duration_seconds: 10.0,
            target: LayoutTargetDocument {
                kind: LayoutTargetKind::Group,
                name: "all".to_string(),
            },
            target_label: "Group all".to_string(),
            scope: SequenceEffectScope::WholeTarget,
            script: "pulse".to_string(),
            script_source: Some("effects/pulse.effect.dawn".to_string()),
            params: Vec::new(),
            render: Some(SequenceEffectRenderDocument {
                script_key: "effects/pulse.effect.dawn".to_string(),
                script_source: "effects/pulse.effect.dawn".to_string(),
                params: Vec::new(),
                target_pixels: (0..target_pixels)
                    .map(|pixel_index| SequenceEffectPixelDocument {
                        fixture_index: 0,
                        pixel_index,
                        pixel_count: target_pixels,
                    })
                    .collect(),
            }),
        }
    }

    fn sequence_document(effect: SequenceEffectDocument) -> SequenceDocument {
        SequenceDocument {
            path: "sequences/opening.sequence.dawn".to_string(),
            object_key: "opening".to_string(),
            duration_seconds: 60.0,
            frame_rate: 30,
            audio: None,
            mark_collections: Vec::new(),
            lanes: Vec::new(),
            effect_scripts: Vec::new(),
            effects: vec![effect],
            degraded: false,
        }
    }

    #[test]
    fn synthetic_active_effects_expand_one_source_to_requested_count() {
        let document = sequence_document(active_effect(3));
        let synthetic = synthetic_active_effect_document(&document, 42.0, 1_000).unwrap();

        assert_eq!(
            synthetic
                .effects
                .iter()
                .filter(|effect| effect_is_active(effect, 42.0))
                .count(),
            1_000
        );
        assert_eq!(active_target_pixel_samples(&synthetic, 42.0), 3_000);
    }

    #[test]
    fn synthetic_active_effects_render_small_cloned_load() {
        let project_path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("..")
            .join("examples")
            .join("club-rig");
        let input = project_input(&project_path).unwrap();
        let fs = WorkspaceFs::open(&input.root).unwrap();
        let analysis =
            analyze_project_with_overlays(&fs, input.project_file.clone(), None, Vec::new());
        assert!(!analysis.has_errors());
        let sequence_target = sequence_target(&analysis, None).unwrap();
        let document = get_sequence_document(
            &fs,
            sequence_target.path,
            &sequence_target.object_key,
            input.project_file,
            Vec::new(),
        )
        .unwrap();
        let synthetic = synthetic_active_effect_document(&document, 42.0, 8).unwrap();

        let frame = evaluate_sequence_frame(&analysis, &synthetic, 42.0, 0);

        assert_eq!(
            synthetic
                .effects
                .iter()
                .filter(|effect| effect_is_active(effect, 42.0))
                .count(),
            8
        );
        assert!(matches!(
            frame.status,
            dawn_app_core::output_runtime::OutputFrameStatus::Live
        ));
    }
}
