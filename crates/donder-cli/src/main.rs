use std::path::PathBuf;

use anyhow::Context;
use clap::{Parser, Subcommand};
use donder_project::{DiagnosticSeverity, ProjectError};

#[derive(Debug, Parser)]
#[command(name = "donder")]
#[command(about = "Donder source graph tooling")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    New {
        path: PathBuf,
        #[arg(long)]
        name: String,
    },
    Check {
        project: PathBuf,
    },
    RenderFrame {
        sequence_file: PathBuf,
        #[arg(long)]
        time: f64,
    },
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Command::New { path, name } => {
            donder_project::create_starter_project(&path, &name)
                .with_context(|| format!("failed to create project at {}", path.display()))?;
            println!("created {}", path.display());
            Ok(())
        }
        Command::Check { project } => match donder_project::check_project(&project) {
            Ok(compiled) => {
                print_diagnostics(&compiled.diagnostics);
                println!(
                    "ok: {} fixtures, {} groups, {} sequences",
                    compiled.show.fixtures.len(),
                    compiled.show.groups.len(),
                    compiled.show.sequences.len()
                );
                Ok(())
            }
            Err(ProjectError::Validation { diagnostics }) => {
                print_diagnostics(&diagnostics);
                anyhow::bail!("check failed")
            }
            Err(err) => Err(err).context("check failed"),
        },
        Command::RenderFrame {
            sequence_file,
            time,
        } => {
            let frame = donder_project::render_frame(&sequence_file, time)?;
            println!(
                "{}",
                serde_json::json!({
                    "pixels": frame.pixels.len() / 4,
                    "bytes": frame.pixels.len(),
                    "fixtureSpans": frame.fixture_spans.len(),
                    "warnings": frame.warnings,
                })
            );
            Ok(())
        }
    }
}

fn print_diagnostics(diagnostics: &[donder_project::Diagnostic]) {
    for diagnostic in diagnostics {
        let severity = match diagnostic.severity {
            DiagnosticSeverity::Error => "error",
            DiagnosticSeverity::Warning => "warning",
        };
        println!(
            "{severity}: {}: {}",
            diagnostic.path.display(),
            diagnostic.message
        );
    }
}
