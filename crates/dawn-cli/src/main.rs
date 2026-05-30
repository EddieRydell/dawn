use std::path::{Path, PathBuf};
use std::process::ExitCode;

use camino::Utf8PathBuf;
use clap::{Parser, Subcommand};
use dawn_project::analysis::{
    analyze_project_with_overlays, DiagnosticCode, DiagnosticSeverity, ProjectAnalysis,
    ProjectDiagnostic, TextRange,
};
use dawn_project::fs::WorkspaceFs;
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
