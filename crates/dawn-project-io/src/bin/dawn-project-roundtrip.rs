use camino::Utf8PathBuf;
use dawn_project_io::{export_project, load_project};
use std::env;
use std::process::ExitCode;

fn main() -> ExitCode {
    let mut args = env::args().skip(1);
    let Some(input) = args.next() else {
        eprintln!("usage: dawn-project-roundtrip <project.dawn> <output-root>");
        return ExitCode::FAILURE;
    };
    let Some(output) = args.next() else {
        eprintln!("usage: dawn-project-roundtrip <project.dawn> <output-root>");
        return ExitCode::FAILURE;
    };

    let input = Utf8PathBuf::from(input);
    let output = Utf8PathBuf::from(output);

    let original = match load_project(&input) {
        Ok(session) => session,
        Err(error) => {
            eprintln!("load failed: {error}");
            return ExitCode::FAILURE;
        }
    };
    let report = match export_project(&original, &output) {
        Ok(report) => report,
        Err(error) => {
            eprintln!("export failed: {error}");
            return ExitCode::FAILURE;
        }
    };
    let exported_entrypoint = output.join(&original.source.entrypoint);
    let exported = match load_project(&exported_entrypoint) {
        Ok(session) => session,
        Err(error) => {
            eprintln!("reload failed: {error}");
            return ExitCode::FAILURE;
        }
    };

    println!("written dawn/effect files: {}", report.written_files.len());
    println!("copied assets: {}", report.copied_assets.len());
    println!(
        "domain graph equal after reload: {}",
        original.project == exported.project
    );
    println!(
        "source document count: {} -> {}",
        original.source.documents.len(),
        exported.source.documents.len()
    );
    ExitCode::SUCCESS
}
