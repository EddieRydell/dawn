use std::hint::black_box;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::{Duration, Instant};

use camino::Utf8PathBuf;
use clap::{Parser, Subcommand, ValueEnum};
use dawn_app_core::fseq_export::{export_fseq_file, FseqExportOptions};
use dawn_app_core::output_runtime::{
    pixel_context_for_effect, prepare_params_from_document, OutputFrameStatus,
    SequenceFrameEvaluationTiming, SequenceFrameEvaluator, SequenceFrameEvaluatorPreparationTiming,
};
use dawn_project::analysis::{
    analyze_project_with_overlays, DiagnosticCode, DiagnosticSeverity, ProjectAnalysis,
    ProjectDiagnostic, ProjectOverlay, TextRange,
};
use dawn_project::document::{
    get_sequence_document, LayoutTargetDocument, SequenceDocument, SequenceEffectDocument,
    SequenceEffectParamDocument, SequenceEffectPixelDocument, SequenceEffectRenderDocument,
    SequenceLaneDocument, SequenceMarkCollectionDocument,
};
use dawn_project::effect_script::{EffectSampleScratch, FixtureContext};
use dawn_project::fs::WorkspaceFs;
use dawn_project::model::{
    Color, Curve, CurvePoint, CurveValue, CurveValueType, DawnObject, EffectParam, EffectScriptId,
    LayoutTargetKind, SequenceEffectScope,
};
use dawn_project::path::{canonicalize_path, utf8_path, PathStringExt};
use dawn_project::render::layout_render_plan;
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
        time: Option<f64>,
        #[arg(long, value_enum, default_value_t = BenchSuite::Project)]
        suite: BenchSuite,
        #[arg(long, value_enum, default_value_t = BenchCaseKindFilter::All)]
        case_kind: BenchCaseKindFilter,
        #[arg(long, value_enum, default_value_t = BenchMatrix::Standard)]
        matrix: BenchMatrix,
        #[arg(long, default_value_t = 300)]
        iterations: usize,
        #[arg(long, default_value_t = 30)]
        warmup: usize,
        #[arg(long)]
        json: bool,
        #[arg(long)]
        synthetic_active_effects: Option<usize>,
        #[arg(long)]
        isolate_effect_id: Option<u32>,
        #[arg(long)]
        no_effect_breakdown: bool,
    },
    Baseline {
        project_path_or_directory: PathBuf,
        #[arg(long)]
        sequence: Option<String>,
        #[arg(long, default_value_t = 30)]
        iterations: usize,
        #[arg(long, default_value_t = 5)]
        warmup: usize,
        #[arg(long)]
        json: bool,
    },
    ExportFseq {
        project_path_or_directory: PathBuf,
        #[arg(long)]
        output: PathBuf,
        #[arg(long)]
        sequence: Option<String>,
        #[arg(long, default_value_t = 50)]
        step_ms: u8,
        #[arg(long)]
        force: bool,
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
            suite,
            case_kind,
            matrix,
            iterations,
            warmup,
            json,
            synthetic_active_effects,
            isolate_effect_id,
            no_effect_breakdown,
        } => bench_effect(BenchEffectOptions {
            path: &project_path_or_directory,
            sequence: sequence.as_deref(),
            time_seconds: time,
            suite,
            case_kind,
            matrix,
            iterations,
            warmup,
            json,
            synthetic_active_effects,
            isolate_effect_id,
            no_effect_breakdown,
        }),
        Command::Baseline {
            project_path_or_directory,
            sequence,
            iterations,
            warmup,
            json,
        } => baseline(BaselineOptions {
            path: &project_path_or_directory,
            sequence: sequence.as_deref(),
            iterations,
            warmup,
            json,
        }),
        Command::ExportFseq {
            project_path_or_directory,
            output,
            sequence,
            step_ms,
            force,
        } => export_fseq_command(
            &project_path_or_directory,
            &output,
            sequence.as_deref(),
            step_ms,
            force,
        ),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum, Serialize)]
#[serde(rename_all = "camelCase")]
enum BenchSuite {
    Project,
    Synthetic,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum, Serialize)]
#[serde(rename_all = "camelCase")]
enum BenchCaseKindFilter {
    All,
    Sample,
    Generator,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
enum BenchCaseKind {
    Sample,
    Generator,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum, Serialize)]
#[serde(rename_all = "camelCase")]
enum BenchMatrix {
    Standard,
    Stress,
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

struct BenchEffectOptions<'a> {
    path: &'a Path,
    sequence: Option<&'a str>,
    time_seconds: Option<f64>,
    suite: BenchSuite,
    case_kind: BenchCaseKindFilter,
    matrix: BenchMatrix,
    iterations: usize,
    warmup: usize,
    json: bool,
    synthetic_active_effects: Option<usize>,
    isolate_effect_id: Option<u32>,
    no_effect_breakdown: bool,
}

struct BaselineOptions<'a> {
    path: &'a Path,
    sequence: Option<&'a str>,
    iterations: usize,
    warmup: usize,
    json: bool,
}

fn bench_effect(options: BenchEffectOptions<'_>) -> Result<ExitCode, String> {
    let time_seconds = match (options.suite, options.time_seconds) {
        (BenchSuite::Project, Some(time_seconds)) => time_seconds,
        (BenchSuite::Project, None) => {
            return Err("--time is required for --suite project".to_string())
        }
        (BenchSuite::Synthetic, Some(time_seconds)) => time_seconds,
        (BenchSuite::Synthetic, None) => 1.0,
    };
    if !time_seconds.is_finite() {
        return Err("time must be finite".to_string());
    }
    if options.iterations == 0 {
        return Err("iterations must be greater than zero".to_string());
    }

    let input = project_input(options.path)?;
    let fs = WorkspaceFs::open(&input.root).map_err(|error| error.to_string())?;
    let overlays = match options.suite {
        BenchSuite::Project => Vec::new(),
        BenchSuite::Synthetic => synthetic_effect_overlays(&fs, &input)?,
    };
    let analysis = analyze_project_with_overlays(&fs, input.project_file.clone(), None, overlays);
    if analysis.has_errors() {
        print_human_report(&analysis);
        return Ok(ExitCode::from(1));
    }

    if options.suite == BenchSuite::Synthetic {
        let report = SyntheticSuiteReport::run(
            &input,
            &analysis,
            time_seconds,
            options.iterations,
            options.warmup,
            options.case_kind,
            options.matrix,
        )?;
        if options.json {
            serde_json::to_writer_pretty(std::io::stdout(), &report)
                .map_err(|error| error.to_string())?;
            println!();
        } else {
            print_synthetic_suite_report(&report);
        }
        return Ok(ExitCode::SUCCESS);
    }

    let sequence_target = sequence_target(&analysis, options.sequence)?;
    let document = get_sequence_document(
        &fs,
        sequence_target.path,
        &sequence_target.object_key,
        input.project_file.clone(),
        Vec::new(),
    )?;

    let mut benchmark_time_seconds = time_seconds;
    let document = if let Some(active_count) = options.synthetic_active_effects {
        synthetic_active_effect_document(&document, time_seconds, active_count)?
    } else {
        document
    };
    let document = if let Some(effect_id) = options.isolate_effect_id {
        let isolated = isolated_effect_document(&document, effect_id, benchmark_time_seconds)?;
        benchmark_time_seconds = isolated.time_seconds;
        isolated.document
    } else {
        document
    };

    let report = EffectBenchReport::run(EffectBenchRunInput {
        input: &input,
        analysis: &analysis,
        document: &document,
        time_seconds: benchmark_time_seconds,
        iterations: options.iterations,
        warmup: options.warmup,
        synthetic_active_effects: options.synthetic_active_effects,
        isolate_effect_id: options.isolate_effect_id,
        no_effect_breakdown: options.no_effect_breakdown,
    });

    if options.json {
        serde_json::to_writer_pretty(std::io::stdout(), &report)
            .map_err(|error| error.to_string())?;
        println!();
    } else {
        print_effect_bench_report(&report);
    }

    Ok(ExitCode::SUCCESS)
}

fn baseline(options: BaselineOptions<'_>) -> Result<ExitCode, String> {
    let report = BaselineReport::run(BaselineRunInput {
        path: options.path,
        sequence: options.sequence,
        iterations: options.iterations,
        warmup: options.warmup,
        scenarios: baseline_scenarios(),
    })?;

    if options.json {
        serde_json::to_writer_pretty(std::io::stdout(), &report)
            .map_err(|error| error.to_string())?;
        println!();
    } else {
        print_baseline_report(&report);
    }

    Ok(ExitCode::SUCCESS)
}

fn export_fseq_command(
    path: &Path,
    output: &Path,
    sequence: Option<&str>,
    step_ms: u8,
    force: bool,
) -> Result<ExitCode, String> {
    if output.exists() && !force {
        return Err(format!(
            "output already exists; pass --force to overwrite: {}",
            output.display()
        ));
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
        input.project_file,
        Vec::new(),
    )?;
    let report = export_fseq_file(
        &analysis,
        &document,
        output,
        FseqExportOptions {
            step_ms,
            ..FseqExportOptions::default()
        },
    )
    .map_err(|error| error.to_string())?;

    println!(
        "exported sequence={} step_ms={} frames={} channels={} bytes={} output={}",
        report.sequence,
        report.step_ms,
        report.frame_count,
        report.channel_count,
        report.bytes_written,
        output.display()
    );
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

#[derive(Debug, Clone, Copy)]
struct BaselineScenarioDefinition {
    name: &'static str,
    time_seconds: f64,
}

fn baseline_scenarios() -> Vec<BaselineScenarioDefinition> {
    vec![
        BaselineScenarioDefinition {
            name: "idle_start",
            time_seconds: 1.0,
        },
        BaselineScenarioDefinition {
            name: "intro_build",
            time_seconds: 14.5,
        },
        BaselineScenarioDefinition {
            name: "drop1_entry",
            time_seconds: 41.0,
        },
        BaselineScenarioDefinition {
            name: "drop1_body",
            time_seconds: 66.0,
        },
        BaselineScenarioDefinition {
            name: "drop1_tail",
            time_seconds: 80.0,
        },
        BaselineScenarioDefinition {
            name: "breakdown_dense_marks",
            time_seconds: 132.5,
        },
        BaselineScenarioDefinition {
            name: "drop2_final",
            time_seconds: 180.1,
        },
    ]
}

struct BaselineRunInput<'a> {
    path: &'a Path,
    sequence: Option<&'a str>,
    iterations: usize,
    warmup: usize,
    scenarios: Vec<BaselineScenarioDefinition>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct BaselineReport {
    project_path: String,
    project_root: String,
    sequence: String,
    iterations: usize,
    warmup: usize,
    project: BaselineProjectReport,
    document: BaselineDocumentReport,
    renderer: BaselineRendererReport,
    scenarios: Vec<BaselineScenarioReport>,
}

impl BaselineReport {
    fn run(input: BaselineRunInput<'_>) -> Result<Self, String> {
        if input.iterations == 0 {
            return Err("iterations must be greater than zero".to_string());
        }
        if input.scenarios.is_empty() {
            return Err("baseline must include at least one scenario".to_string());
        }

        let project_input = project_input(input.path)?;
        let fs = WorkspaceFs::open(&project_input.root).map_err(|error| error.to_string())?;
        let ((analysis, analysis_last_run), project_wall_clock) =
            timed_operation(input.iterations, input.warmup, || {
                let analysis = analyze_project_with_overlays(
                    &fs,
                    project_input.project_file.clone(),
                    None,
                    Vec::new(),
                );
                let summary = ProjectAnalysisTimingReport {
                    reachable_file_count: analysis.reachable_file_count(),
                    object_count: analysis.object_count(),
                };
                Ok((analysis, summary))
            })?;
        if analysis.has_errors() {
            print_human_report(&analysis);
            return Err("baseline project analysis has errors".to_string());
        }

        let sequence_target = sequence_target(&analysis, input.sequence)?;
        let sequence_path = sequence_target.path.clone();
        let sequence_key = sequence_target.object_key.clone();
        let (document, document_wall_clock) =
            timed_operation(input.iterations, input.warmup, || {
                get_sequence_document(
                    &fs,
                    sequence_path.clone(),
                    &sequence_key,
                    project_input.project_file.clone(),
                    Vec::new(),
                )
            })?;

        let ((mut evaluator, prepare_timing), prepare_wall_clock) =
            timed_operation(input.iterations, input.warmup, || {
                SequenceFrameEvaluator::new_timed(&analysis, &document)
            })?;

        let scenarios = input
            .scenarios
            .iter()
            .map(|scenario| {
                BaselineScenarioReport::run(
                    &analysis,
                    &document,
                    &mut evaluator,
                    *scenario,
                    input.iterations,
                    input.warmup,
                )
            })
            .collect::<Result<Vec<_>, _>>()?;

        Ok(Self {
            project_path: display_path(&canonicalize_path(&project_input.project_path)),
            project_root: display_path(&canonicalize_path(&project_input.root)),
            sequence: document.object_key.clone(),
            iterations: input.iterations,
            warmup: input.warmup,
            project: BaselineProjectReport {
                analysis_wall_clock: project_wall_clock,
                analysis_internal: AnalysisSummaryReport::from_analysis(&analysis),
                analysis_last_run,
            },
            document: BaselineDocumentReport {
                load_wall_clock: document_wall_clock,
                path: document.path.clone(),
                duration_seconds: document.duration_seconds,
                frame_rate: document.frame_rate,
                mark_collections: document.mark_collections.len(),
                effects: document.effects.len(),
                lanes: document.lanes.len(),
                degraded: document.degraded,
            },
            renderer: BaselineRendererReport {
                prepare_wall_clock,
                prepare: PreparationTimingReport::from_timing(prepare_timing),
                prepared_effects: evaluator.prepared_effect_count(),
            },
            scenarios,
        })
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct BaselineProjectReport {
    analysis_wall_clock: TimingStatsReport,
    analysis_last_run: ProjectAnalysisTimingReport,
    analysis_internal: AnalysisSummaryReport,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ProjectAnalysisTimingReport {
    reachable_file_count: usize,
    object_count: usize,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct AnalysisSummaryReport {
    project_key: String,
    resolved: bool,
    error_count: usize,
    warning_count: usize,
    reachable_file_count: usize,
    object_count: usize,
}

impl AnalysisSummaryReport {
    fn from_analysis(analysis: &ProjectAnalysis) -> Self {
        Self {
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
        }
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct BaselineDocumentReport {
    load_wall_clock: TimingStatsReport,
    path: String,
    duration_seconds: f64,
    frame_rate: u32,
    mark_collections: usize,
    effects: usize,
    lanes: usize,
    degraded: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct BaselineRendererReport {
    prepare_wall_clock: TimingStatsReport,
    prepare: PreparationTimingReport,
    prepared_effects: usize,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct BaselineScenarioReport {
    name: String,
    time_seconds: f64,
    status: String,
    fixture_count: usize,
    pixel_count: usize,
    active_authored_effects: u32,
    active_prepared_effects: u32,
    visited_prepared_effects: u32,
    sampled_pixels: u32,
    vm_sample_evaluations: u32,
    sample_reuse_saved_evaluations: u32,
    sample_reuse_group_hits: u32,
    bytecode: BytecodeAggregateReport,
    frame_wall_clock: TimingStatsReport,
    last_evaluation: EvaluationTimingReport,
}

impl BaselineScenarioReport {
    fn run(
        analysis: &ProjectAnalysis,
        document: &SequenceDocument,
        evaluator: &mut SequenceFrameEvaluator,
        definition: BaselineScenarioDefinition,
        iterations: usize,
        warmup: usize,
    ) -> Result<Self, String> {
        for generation in 0..warmup {
            black_box(evaluator.evaluate(definition.time_seconds, generation as u64));
        }

        let mut frame_samples = Vec::with_capacity(iterations);
        let mut last_frame = None;
        let mut last_timing = None;
        for generation in 0..iterations {
            let start = Instant::now();
            let (frame, timing) =
                evaluator.evaluate_timed(definition.time_seconds, generation as u64);
            frame_samples.push(start.elapsed());
            last_timing = Some(timing);
            last_frame = Some(black_box(frame));
        }

        let frame = last_frame.ok_or_else(|| "baseline scenario did not render".to_string())?;
        let timing = last_timing.unwrap_or_default();
        let active_effects = document
            .effects
            .iter()
            .filter(|effect| effect_is_active(effect, definition.time_seconds))
            .collect::<Vec<_>>();

        Ok(Self {
            name: definition.name.to_string(),
            time_seconds: definition.time_seconds,
            status: output_status_label(&frame.status),
            fixture_count: frame.fixtures.len(),
            pixel_count: frame
                .fixtures
                .iter()
                .map(|fixture| fixture.pixels.len())
                .sum(),
            active_authored_effects: timing.active_authored_effects,
            active_prepared_effects: timing.active_prepared_effects,
            visited_prepared_effects: timing.visited_prepared_effects,
            sampled_pixels: timing.sampled_pixels,
            vm_sample_evaluations: timing.vm_sample_evaluations,
            sample_reuse_saved_evaluations: timing.sample_reuse_saved_evaluations,
            sample_reuse_group_hits: timing.sample_reuse_group_hits,
            bytecode: BytecodeAggregateReport::from_active_effects(analysis, &active_effects),
            frame_wall_clock: TimingStatsReport::from_durations(frame_samples),
            last_evaluation: EvaluationTimingReport::from_timing(timing),
        })
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct EvaluationTimingReport {
    total_ms: f64,
    fixture_clone_ms: f64,
    effect_loop_ms: f64,
    output_frame_ms: f64,
    active_effects: u32,
    active_authored_effects: u32,
    active_prepared_effects: u32,
    visited_prepared_effects: u32,
    sampled_pixels: u32,
    vm_sample_evaluations: u32,
    sample_reuse_saved_evaluations: u32,
    sample_reuse_group_hits: u32,
}

impl EvaluationTimingReport {
    fn from_timing(timing: SequenceFrameEvaluationTiming) -> Self {
        Self {
            total_ms: timing.total_ms,
            fixture_clone_ms: timing.fixture_clone_ms,
            effect_loop_ms: timing.effect_loop_ms,
            output_frame_ms: timing.output_frame_ms,
            active_effects: timing.active_effects,
            active_authored_effects: timing.active_authored_effects,
            active_prepared_effects: timing.active_prepared_effects,
            visited_prepared_effects: timing.visited_prepared_effects,
            sampled_pixels: timing.sampled_pixels,
            vm_sample_evaluations: timing.vm_sample_evaluations,
            sample_reuse_saved_evaluations: timing.sample_reuse_saved_evaluations,
            sample_reuse_group_hits: timing.sample_reuse_group_hits,
        }
    }
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
    isolate_effect_id: Option<u32>,
    no_effect_breakdown: bool,
    total_effects: usize,
    prepared_effects: usize,
    prepare: PreparationTimingReport,
    active_effect_count: usize,
    rendered_active_prepared_effects: u32,
    visited_prepared_effects: u32,
    target_pixel_samples_per_frame: usize,
    vm_sample_evaluations: u32,
    sample_reuse_saved_evaluations: u32,
    sample_reuse_group_hits: u32,
    bytecode: BytecodeAggregateReport,
    whole_frame: TimingStatsReport,
    effects: Vec<EffectBenchItemReport>,
}

impl EffectBenchReport {
    fn run(input: EffectBenchRunInput<'_>) -> Self {
        let (mut evaluator, prepare_timing) =
            SequenceFrameEvaluator::new_timed(input.analysis, input.document)
                .expect("benchmark analysis must resolve before rendering");
        for generation in 0..input.warmup {
            black_box(evaluator.evaluate(input.time_seconds, generation as u64));
        }

        let mut whole_frame_samples = Vec::with_capacity(input.iterations);
        let mut last_evaluation_timing = None;
        for generation in 0..input.iterations {
            let start = Instant::now();
            let (frame, evaluation_timing) =
                evaluator.evaluate_timed(input.time_seconds, generation as u64);
            black_box(frame);
            last_evaluation_timing = Some(evaluation_timing);
            whole_frame_samples.push(start.elapsed());
        }

        let active_effects = input
            .document
            .effects
            .iter()
            .filter(|effect| effect_is_active(effect, input.time_seconds))
            .collect::<Vec<_>>();
        let active_effect_count = active_effects.len();
        let target_pixel_samples_per_frame = active_effects
            .iter()
            .filter_map(|effect| effect.render.as_ref())
            .map(|render| render.target_pixels.len())
            .sum::<usize>();
        let bytecode =
            BytecodeAggregateReport::from_active_effects(input.analysis, &active_effects);
        let effects = if input.no_effect_breakdown {
            Vec::new()
        } else {
            active_effects
                .iter()
                .filter_map(|effect| {
                    EffectBenchItemReport::run(
                        input.analysis,
                        input.document,
                        effect,
                        input.time_seconds,
                        input.iterations,
                    )
                })
                .collect::<Vec<_>>()
        };

        Self {
            project_path: display_path(&canonicalize_path(&input.input.project_path)),
            project_root: display_path(&canonicalize_path(&input.input.root)),
            sequence: input.document.object_key.clone(),
            time_seconds: input.time_seconds,
            iterations: input.iterations,
            warmup: input.warmup,
            synthetic_active_effects: input.synthetic_active_effects,
            isolate_effect_id: input.isolate_effect_id,
            no_effect_breakdown: input.no_effect_breakdown,
            total_effects: input.document.effects.len(),
            prepared_effects: evaluator.prepared_effect_count(),
            prepare: PreparationTimingReport::from_timing(prepare_timing),
            active_effect_count,
            rendered_active_prepared_effects: last_evaluation_timing
                .map(|timing| timing.active_prepared_effects)
                .unwrap_or(0),
            visited_prepared_effects: last_evaluation_timing
                .map(|timing| timing.visited_prepared_effects)
                .unwrap_or(0),
            target_pixel_samples_per_frame,
            vm_sample_evaluations: last_evaluation_timing
                .map(|timing| timing.vm_sample_evaluations)
                .unwrap_or(0),
            sample_reuse_saved_evaluations: last_evaluation_timing
                .map(|timing| timing.sample_reuse_saved_evaluations)
                .unwrap_or(0),
            sample_reuse_group_hits: last_evaluation_timing
                .map(|timing| timing.sample_reuse_group_hits)
                .unwrap_or(0),
            bytecode,
            whole_frame: TimingStatsReport::from_durations(whole_frame_samples),
            effects,
        }
    }
}

struct EffectBenchRunInput<'a> {
    input: &'a ProjectInput,
    analysis: &'a ProjectAnalysis,
    document: &'a SequenceDocument,
    time_seconds: f64,
    iterations: usize,
    warmup: usize,
    synthetic_active_effects: Option<usize>,
    isolate_effect_id: Option<u32>,
    no_effect_breakdown: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct SyntheticSuiteReport {
    project_path: String,
    project_root: String,
    suite: BenchSuite,
    matrix: BenchMatrix,
    iterations: usize,
    warmup: usize,
    time_seconds: f64,
    case_kind: BenchCaseKindFilter,
    cases: Vec<SyntheticCaseReport>,
}

impl SyntheticSuiteReport {
    fn run(
        input: &ProjectInput,
        analysis: &ProjectAnalysis,
        time_seconds: f64,
        iterations: usize,
        warmup: usize,
        case_kind: BenchCaseKindFilter,
        matrix: BenchMatrix,
    ) -> Result<Self, String> {
        let case_definitions = synthetic_case_definitions(case_kind);
        let target_sizes = synthetic_target_sizes(matrix);
        let target_template = synthetic_target_template(analysis)?;
        let max_target_size = target_sizes.iter().copied().max().unwrap_or(0);
        if target_template.pixels.len() < max_target_size {
            return Err(format!(
                "--matrix {:?} requires at least {} pixels, but the project display has {}",
                matrix,
                max_target_size,
                target_template.pixels.len()
            ));
        }

        let mut cases = Vec::new();
        for target_pixels in target_sizes {
            let pixels = target_template
                .pixels
                .iter()
                .take(target_pixels)
                .cloned()
                .collect::<Vec<_>>();
            for definition in &case_definitions {
                let document =
                    synthetic_sequence_document(analysis, definition, &target_template, &pixels)?;
                cases.push(SyntheticCaseReport::run(
                    analysis,
                    &document,
                    definition,
                    target_pixels,
                    time_seconds,
                    iterations,
                    warmup,
                )?);
            }
        }

        Ok(Self {
            project_path: display_path(&canonicalize_path(&input.project_path)),
            project_root: display_path(&canonicalize_path(&input.root)),
            suite: BenchSuite::Synthetic,
            matrix,
            iterations,
            warmup,
            time_seconds,
            case_kind,
            cases,
        })
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct SyntheticCaseReport {
    name: String,
    kind: BenchCaseKind,
    target_pixels: usize,
    scope: String,
    authored_effects: usize,
    prepared_effects: usize,
    generated_children: usize,
    prepare: PreparationTimingReport,
    whole_frame: TimingStatsReport,
    visited_prepared_effects: u32,
    rendered_active_prepared_effects: u32,
    sampled_pixels: u32,
    vm_sample_evaluations: u32,
    sample_reuse_saved_evaluations: u32,
    sample_reuse_group_hits: u32,
}

impl SyntheticCaseReport {
    fn run(
        analysis: &ProjectAnalysis,
        document: &SequenceDocument,
        definition: &SyntheticCaseDefinition,
        target_pixels: usize,
        time_seconds: f64,
        iterations: usize,
        warmup: usize,
    ) -> Result<Self, String> {
        let (mut evaluator, prepare_timing) =
            SequenceFrameEvaluator::new_timed(analysis, document)?;
        for generation in 0..warmup {
            black_box(evaluator.evaluate(time_seconds, generation as u64));
        }

        let mut whole_frame_samples = Vec::with_capacity(iterations);
        let mut last_evaluation_timing = None;
        for generation in 0..iterations {
            let start = Instant::now();
            let (frame, evaluation_timing) =
                evaluator.evaluate_timed(time_seconds, generation as u64);
            black_box(frame);
            last_evaluation_timing = Some(evaluation_timing);
            whole_frame_samples.push(start.elapsed());
        }
        let last_evaluation_timing = last_evaluation_timing.unwrap_or_default();

        Ok(Self {
            name: definition.name.to_string(),
            kind: definition.kind,
            target_pixels,
            scope: format!("{:?}", definition.scope),
            authored_effects: document.effects.len(),
            prepared_effects: prepare_timing.prepared_effect_count,
            generated_children: prepare_timing.generated_child_count,
            prepare: PreparationTimingReport::from_timing(prepare_timing),
            whole_frame: TimingStatsReport::from_durations(whole_frame_samples),
            visited_prepared_effects: last_evaluation_timing.visited_prepared_effects,
            rendered_active_prepared_effects: last_evaluation_timing.active_prepared_effects,
            sampled_pixels: last_evaluation_timing.sampled_pixels,
            vm_sample_evaluations: last_evaluation_timing.vm_sample_evaluations,
            sample_reuse_saved_evaluations: last_evaluation_timing.sample_reuse_saved_evaluations,
            sample_reuse_group_hits: last_evaluation_timing.sample_reuse_group_hits,
        })
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct PreparationTimingReport {
    total_ms: f64,
    layout_template_ms: f64,
    authored_sample_ms: f64,
    generator_expansion_ms: f64,
    timeline_index_ms: f64,
    prepared_effect_count: usize,
    generator_parent_count: usize,
    generated_child_count: usize,
    generator_parents: Vec<GeneratorParentTimingReport>,
}

impl PreparationTimingReport {
    fn from_timing(timing: SequenceFrameEvaluatorPreparationTiming) -> Self {
        Self {
            total_ms: timing.total_ms,
            layout_template_ms: timing.layout_template_ms,
            authored_sample_ms: timing.authored_sample_ms,
            generator_expansion_ms: timing.generator_expansion_ms,
            timeline_index_ms: timing.timeline_index_ms,
            prepared_effect_count: timing.prepared_effect_count,
            generator_parent_count: timing.generator_parent_count,
            generated_child_count: timing.generated_child_count,
            generator_parents: timing
                .generator_parents
                .into_iter()
                .map(GeneratorParentTimingReport::from_timing)
                .collect(),
        }
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct GeneratorParentTimingReport {
    parent_effect_id: u32,
    script: EffectScriptReport,
    target_pixels: usize,
    emitted_children: usize,
    prepared_children: usize,
    prepared_cache_hit: bool,
    topology_cache_hit: bool,
    total_prepare_ms: f64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct EffectScriptReport {
    path: String,
    effect_name: String,
}

impl EffectScriptReport {
    fn from_id(script_id: &EffectScriptId) -> Self {
        Self {
            path: display_path(&script_id.path),
            effect_name: script_id.effect_name.clone(),
        }
    }
}

impl GeneratorParentTimingReport {
    fn from_timing(
        timing: dawn_app_core::output_runtime::GeneratorParentPreparationTiming,
    ) -> Self {
        Self {
            parent_effect_id: timing.parent_effect_id,
            script: EffectScriptReport::from_id(&timing.script_id),
            target_pixels: timing.target_pixels,
            emitted_children: timing.emitted_children,
            prepared_children: timing.prepared_children,
            prepared_cache_hit: timing.prepared_cache_hit,
            topology_cache_hit: timing.topology_cache_hit,
            total_prepare_ms: timing.total_prepare_ms,
        }
    }
}

#[derive(Debug, Clone)]
struct SyntheticCaseDefinition {
    name: &'static str,
    kind: BenchCaseKind,
    script_path: &'static str,
    script: &'static str,
    scope: SequenceEffectScope,
    duration_seconds: f64,
    params: Vec<SequenceEffectParamDocument>,
}

#[derive(Debug)]
struct SyntheticTargetTemplate {
    target: LayoutTargetDocument,
    target_label: String,
    pixels: Vec<SequenceEffectPixelDocument>,
}

fn synthetic_effect_overlays(
    fs: &WorkspaceFs,
    input: &ProjectInput,
) -> Result<Vec<ProjectOverlay>, String> {
    let project_content = fs
        .read_to_string(&input.project_file)
        .map_err(|error| error.to_string())?;
    Ok(vec![
        ProjectOverlay {
            path: input.project_file.clone(),
            content: project_content_with_synthetic_imports(&project_content),
        },
        ProjectOverlay {
            path: synthetic_overlay_path(input, "effects/synthetic-bench-child.effect.dawn")?,
            content: SYNTHETIC_CHILD_EFFECT.to_string(),
        },
        ProjectOverlay {
            path: synthetic_overlay_path(
                input,
                "effects/synthetic-bench-constant-color.effect.dawn",
            )?,
            content: SYNTHETIC_CONSTANT_COLOR_EFFECT.to_string(),
        },
        ProjectOverlay {
            path: synthetic_overlay_path(input, "effects/synthetic-bench-curve-color.effect.dawn")?,
            content: SYNTHETIC_CURVE_COLOR_EFFECT.to_string(),
        },
        ProjectOverlay {
            path: synthetic_overlay_path(input, "effects/synthetic-bench-pixel-math.effect.dawn")?,
            content: SYNTHETIC_PIXEL_MATH_EFFECT.to_string(),
        },
        ProjectOverlay {
            path: synthetic_overlay_path(
                input,
                "effects/synthetic-bench-single-child.effect.dawn",
            )?,
            content: SYNTHETIC_SINGLE_CHILD_GENERATOR.to_string(),
        },
        ProjectOverlay {
            path: synthetic_overlay_path(
                input,
                "effects/synthetic-bench-sequential-sections.effect.dawn",
            )?,
            content: SYNTHETIC_SEQUENTIAL_SECTIONS_GENERATOR.to_string(),
        },
        ProjectOverlay {
            path: synthetic_overlay_path(
                input,
                "effects/synthetic-bench-dense-overlap.effect.dawn",
            )?,
            content: SYNTHETIC_DENSE_OVERLAP_GENERATOR.to_string(),
        },
        ProjectOverlay {
            path: synthetic_overlay_path(input, "effects/synthetic-bench-per-fixture.effect.dawn")?,
            content: SYNTHETIC_PER_FIXTURE_GENERATOR.to_string(),
        },
        ProjectOverlay {
            path: synthetic_overlay_path(input, "effects/synthetic-bench-mark-dense.effect.dawn")?,
            content: SYNTHETIC_MARK_DENSE_GENERATOR.to_string(),
        },
        ProjectOverlay {
            path: synthetic_overlay_path(
                input,
                "effects/synthetic-bench-nested-pulse.effect.dawn",
            )?,
            content: SYNTHETIC_NESTED_PULSE_GENERATOR.to_string(),
        },
        ProjectOverlay {
            path: synthetic_overlay_path(
                input,
                "effects/synthetic-bench-mark-nested-overlap.effect.dawn",
            )?,
            content: SYNTHETIC_MARK_NESTED_OVERLAP_GENERATOR.to_string(),
        },
    ])
}

fn synthetic_overlay_path(input: &ProjectInput, path: &str) -> Result<Utf8PathBuf, String> {
    let root = canonicalize_path(&input.project_path)
        .parent()
        .ok_or_else(|| "project path has no parent".to_string())?
        .to_path_buf();
    Ok(root.join(path))
}

fn project_content_with_synthetic_imports(project_content: &str) -> String {
    let imports = synthetic_effect_overlay_paths()
        .into_iter()
        .enumerate()
        .map(|(index, path)| format!("  - from: {path}\n    as: synthetic_bench_{index}\n"))
        .collect::<String>();
    if let Some(rest) = project_content.strip_prefix("imports:\n") {
        format!("imports:\n{imports}{rest}")
    } else if let Some(rest) = project_content.strip_prefix("imports:\r\n") {
        format!("imports:\n{imports}{rest}")
    } else {
        format!("imports:\n{imports}\n{project_content}")
    }
}

fn synthetic_effect_overlay_paths() -> Vec<&'static str> {
    vec![
        "effects/synthetic-bench-child.effect.dawn",
        "effects/synthetic-bench-constant-color.effect.dawn",
        "effects/synthetic-bench-curve-color.effect.dawn",
        "effects/synthetic-bench-pixel-math.effect.dawn",
        "effects/synthetic-bench-single-child.effect.dawn",
        "effects/synthetic-bench-sequential-sections.effect.dawn",
        "effects/synthetic-bench-dense-overlap.effect.dawn",
        "effects/synthetic-bench-per-fixture.effect.dawn",
        "effects/synthetic-bench-mark-dense.effect.dawn",
        "effects/synthetic-bench-nested-pulse.effect.dawn",
        "effects/synthetic-bench-mark-nested-overlap.effect.dawn",
    ]
}

fn synthetic_case_definitions(case_kind: BenchCaseKindFilter) -> Vec<SyntheticCaseDefinition> {
    let mut cases = vec![
        SyntheticCaseDefinition {
            name: "sample_constant_color",
            kind: BenchCaseKind::Sample,
            script_path: "effects/synthetic-bench-constant-color.effect.dawn",
            script: "SyntheticConstantColor",
            scope: SequenceEffectScope::WholeTarget,
            duration_seconds: 4.0,
            params: vec![param_color("color", Color::new(12, 48, 180))],
        },
        SyntheticCaseDefinition {
            name: "sample_curve_color",
            kind: BenchCaseKind::Sample,
            script_path: "effects/synthetic-bench-curve-color.effect.dawn",
            script: "SyntheticCurveColor",
            scope: SequenceEffectScope::WholeTarget,
            duration_seconds: 4.0,
            params: vec![param_color_curve("gradient")],
        },
        SyntheticCaseDefinition {
            name: "sample_pixel_math",
            kind: BenchCaseKind::Sample,
            script_path: "effects/synthetic-bench-pixel-math.effect.dawn",
            script: "SyntheticPixelMath",
            scope: SequenceEffectScope::WholeTarget,
            duration_seconds: 4.0,
            params: Vec::new(),
        },
        SyntheticCaseDefinition {
            name: "generator_single_child",
            kind: BenchCaseKind::Generator,
            script_path: "effects/synthetic-bench-single-child.effect.dawn",
            script: "SyntheticSingleChild",
            scope: SequenceEffectScope::WholeTarget,
            duration_seconds: 4.0,
            params: vec![param_color("color", Color::new(255, 80, 24))],
        },
        SyntheticCaseDefinition {
            name: "generator_sequential_sections",
            kind: BenchCaseKind::Generator,
            script_path: "effects/synthetic-bench-sequential-sections.effect.dawn",
            script: "SyntheticSequentialSections",
            scope: SequenceEffectScope::WholeTarget,
            duration_seconds: 4.0,
            params: vec![param_integer("section_width_pixels", 8)],
        },
        SyntheticCaseDefinition {
            name: "generator_dense_overlapping_sections",
            kind: BenchCaseKind::Generator,
            script_path: "effects/synthetic-bench-dense-overlap.effect.dawn",
            script: "SyntheticDenseOverlappingSections",
            scope: SequenceEffectScope::WholeTarget,
            duration_seconds: 4.0,
            params: vec![param_integer("section_width_pixels", 2)],
        },
        SyntheticCaseDefinition {
            name: "generator_per_fixture_sections",
            kind: BenchCaseKind::Generator,
            script_path: "effects/synthetic-bench-per-fixture.effect.dawn",
            script: "SyntheticPerFixtureSections",
            scope: SequenceEffectScope::PerFixture,
            duration_seconds: 4.0,
            params: Vec::new(),
        },
        SyntheticCaseDefinition {
            name: "generator_mark_dense_emission",
            kind: BenchCaseKind::Generator,
            script_path: "effects/synthetic-bench-mark-dense.effect.dawn",
            script: "SyntheticMarkDenseEmission",
            scope: SequenceEffectScope::WholeTarget,
            duration_seconds: 4.0,
            params: vec![
                param_marks("beats", "synthetic_marks"),
                param_integer("section_width_pixels", 3),
                param_integer("sections_per_mark", 6),
            ],
        },
        SyntheticCaseDefinition {
            name: "generator_mark_nested_long_overlap",
            kind: BenchCaseKind::Generator,
            script_path: "effects/synthetic-bench-mark-nested-overlap.effect.dawn",
            script: "SyntheticMarkNestedLongOverlap",
            scope: SequenceEffectScope::WholeTarget,
            duration_seconds: 10.0,
            params: vec![
                param_marks("beats", "synthetic_marks"),
                param_integer("section_width_pixels", 2),
                param_integer("nested_sections", 12),
                param_float("pulse_seconds", 5.0),
            ],
        },
    ];
    cases.retain(|case| match case_kind {
        BenchCaseKindFilter::All => true,
        BenchCaseKindFilter::Sample => case.kind == BenchCaseKind::Sample,
        BenchCaseKindFilter::Generator => case.kind == BenchCaseKind::Generator,
    });
    cases
}

fn synthetic_target_sizes(matrix: BenchMatrix) -> Vec<usize> {
    match matrix {
        BenchMatrix::Standard => vec![30, 300, 1_000],
        BenchMatrix::Stress => vec![1_000, 3_000],
    }
}

fn synthetic_target_template(
    analysis: &ProjectAnalysis,
) -> Result<SyntheticTargetTemplate, String> {
    let project = analysis.resolved.as_ref().ok_or_else(|| {
        "Project must resolve before synthetic benchmark is available".to_string()
    })?;
    let render_plan = layout_render_plan(&project.display.layout.fixtures);
    let mut pixels = Vec::new();
    for (fixture_index, fixture) in render_plan.fixtures.iter().enumerate() {
        let pixel_count = fixture.emitters.len();
        for pixel_index in 0..pixel_count {
            pixels.push(SequenceEffectPixelDocument {
                fixture_index,
                pixel_index,
                pixel_count,
            });
        }
    }
    let group_name = project
        .display
        .layout
        .groups
        .first()
        .map(|group| group.name.clone())
        .unwrap_or_else(|| "synthetic".to_string());
    Ok(SyntheticTargetTemplate {
        target: LayoutTargetDocument {
            kind: LayoutTargetKind::Group,
            name: group_name.clone(),
        },
        target_label: format!("Group {group_name}"),
        pixels,
    })
}

fn synthetic_sequence_document(
    analysis: &ProjectAnalysis,
    definition: &SyntheticCaseDefinition,
    target_template: &SyntheticTargetTemplate,
    pixels: &[SequenceEffectPixelDocument],
) -> Result<SequenceDocument, String> {
    let script_id = synthetic_script_id(analysis, definition.script_path)?;
    Ok(SequenceDocument {
        path: "synthetic-bench.sequence.dawn".to_string(),
        object_key: definition.name.to_string(),
        duration_seconds: definition.duration_seconds,
        frame_rate: 60,
        audio: None,
        mark_collections: vec![synthetic_mark_collection()],
        lanes: vec![SequenceLaneDocument {
            target: target_template.target.clone(),
            label: target_template.target_label.clone(),
        }],
        effect_scripts: Vec::new(),
        curve_library: Vec::new(),
        effects: vec![SequenceEffectDocument {
            index: 0,
            id: 1,
            start_seconds: 0.0,
            duration_seconds: definition.duration_seconds,
            target: target_template.target.clone(),
            target_label: target_template.target_label.clone(),
            scope: definition.scope,
            script: definition.script.to_string(),
            script_source: Some(script_id.clone().into()),
            params: definition.params.clone(),
            render: Some(SequenceEffectRenderDocument {
                script: script_id.clone().into(),
                script_source: script_id.display_key(),
                params: definition.params.clone(),
                target_pixels: pixels.to_vec(),
            }),
        }],
        degraded: false,
    })
}

fn synthetic_script_id(analysis: &ProjectAnalysis, path: &str) -> Result<EffectScriptId, String> {
    let suffix = path.replace('\\', "/");
    let matches = analysis
        .scripts
        .keys()
        .filter(|script_id| script_id.path.to_slash_string().ends_with(&suffix))
        .cloned()
        .collect::<Vec<_>>();
    match matches.as_slice() {
        [script] => Ok(script.clone()),
        [] => Err(format!("synthetic effect script `{path}` was not analyzed")),
        _ => Err(format!(
            "synthetic effect script `{path}` matched multiple analyzed scripts"
        )),
    }
}

fn synthetic_mark_collection() -> SequenceMarkCollectionDocument {
    SequenceMarkCollectionDocument {
        key: "synthetic_marks".to_string(),
        name: "Synthetic Marks".to_string(),
        color: "#38bdf8".to_string(),
        marks_seconds: vec![
            0.15, 0.28, 0.43, 0.61, 0.78, 0.94, 1.12, 1.31, 1.48, 1.66, 1.83, 2.01, 2.18, 2.36,
            2.54, 2.71, 2.88, 3.05, 3.22, 3.39, 3.56, 3.73,
        ],
    }
}

fn param_color(name: &str, value: Color) -> SequenceEffectParamDocument {
    SequenceEffectParamDocument {
        name: name.to_string(),
        value: EffectParam::Color { value },
        curve_source: None,
    }
}

fn param_integer(name: &str, value: u64) -> SequenceEffectParamDocument {
    SequenceEffectParamDocument {
        name: name.to_string(),
        value: EffectParam::Integer { value },
        curve_source: None,
    }
}

fn param_float(name: &str, value: f64) -> SequenceEffectParamDocument {
    SequenceEffectParamDocument {
        name: name.to_string(),
        value: EffectParam::Float { value },
        curve_source: None,
    }
}

fn param_marks(name: &str, key: &str) -> SequenceEffectParamDocument {
    SequenceEffectParamDocument {
        name: name.to_string(),
        value: EffectParam::Marks {
            key: key.to_string(),
        },
        curve_source: None,
    }
}

fn param_color_curve(name: &str) -> SequenceEffectParamDocument {
    SequenceEffectParamDocument {
        name: name.to_string(),
        value: EffectParam::Curve {
            curve: Curve {
                value_type: CurveValueType::Color,
                points: vec![
                    CurvePoint {
                        time: 0.0,
                        value: CurveValue::Color(Color::new(255, 32, 16)),
                    },
                    CurvePoint {
                        time: 0.5,
                        value: CurveValue::Color(Color::new(16, 220, 120)),
                    },
                    CurvePoint {
                        time: 1.0,
                        value: CurveValue::Color(Color::new(40, 120, 255)),
                    },
                ],
            },
        },
        curve_source: None,
    }
}

const SYNTHETIC_CONSTANT_COLOR_EFFECT: &str = r##"
effect SyntheticConstantColor {
  param color color = #ffffff;

  color sample(float progress, float seconds, Fixture fixture, Pixel pixel) {
    return color;
  }
}
"##;

const SYNTHETIC_CURVE_COLOR_EFFECT: &str = r##"
effect SyntheticCurveColor {
  param curve<color> gradient;

  color sample(float progress, float seconds, Fixture fixture, Pixel pixel) {
    return gradient(progress);
  }
}
"##;

const SYNTHETIC_PIXEL_MATH_EFFECT: &str = r##"
effect SyntheticPixelMath {
  color sample(float progress, float seconds, Fixture fixture, Pixel pixel) {
    float index = pixel_index(pixel);
    float count = max(1.0, pixel_count(pixel));
    float level = (index + 1.0) / count;
    return rgb(level * 255.0, progress * 255.0, min(255.0, seconds * 40.0));
  }
}
"##;

const SYNTHETIC_CHILD_EFFECT: &str = r##"
internal effect SyntheticChild {
  param color color = #ffffff;

  color sample(float progress, float seconds, Fixture fixture, Pixel pixel) {
    return color;
  }
}
"##;

const SYNTHETIC_SINGLE_CHILD_GENERATOR: &str = r##"
use "./synthetic-bench-child.effect.dawn" as effects;

effect SyntheticSingleChild {
  param color color = #ffffff;

  void generate(Timeline timeline, Target target, float duration) {
    timeline.emit effects.SyntheticChild {
      target: target;
      start: 0.0;
      duration: duration;
      params: {
        color: color;
      };
    };
  }
}
"##;

const SYNTHETIC_SEQUENTIAL_SECTIONS_GENERATOR: &str = r##"
use "./synthetic-bench-child.effect.dawn" as effects;

effect SyntheticSequentialSections {
  param int section_width_pixels = 8;

  void generate(Timeline timeline, Target target, float duration) {
    TargetItems items = sections(target, section_width_pixels);
    int item_count = count(items);
    for (int i = 0; i < item_count; i = i + 1) {
      TargetItem item = pick(items, i);
      timeline.emit effects.SyntheticChild {
        target: item.target;
        start: (i / max(1.0, item_count)) * duration;
        duration: max(0.1, duration / max(1.0, item_count));
        params: {
          color: hsv(item.position * 360.0, 1.0, 1.0);
        };
      };
    }
  }
}
"##;

const SYNTHETIC_DENSE_OVERLAP_GENERATOR: &str = r##"
use "./synthetic-bench-child.effect.dawn" as effects;

effect SyntheticDenseOverlappingSections {
  param int section_width_pixels = 2;

  void generate(Timeline timeline, Target target, float duration) {
    TargetItems items = sections(target, section_width_pixels);
    int item_count = count(items);
    for (int i = 0; i < item_count; i = i + 1) {
      TargetItem item = pick(items, i);
      timeline.emit effects.SyntheticChild {
        target: item.target;
        start: 0.0;
        duration: duration;
        params: {
          color: hsv(item.position * 360.0, 0.8, 1.0);
        };
      };
    }
  }
}
"##;

const SYNTHETIC_PER_FIXTURE_GENERATOR: &str = r##"
use "./synthetic-bench-child.effect.dawn" as effects;

effect SyntheticPerFixtureSections {
  void generate(Timeline timeline, Target target, float duration) {
    TargetItems items = fixtures(target);
    int item_count = count(items);
    for (int i = 0; i < item_count; i = i + 1) {
      TargetItem item = pick(items, i);
      timeline.emit effects.SyntheticChild {
        target: item.target;
        start: 0.0;
        duration: duration;
        params: {
          color: hsv(item.position * 360.0, 1.0, 1.0);
        };
      };
    }
  }
}
"##;

const SYNTHETIC_MARK_DENSE_GENERATOR: &str = r##"
use "./synthetic-bench-child.effect.dawn" as effects;

effect SyntheticMarkDenseEmission {
  param marks beats;
  param int section_width_pixels = 3;
  param int sections_per_mark = 6;

  void generate(Timeline timeline, Target target, float duration) {
    TargetItems items = sections(target, section_width_pixels);
    int item_count = count(items);
    int beat_count = mark_count(beats);
    for (int beat = 0; beat < beat_count; beat = beat + 1) {
      float hit = mark_at(beats, beat, 0.0);
      if (hit >= 0.0 && hit < duration) {
        for (int section = 0; section < sections_per_mark; section = section + 1) {
          int choice = floor(rand(beat, section) * item_count);
          TargetItem item = pick(items, choice);
          timeline.emit effects.SyntheticChild {
            target: item.target;
            start: hit;
            duration: 0.25;
            params: {
              color: hsv(item.position * 360.0, 1.0, 1.0);
            };
          };
        }
      }
    }
  }
}
"##;

const SYNTHETIC_NESTED_PULSE_GENERATOR: &str = r##"
use "./synthetic-bench-child.effect.dawn" as effects;

effect SyntheticNestedPulse {
  param int section_width_pixels = 2;
  param int nested_sections = 12;

  void generate(Timeline timeline, Target target, float duration) {
    TargetItems items = sections(target, section_width_pixels);
    int item_count = count(items);
    for (int section = 0; section < nested_sections; section = section + 1) {
      int choice = floor(rand(section, nested_sections) * item_count);
      TargetItem item = pick(items, choice);
      timeline.emit effects.SyntheticChild {
        target: item.target;
        start: 0.0;
        duration: duration;
        params: {
          color: hsv(item.position * 360.0, 1.0, 1.0);
        };
      };
    }
  }
}
"##;

const SYNTHETIC_MARK_NESTED_OVERLAP_GENERATOR: &str = r##"
use "./synthetic-bench-nested-pulse.effect.dawn" as nested;

effect SyntheticMarkNestedLongOverlap {
  param marks beats;
  param int section_width_pixels = 2;
  param int nested_sections = 12;
  param float pulse_seconds = 5.0;

  void generate(Timeline timeline, Target target, float duration) {
    TargetItems items = sections(target, section_width_pixels);
    int item_count = count(items);
    int beat_count = mark_count(beats);
    for (int beat = 0; beat < beat_count; beat = beat + 1) {
      float hit = mark_at(beats, beat, 0.0);
      if (hit >= 0.0 && hit + pulse_seconds <= duration) {
        int choice = floor(rand(beat, nested_sections) * item_count);
        TargetItem item = pick(items, choice);
        timeline.emit nested.SyntheticNestedPulse {
          target: item.target;
          start: hit;
          duration: pulse_seconds;
          params: {
            section_width_pixels: section_width_pixels;
            nested_sections: nested_sections;
          };
        };
      }
    }
  }
}
"##;

#[derive(Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
struct BytecodeAggregateReport {
    instruction_count: usize,
    constant_count: usize,
    param_slots: usize,
    float_slots: usize,
    int_slots: usize,
    bool_slots: usize,
    color_slots: usize,
    ref_slots: usize,
    fixture_slots: usize,
    pixel_slots: usize,
    total_slots: usize,
}

impl BytecodeAggregateReport {
    fn from_active_effects(
        analysis: &ProjectAnalysis,
        effects: &[&SequenceEffectDocument],
    ) -> Self {
        effects
            .iter()
            .filter_map(|effect| {
                let render = effect.render.as_ref()?;
                analysis.compiled_script_for_id(&render.script.to_script_id())
            })
            .map(|script| script.bytecode_stats())
            .fold(Self::default(), |mut aggregate, stats| {
                aggregate.instruction_count += stats.instruction_count;
                aggregate.constant_count += stats.constant_count;
                aggregate.param_slots += stats.param_slots;
                aggregate.float_slots += stats.float_slots;
                aggregate.int_slots += stats.int_slots;
                aggregate.bool_slots += stats.bool_slots;
                aggregate.color_slots += stats.color_slots;
                aggregate.ref_slots += stats.ref_slots;
                aggregate.fixture_slots += stats.fixture_slots;
                aggregate.pixel_slots += stats.pixel_slots;
                aggregate.total_slots += stats.total_slots;
                aggregate
            })
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct EffectBenchItemReport {
    effect_id: u32,
    effect_index: usize,
    script_label: String,
    script: EffectScriptReport,
    scope: String,
    target_label: String,
    target_pixels: usize,
    params: usize,
    instruction_count: usize,
    constant_count: usize,
    param_slots: usize,
    float_slots: usize,
    int_slots: usize,
    bool_slots: usize,
    color_slots: usize,
    ref_slots: usize,
    fixture_slots: usize,
    pixel_slots: usize,
    total_slots: usize,
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
        let script = analysis.compiled_script_for_id(&render.script.to_script_id())?;
        let prepared_params = prepare_params_from_document(
            script,
            &render.params,
            &document.mark_collections,
            effect.start_seconds,
            effect.duration_seconds,
        )
        .ok()?;
        let stats = script.bytecode_stats();
        let mut scratch = EffectSampleScratch::new(stats);
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
                let _ = black_box(script.sample_prepared_with_scratch(
                    progress,
                    local_seconds,
                    FixtureContext {
                        index: pixel.fixture_index,
                    },
                    pixel_context,
                    &prepared_params,
                    &mut scratch,
                ));
            }
            let elapsed = start.elapsed();
            effect_samples.push(elapsed);
            per_sample_samples.push(divide_duration(elapsed, target_pixel_count));
        }

        Some(Self {
            effect_id: effect.id,
            effect_index: effect.index,
            script_label: effect.script.clone(),
            script: EffectScriptReport::from_id(&render.script.to_script_id()),
            scope: format!("{:?}", effect.scope),
            target_label: effect.target_label.clone(),
            target_pixels: target_pixel_count,
            params: render.params.len(),
            instruction_count: stats.instruction_count,
            constant_count: stats.constant_count,
            param_slots: stats.param_slots,
            float_slots: stats.float_slots,
            int_slots: stats.int_slots,
            bool_slots: stats.bool_slots,
            color_slots: stats.color_slots,
            ref_slots: stats.ref_slots,
            fixture_slots: stats.fixture_slots,
            pixel_slots: stats.pixel_slots,
            total_slots: stats.total_slots,
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

#[derive(Debug)]
struct IsolatedEffectDocument {
    document: SequenceDocument,
    time_seconds: f64,
}

fn isolated_effect_document(
    document: &SequenceDocument,
    effect_id: u32,
    time_seconds: f64,
) -> Result<IsolatedEffectDocument, String> {
    let source = document
        .effects
        .iter()
        .find(|effect| effect.id == effect_id)
        .ok_or_else(|| format!("effect id {effect_id} was not found"))?;
    if source.render.is_none() {
        return Err(format!("effect id {effect_id} is not renderable"));
    }
    let effect_end_seconds = source.start_seconds + source.duration_seconds;
    if time_seconds < source.start_seconds || time_seconds >= effect_end_seconds {
        return Err(format!(
            "--time must fall within isolated effect id {effect_id}: {:.3}s <= time < {:.3}s",
            source.start_seconds, effect_end_seconds
        ));
    }

    let mut effect = source.clone();
    effect.index = 0;
    effect.start_seconds = 0.0;

    let mark_collections = document
        .mark_collections
        .iter()
        .map(|collection| SequenceMarkCollectionDocument {
            key: collection.key.clone(),
            name: collection.name.clone(),
            color: collection.color.clone(),
            marks_seconds: collection
                .marks_seconds
                .iter()
                .copied()
                .filter(|mark| *mark >= source.start_seconds && *mark < effect_end_seconds)
                .map(|mark| mark - source.start_seconds)
                .collect(),
        })
        .collect::<Vec<_>>();

    Ok(IsolatedEffectDocument {
        document: SequenceDocument {
            path: document.path.clone(),
            object_key: format!("{}#effect-{effect_id}", document.object_key),
            duration_seconds: source.duration_seconds,
            frame_rate: document.frame_rate,
            audio: None,
            mark_collections,
            lanes: document.lanes.clone(),
            effect_scripts: document.effect_scripts.clone(),
            curve_library: document.curve_library.clone(),
            effects: vec![effect],
            degraded: document.degraded,
        },
        time_seconds: time_seconds - source.start_seconds,
    })
}

fn timed_operation<T>(
    iterations: usize,
    warmup: usize,
    mut operation: impl FnMut() -> Result<T, String>,
) -> Result<(T, TimingStatsReport), String> {
    if iterations == 0 {
        return Err("iterations must be greater than zero".to_string());
    }
    for _ in 0..warmup {
        black_box(operation()?);
    }

    let mut samples = Vec::with_capacity(iterations);
    let mut last = None;
    for _ in 0..iterations {
        let start = Instant::now();
        let result = operation()?;
        samples.push(start.elapsed());
        last = Some(black_box(result));
    }

    let result = last.ok_or_else(|| "timed operation did not run".to_string())?;
    Ok((result, TimingStatsReport::from_durations(samples)))
}

fn output_status_label(status: &OutputFrameStatus) -> String {
    match status {
        OutputFrameStatus::Live => "live".to_string(),
        OutputFrameStatus::Idle(message) => format!("idle: {message}"),
        OutputFrameStatus::Error(message) => format!("error: {message}"),
    }
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

fn print_baseline_report(report: &BaselineReport) {
    println!(
        "baseline project={} sequence={} iterations={} warmup={}",
        report.project_path, report.sequence, report.iterations, report.warmup
    );
    println!(
        "project key={} resolved={} reachable_files={} objects={} errors={} warnings={}",
        report.project.analysis_internal.project_key,
        report.project.analysis_internal.resolved,
        report.project.analysis_internal.reachable_file_count,
        report.project.analysis_internal.object_count,
        report.project.analysis_internal.error_count,
        report.project.analysis_internal.warning_count
    );
    print_timing_stats("analysis", &report.project.analysis_wall_clock);
    println!(
        "document path={} duration={:.3}s fps={} lanes={} marks={} effects={} degraded={}",
        report.document.path,
        report.document.duration_seconds,
        report.document.frame_rate,
        report.document.lanes,
        report.document.mark_collections,
        report.document.effects,
        report.document.degraded
    );
    print_timing_stats("document load", &report.document.load_wall_clock);
    println!(
        "renderer prepared_effects={} generated_children={} generator_parents={}",
        report.renderer.prepared_effects,
        report.renderer.prepare.generated_child_count,
        report.renderer.prepare.generator_parent_count
    );
    print_timing_stats("renderer prepare", &report.renderer.prepare_wall_clock);
    println!(
        "{:<24} {:>9} {:>8} {:>8} {:>8} {:>9} {:>9} {:>9} {:>10}",
        "scenario",
        "time",
        "status",
        "active",
        "visited",
        "pixels",
        "samples",
        "reuse",
        "frame p50"
    );
    for scenario in &report.scenarios {
        println!(
            "{:<24} {:>8.3}s {:>8} {:>8} {:>8} {:>9} {:>9} {:>9} {:>9.3}ms",
            scenario.name,
            scenario.time_seconds,
            scenario.status,
            scenario.active_authored_effects,
            scenario.visited_prepared_effects,
            scenario.pixel_count,
            scenario.vm_sample_evaluations,
            scenario.sample_reuse_saved_evaluations,
            scenario.frame_wall_clock.p50_ms
        );
        println!(
            "  eval total={:.3}ms clone={:.3}ms effects={:.3}ms output={:.3}ms active_prepared={} sampled_pixels={} sample_reuse_group_hits={}",
            scenario.last_evaluation.total_ms,
            scenario.last_evaluation.fixture_clone_ms,
            scenario.last_evaluation.effect_loop_ms,
            scenario.last_evaluation.output_frame_ms,
            scenario.active_prepared_effects,
            scenario.sampled_pixels,
            scenario.sample_reuse_group_hits
        );
    }
}

fn print_effect_bench_report(report: &EffectBenchReport) {
    println!(
        "project={} sequence={} time={:.3}s iterations={} warmup={} synthetic_active_effects={} isolate_effect_id={} effect_breakdown={}",
        report.project_path,
        report.sequence,
        report.time_seconds,
        report.iterations,
        report.warmup,
        report
            .synthetic_active_effects
            .map(|count| count.to_string())
            .unwrap_or_else(|| "none".to_string()),
        report
            .isolate_effect_id
            .map(|id| id.to_string())
            .unwrap_or_else(|| "none".to_string()),
        if report.no_effect_breakdown {
            "disabled"
        } else {
            "enabled"
        }
    );
    println!(
        "total effects={} prepared effects={} authored active effects={} prepared active effects={} visited prepared effects/frame={} target pixel samples/frame={} vm sample evaluations/frame={} sample reuse saved/frame={} sample reuse group hits/frame={} bytecode=instructions:{} constants:{} param_slots:{} registers=float:{} int:{} bool:{} color:{} ref:{} fixture:{} pixel:{} total:{}",
        report.total_effects,
        report.prepared_effects,
        report.active_effect_count,
        report.rendered_active_prepared_effects,
        report.visited_prepared_effects,
        report.target_pixel_samples_per_frame,
        report.vm_sample_evaluations,
        report.sample_reuse_saved_evaluations,
        report.sample_reuse_group_hits,
        report.bytecode.instruction_count,
        report.bytecode.constant_count,
        report.bytecode.param_slots,
        report.bytecode.float_slots,
        report.bytecode.int_slots,
        report.bytecode.bool_slots,
        report.bytecode.color_slots,
        report.bytecode.ref_slots,
        report.bytecode.fixture_slots,
        report.bytecode.pixel_slots,
        report.bytecode.total_slots
    );
    println!(
        "prepare total={:.3}ms layout={:.3}ms authored_sample={:.3}ms generator_expand={:.3}ms timeline_index={:.3}ms generator_parents={}",
        report.prepare.total_ms,
        report.prepare.layout_template_ms,
        report.prepare.authored_sample_ms,
        report.prepare.generator_expansion_ms,
        report.prepare.timeline_index_ms,
        report.prepare.generator_parent_count
    );
    let mut generator_parents = report.prepare.generator_parents.iter().collect::<Vec<_>>();
    generator_parents.sort_by(|left, right| {
        right
            .total_prepare_ms
            .total_cmp(&left.total_prepare_ms)
            .then(left.parent_effect_id.cmp(&right.parent_effect_id))
    });
    for parent in generator_parents.into_iter().take(5) {
        println!(
            "generator parent id={} script={} target_pixels={} emitted_children={} prepared_children={} prepared_cache_hit={} topology_cache_hit={} prepare={:.3}ms",
            parent.parent_effect_id,
            parent.script.effect_name,
            parent.target_pixels,
            parent.emitted_children,
            parent.prepared_children,
            parent.prepared_cache_hit,
            parent.topology_cache_hit,
            parent.total_prepare_ms
        );
    }
    print_timing_stats("whole frame", &report.whole_frame);
    if report.no_effect_breakdown {
        println!("per-effect timing=disabled");
        return;
    }
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
            "effect id={} index={} script={} target={} pixels={} params={} scope={} bytecode=instructions:{} constants:{} param_slots:{} registers=float:{} int:{} bool:{} color:{} ref:{} fixture:{} pixel:{} total:{}",
            effect.effect_id,
            effect.effect_index,
            effect.script_label,
            effect.target_label,
            effect.target_pixels,
            effect.params,
            effect.scope,
            effect.instruction_count,
            effect.constant_count,
            effect.param_slots,
            effect.float_slots,
            effect.int_slots,
            effect.bool_slots,
            effect.color_slots,
            effect.ref_slots,
            effect.fixture_slots,
            effect.pixel_slots,
            effect.total_slots
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

fn print_synthetic_suite_report(report: &SyntheticSuiteReport) {
    println!(
        "project={} suite=synthetic matrix={:?} time={:.3}s iterations={} warmup={} case_kind={:?}",
        report.project_path,
        report.matrix,
        report.time_seconds,
        report.iterations,
        report.warmup,
        report.case_kind
    );
    println!(
        "{:<42} {:<9} {:>7} {:>8} {:>9} {:>9} {:>10} {:>10} {:>10}",
        "case",
        "kind",
        "pixels",
        "authored",
        "prepared",
        "children",
        "prep",
        "frame p50",
        "sampled"
    );
    for case in &report.cases {
        println!(
            "{:<42} {:<9} {:>7} {:>8} {:>9} {:>9} {:>9.3}ms {:>9.3}ms {:>10}",
            case.name,
            format!("{:?}", case.kind),
            case.target_pixels,
            case.authored_effects,
            case.prepared_effects,
            case.generated_children,
            case.prepare.total_ms,
            case.whole_frame.p50_ms,
            case.sampled_pixels
        );
        println!(
            "  prepare total={:.3}ms layout={:.3}ms authored_sample={:.3}ms generator_expand={:.3}ms timeline_index={:.3}ms generator_parents={}",
            case.prepare.total_ms,
            case.prepare.layout_template_ms,
            case.prepare.authored_sample_ms,
            case.prepare.generator_expansion_ms,
            case.prepare.timeline_index_ms,
            case.prepare.generator_parent_count
        );
        println!(
            "  render visited_prepared={} active_prepared={} sampled_pixels={} vm_sample_evaluations={} sample_reuse_saved={} sample_reuse_group_hits={}",
            case.visited_prepared_effects,
            case.rendered_active_prepared_effects,
            case.sampled_pixels,
            case.vm_sample_evaluations,
            case.sample_reuse_saved_evaluations,
            case.sample_reuse_group_hits
        );
        let mut generator_parents = case.prepare.generator_parents.iter().collect::<Vec<_>>();
        generator_parents.sort_by(|left, right| {
            right
                .total_prepare_ms
                .total_cmp(&left.total_prepare_ms)
                .then(left.parent_effect_id.cmp(&right.parent_effect_id))
        });
        for parent in generator_parents.into_iter().take(5) {
            println!(
                "  generator parent id={} script={} target_pixels={} emitted_children={} prepared_children={} prepared_cache_hit={} topology_cache_hit={} prepare={:.3}ms",
                parent.parent_effect_id,
                parent.script.effect_name,
                parent.target_pixels,
                parent.emitted_children,
                parent.prepared_children,
                parent.prepared_cache_hit,
                parent.topology_cache_hit,
                parent.total_prepare_ms
            );
        }
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
    use dawn_app_core::output_runtime::evaluate_sequence_frame;
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
            script_source: Some(dawn_project::document::EffectScriptReferenceDocument {
                path: "effects/pulse.effect.dawn".to_string(),
                effect_name: "pulse".to_string(),
            }),
            params: Vec::new(),
            render: Some(SequenceEffectRenderDocument {
                script: dawn_project::document::EffectScriptReferenceDocument {
                    path: "effects/pulse.effect.dawn".to_string(),
                    effect_name: "pulse".to_string(),
                },
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
            curve_library: Vec::new(),
            effects: vec![effect],
            degraded: false,
        }
    }

    fn synthetic_analysis() -> (ProjectInput, ProjectAnalysis) {
        let project_path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("..")
            .join("examples")
            .join("thirty-output-controller");
        let input = project_input(&project_path).unwrap();
        let fs = WorkspaceFs::open(&input.root).unwrap();
        let overlays = synthetic_effect_overlays(&fs, &input).unwrap();
        let analysis =
            analyze_project_with_overlays(&fs, input.project_file.clone(), None, overlays);
        assert!(!analysis.has_errors());
        (input, analysis)
    }

    fn thirty_output_project_path() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("..")
            .join("examples")
            .join("thirty-output-controller")
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

    #[test]
    fn synthetic_suite_construction_includes_sample_and_generator_cases() {
        let cases = synthetic_case_definitions(BenchCaseKindFilter::All);

        assert!(cases.iter().any(|case| case.kind == BenchCaseKind::Sample));
        assert!(cases
            .iter()
            .any(|case| case.kind == BenchCaseKind::Generator));
    }

    #[test]
    fn generator_suite_case_reports_generated_children() {
        let (input, analysis) = synthetic_analysis();

        let report = SyntheticSuiteReport::run(
            &input,
            &analysis,
            1.0,
            1,
            0,
            BenchCaseKindFilter::Generator,
            BenchMatrix::Standard,
        )
        .unwrap();

        assert!(report.cases.iter().any(|case| case.generated_children > 0));
    }

    #[test]
    fn synthetic_suite_json_includes_prepare_and_render_timing_fields() {
        let (input, analysis) = synthetic_analysis();
        let report = SyntheticSuiteReport::run(
            &input,
            &analysis,
            1.0,
            1,
            0,
            BenchCaseKindFilter::Sample,
            BenchMatrix::Standard,
        )
        .unwrap();

        let value = serde_json::to_value(report).unwrap();
        let first_case = &value["cases"][0];

        assert!(first_case["prepare"]["totalMs"].is_number());
        assert!(first_case["prepare"]["timelineIndexMs"].is_number());
        assert!(first_case["wholeFrame"]["p50Ms"].is_number());
        assert!(first_case["sampledPixels"].is_number());
    }

    #[test]
    fn baseline_scenarios_cover_thirty_output_controller_flow() {
        let scenarios = baseline_scenarios();

        assert!(scenarios
            .iter()
            .any(|scenario| scenario.name == "idle_start"));
        assert!(scenarios
            .iter()
            .any(|scenario| scenario.name == "drop1_entry"));
        assert!(scenarios
            .iter()
            .any(|scenario| scenario.name == "drop2_final"));
        assert!(scenarios
            .windows(2)
            .all(|pair| pair[0].time_seconds < pair[1].time_seconds));
    }

    #[test]
    fn baseline_report_includes_granular_metric_groups() {
        let path = thirty_output_project_path();
        let report = BaselineReport::run(BaselineRunInput {
            path: &path,
            sequence: None,
            iterations: 1,
            warmup: 0,
            scenarios: vec![BaselineScenarioDefinition {
                name: "drop1_entry",
                time_seconds: 41.0,
            }],
        })
        .unwrap();

        assert_eq!(
            report.project.analysis_internal.project_key,
            "thirty_output_controller"
        );
        assert!(report.project.analysis_wall_clock.p50_ms >= 0.0);
        assert_eq!(report.document.frame_rate, 144);
        assert!(report.document.effects > 100);
        assert!(report.renderer.prepared_effects > report.document.effects);
        assert!(report.renderer.prepare.generated_child_count > 0);

        let scenario = report
            .scenarios
            .first()
            .expect("scenario should be reported");
        assert_eq!(scenario.name, "drop1_entry");
        assert_eq!(scenario.fixture_count, 30);
        assert!(scenario.pixel_count > 0);
        assert!(scenario.active_authored_effects > 0);
        assert!(scenario.visited_prepared_effects > 0);
        assert!(scenario.frame_wall_clock.p50_ms >= 0.0);
        assert!(scenario.last_evaluation.total_ms >= 0.0);
        assert!(
            scenario.vm_sample_evaluations + scenario.sample_reuse_saved_evaluations
                >= scenario.sampled_pixels
        );
    }
}
