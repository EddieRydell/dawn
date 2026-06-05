use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use camino::Utf8PathBuf;
use clap::{Parser, Subcommand};
use dawn_backend::{
    AppBackend, AppUpdate, AppView, BackendTask, BackendTaskOutput, FseqExportOptions,
};
use dawn_language::analysis::{
    DiagnosticCode, DiagnosticSeverity, ProjectAnalysis, ProjectDiagnostic, TextRange,
};
use dawn_language::document::get_sequence_document;
use dawn_language::fs::WorkspaceFs;
use dawn_language::model::DawnObject;
use dawn_language::path::{canonicalize_path, utf8_path, PathStringExt};
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
    run().unwrap_or_else(|message| {
        eprintln!("{message}");
        ExitCode::from(2)
    })
}

fn run() -> Result<ExitCode, String> {
    match Cli::parse().command {
        Command::Analyze {
            project_path_or_directory,
            json,
        } => analyze(&project_path_or_directory, json),
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

fn analyze(path: &Path, json: bool) -> Result<ExitCode, String> {
    let input = project_input(path)?;
    let view = open_project_view(path)?;
    let analysis = view
        .analysis
        .as_ref()
        .ok_or_else(|| "project analysis is not available".to_string())?;

    if json {
        let report = AnalysisReport::from_analysis(analysis, &input);
        serde_json::to_writer_pretty(std::io::stdout(), &report)
            .map_err(|error| error.to_string())?;
        println!();
    } else {
        print_human_report(analysis);
    }

    if analysis.has_errors() {
        Ok(ExitCode::from(1))
    } else {
        Ok(ExitCode::SUCCESS)
    }
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

    let mut backend = AppBackend::new();
    let update = backend
        .open_project(path.to_path_buf())
        .map_err(format_backend_error)?;
    let view = drain_update(&mut backend, update)?;
    let analysis = view
        .analysis
        .as_ref()
        .ok_or_else(|| "project analysis is not available".to_string())?;
    if analysis.has_errors() {
        print_human_report(analysis);
        return Ok(ExitCode::from(1));
    }

    let sequence_target = sequence_target(analysis, sequence)?;
    let project_root = view
        .project_root
        .as_deref()
        .ok_or_else(|| "project root is not available".to_string())?;
    let project_file = view
        .project_file
        .as_deref()
        .ok_or_else(|| "project file is not available".to_string())?;
    let project_root = Utf8PathBuf::from(project_root);
    let fs = WorkspaceFs::open(project_root.as_std_path()).map_err(|error| error.to_string())?;
    let document = get_sequence_document(
        &fs,
        sequence_target.path,
        &sequence_target.object_key,
        Utf8PathBuf::from(project_file),
        Vec::new(),
    )?;
    let output_path = utf8_path(output)?;

    let update = backend
        .export_fseq(
            document,
            output_path,
            FseqExportOptions {
                step_ms,
                ..FseqExportOptions::default()
            },
        )
        .map_err(format_backend_error)?;
    let view = drain_update(&mut backend, update)?;
    let report = view
        .render
        .export_report
        .as_ref()
        .ok_or_else(|| "FSEQ export report is not available".to_string())?;

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

fn open_project_view(path: &Path) -> Result<AppView, String> {
    let mut backend = AppBackend::new();
    let update = backend
        .open_project(path.to_path_buf())
        .map_err(format_backend_error)?;
    drain_update(&mut backend, update)
}

fn drain_update(backend: &mut AppBackend, update: AppUpdate) -> Result<AppView, String> {
    let AppUpdate { mut view, tasks } = update;
    let mut tasks = VecDeque::from(tasks);
    while let Some(task) = tasks.pop_front() {
        let output = run_task(task)?;
        let update = backend
            .complete_task(output)
            .map_err(format_backend_error)?;
        view = update.view;
        tasks.extend(update.tasks);
    }
    Ok(view)
}

fn run_task(task: BackendTask) -> Result<BackendTaskOutput, String> {
    task.run().map_err(format_backend_error)
}

fn format_backend_error(error: dawn_backend::BackendError) -> String {
    error.to_string()
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
            1 => {
                let mut matches = matches;
                Ok(matches.remove(0))
            }
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
            project_path: utf8_path(project_path)?,
        });
    }

    if path.is_file() {
        let root = path
            .parent()
            .ok_or_else(|| "project file has no parent".to_string())?;
        return Ok(ProjectInput {
            root: utf8_path(root)?,
            project_path: utf8_path(path)?,
        });
    }

    Err(format!("path was not found: {}", path.display()))
}

#[derive(Debug)]
struct ProjectInput {
    root: Utf8PathBuf,
    project_path: Utf8PathBuf,
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
