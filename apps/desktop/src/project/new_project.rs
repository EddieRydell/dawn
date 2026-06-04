use std::fs;
use std::path::{Component, Path, PathBuf};

pub(crate) const STARTER_SEQUENCE_PATH: &str = "sequences/example.sequence.dawn";

pub(crate) fn create_starter_project(
    parent_path: &str,
    directory_name: &str,
) -> Result<PathBuf, String> {
    validate_directory_name(directory_name)?;
    let parent = PathBuf::from(parent_path);
    if !parent.is_dir() {
        return Err("parent location is not a directory".to_string());
    }

    let target = parent.join(directory_name);
    validate_target_directory(&target)?;

    fs::create_dir_all(&target)
        .map_err(|error| format!("failed to create project directory: {error}"))?;
    for directory in STARTER_DIRECTORIES {
        fs::create_dir(target.join(directory))
            .map_err(|error| format!("failed to create {directory}: {error}"))?;
    }

    let project_name = display_name(directory_name);
    let project_key = snake_case_key(directory_name);
    let files = starter_files(&project_name, &project_key);
    for file in files {
        let path = target.join(file.path);
        if path.exists() {
            return Err(format!("starter file already exists: {}", file.path));
        }
        fs::write(&path, file.content)
            .map_err(|error| format!("failed to write {}: {error}", file.path))?;
    }

    Ok(target)
}

fn validate_directory_name(name: &str) -> Result<(), String> {
    if name.is_empty() {
        return Err("project folder name is required".to_string());
    }
    if name.trim() != name {
        return Err("project folder name cannot start or end with whitespace".to_string());
    }
    if name == "." || name == ".." {
        return Err("project folder name cannot be . or ..".to_string());
    }
    if name.contains('/') || name.contains('\\') {
        return Err("project folder name must be a folder name, not a path".to_string());
    }
    if Path::new(name)
        .components()
        .any(|component| !matches!(component, Component::Normal(_)))
    {
        return Err("project folder name must be a plain folder name".to_string());
    }
    Ok(())
}

fn validate_target_directory(target: &Path) -> Result<(), String> {
    if target.exists() {
        if !target.is_dir() {
            return Err("target path already exists and is not a directory".to_string());
        }
        let mut entries = fs::read_dir(target)
            .map_err(|error| format!("failed to inspect target directory: {error}"))?;
        if entries
            .next()
            .transpose()
            .map_err(|error| format!("failed to inspect target directory contents: {error}"))?
            .is_some()
        {
            return Err("target directory must be empty".to_string());
        }
    }
    Ok(())
}

fn display_name(directory_name: &str) -> String {
    directory_name
        .split(|character: char| !character.is_alphanumeric())
        .filter(|part| !part.is_empty())
        .map(|part| {
            let mut characters = part.chars();
            let Some(first) = characters.next() else {
                return String::new();
            };
            let mut word = first.to_uppercase().collect::<String>();
            word.push_str(&characters.as_str().to_lowercase());
            word
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn snake_case_key(directory_name: &str) -> String {
    let mut key = String::new();
    let mut previous_was_separator = true;
    for character in directory_name.chars() {
        if character.is_ascii_alphanumeric() {
            key.push(character.to_ascii_lowercase());
            previous_was_separator = false;
        } else if !previous_was_separator {
            key.push('_');
            previous_was_separator = true;
        }
    }
    while key.ends_with('_') {
        key.pop();
    }
    if key.is_empty()
        || key
            .chars()
            .next()
            .is_some_and(|character| character.is_ascii_digit())
    {
        key.insert_str(0, "project_");
    }
    key
}

struct StarterFile {
    path: &'static str,
    content: String,
}

const STARTER_DIRECTORIES: &[&str] = &[
    "audio",
    "curves",
    "displays",
    "effects",
    "fixtures",
    "layouts",
    "patches",
    "sequences",
];

fn starter_files(project_name: &str, project_key: &str) -> Vec<StarterFile> {
    vec![
        StarterFile {
            path: "project.dawn",
            content: project_file(project_name, project_key),
        },
        StarterFile {
            path: "displays/main.display.dawn",
            content: display_file(project_name),
        },
        StarterFile {
            path: "layouts/main.layout.dawn",
            content: LAYOUT_FILE.to_string(),
        },
        StarterFile {
            path: "fixtures/basic.fixture.dawn",
            content: FIXTURE_FILE.to_string(),
        },
        StarterFile {
            path: "patches/main.patch.dawn",
            content: PATCH_FILE.to_string(),
        },
        StarterFile {
            path: "curves/basic.curve.dawn",
            content: CURVE_FILE.to_string(),
        },
        StarterFile {
            path: "effects/pulse.effect.dawn",
            content: PULSE_EFFECT.to_string(),
        },
        StarterFile {
            path: "effects/chase.effect.dawn",
            content: CHASE_EFFECT.to_string(),
        },
        StarterFile {
            path: "effects/twinkle.effect.dawn",
            content: TWINKLE_EFFECT.to_string(),
        },
        StarterFile {
            path: "effects/wipe.effect.dawn",
            content: WIPE_EFFECT.to_string(),
        },
        StarterFile {
            path: "effects/mark-pulse.effect.dawn",
            content: MARK_PULSE_EFFECT.to_string(),
        },
        StarterFile {
            path: STARTER_SEQUENCE_PATH,
            content: SEQUENCE_FILE.to_string(),
        },
        StarterFile {
            path: "AGENTS.md",
            content: AGENTS_FILE.to_string(),
        },
        StarterFile {
            path: "CLAUDE.md",
            content: CLAUDE_FILE.to_string(),
        },
    ]
}

fn project_file(project_name: &str, project_key: &str) -> String {
    PROJECT_FILE
        .replace("\nstarter:\n", &format!("\n{project_key}:\n"))
        .replace("  name: Starter\n", &format!("  name: {project_name}\n"))
}

fn display_file(project_name: &str) -> String {
    DISPLAY_FILE.replace("  name: Starter\n", &format!("  name: {project_name}\n"))
}

const PROJECT_FILE: &str = include_str!("../../../../examples/starter/project.dawn");
const DISPLAY_FILE: &str = include_str!("../../../../examples/starter/displays/main.display.dawn");
const LAYOUT_FILE: &str = include_str!("../../../../examples/starter/layouts/main.layout.dawn");
const FIXTURE_FILE: &str = include_str!("../../../../examples/starter/fixtures/basic.fixture.dawn");
const PATCH_FILE: &str = include_str!("../../../../examples/starter/patches/main.patch.dawn");
const CURVE_FILE: &str = include_str!("../../../../examples/starter/curves/basic.curve.dawn");
const PULSE_EFFECT: &str = include_str!("../../../../examples/starter/effects/pulse.effect.dawn");
const CHASE_EFFECT: &str = include_str!("../../../../examples/starter/effects/chase.effect.dawn");
const TWINKLE_EFFECT: &str =
    include_str!("../../../../examples/starter/effects/twinkle.effect.dawn");
const WIPE_EFFECT: &str = include_str!("../../../../examples/starter/effects/wipe.effect.dawn");
const MARK_PULSE_EFFECT: &str =
    include_str!("../../../../examples/starter/effects/mark-pulse.effect.dawn");
const SEQUENCE_FILE: &str =
    include_str!("../../../../examples/starter/sequences/example.sequence.dawn");
const AGENTS_FILE: &str = include_str!("../../../../examples/starter/AGENTS.md");
const CLAUDE_FILE: &str = include_str!("../../../../examples/starter/CLAUDE.md");
