use dawn_language::effect_dsl::compile_effects;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};

fn main() {
    let root = env::args()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("examples"));

    let mut files = Vec::new();
    collect_effect_files(&root, &mut files);
    files.sort();

    let mut failed = false;
    for file in files {
        let source = match fs::read_to_string(&file) {
            Ok(source) => source,
            Err(error) => {
                eprintln!("{}: failed to read: {error}", file.display());
                failed = true;
                continue;
            }
        };

        if let Err(diagnostics) = compile_effects(&source) {
            failed = true;
            eprintln!("{}:", file.display());
            for diagnostic in diagnostics {
                eprintln!(
                    "  {}..{} {}",
                    diagnostic.span.start, diagnostic.span.end, diagnostic.message
                );
            }
        }
    }

    if failed {
        std::process::exit(1);
    }
}

fn collect_effect_files(path: &Path, files: &mut Vec<PathBuf>) {
    if path.is_file() {
        if path
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.ends_with(".effect.dawn"))
        {
            files.push(path.to_path_buf());
        }
        return;
    }

    let Ok(entries) = fs::read_dir(path) else {
        return;
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_effect_files(&path, files);
        } else if path
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.ends_with(".effect.dawn"))
        {
            files.push(path);
        }
    }
}
