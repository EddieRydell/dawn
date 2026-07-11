use std::fs;

use camino::Utf8Path;

pub(crate) struct ProjectBoilerplateFile {
    path: &'static str,
    text: String,
}

pub(crate) fn new_project_files(project_name: &str) -> Vec<ProjectBoilerplateFile> {
    let project_id = object_key_from_name(project_name);
    vec![
        ProjectBoilerplateFile {
            path: "project.dawn",
            text: format!(
                "imports:\n- from: setups/main.setup.dawn\n  as: setups\n- from: sequences\n  as: sequences\n{project_id}:\n  type: project\n  setup: setups.main\n  sequences:\n  - sequences.main\n"
            ),
        },
        ProjectBoilerplateFile {
            path: "setups/main.setup.dawn",
            text: "imports:\n- from: ../layouts/main.layout.dawn\n  as: layouts\n- from: ../patches/main.patch.dawn\n  as: patches\nmain:\n  type: setup\n  layout: layouts.main\n  patch: patches.main\n  controllers:\n  - output_controller\noutput_controller:\n  type: controller\n  protocol: sacn\n  output:\n    channel_order: rgb\n    type: linear_rgb\n    output_count: 1\n    pixels_per_output: 1\n    first_universe: 1\n"
                .to_string(),
        },
        ProjectBoilerplateFile {
            path: "layouts/main.layout.dawn",
            text: "main:\n  type: layout\n  target_order: []\n  fixtures: []\n  groups: []\n"
                .to_string(),
        },
        ProjectBoilerplateFile {
            path: "patches/main.patch.dawn",
            text: "main:\n  type: patch\n  routes: []\n".to_string(),
        },
        ProjectBoilerplateFile {
            path: "sequences/main.sequence.dawn",
            text: sequence_boilerplate("main", 60.0, 60),
        },
    ]
}

pub(crate) fn write_new_project_files(
    root: &Utf8Path,
    files: &[ProjectBoilerplateFile],
) -> Result<(), String> {
    fs::create_dir(root).map_err(|error| error.to_string())?;
    let result = (|| {
        for file in files {
            let path = root.join(file.path);
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent).map_err(|error| error.to_string())?;
            }
            fs::write(path, &file.text).map_err(|error| error.to_string())?;
        }
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_dir_all(root);
    }
    result
}

fn sequence_boilerplate(object_key: &str, duration_seconds: f64, frame_rate: u32) -> String {
    format!(
        "{object_key}:\n  type: sequence\n  duration: {}s\n  frame_rate: {frame_rate}\n  audio: null\n  mark_collections:\n  - key: marks\n    name: Marks\n    color: '#38bdf8'\n    marks: []\n  layers:\n  - id: 0\n    name: Default\n    color: '#38bdf8'\n    enabled: true\n  effects: []\n  composition_graph:\n    nodes:\n    - id: 1\n      position:\n        x: 80.0\n        y: 80.0\n      type: layer\n      layer_id: 0\n    - id: 2\n      position:\n        x: 420.0\n        y: 80.0\n      type: output\n    edges:\n    - from: 1\n      from_port: output\n      to: 2\n      to_port: input\n  automation_clips: []\n",
        seconds_literal(duration_seconds)
    )
}

fn seconds_literal(seconds: f64) -> String {
    if seconds.fract() == 0.0 {
        format!("{seconds:.0}")
    } else {
        seconds.to_string()
    }
}

fn object_key_from_name(name: &str) -> String {
    let mut key = String::new();
    for character in name.chars() {
        if character.is_ascii_alphanumeric() || character == '_' {
            key.push(character.to_ascii_lowercase());
        } else if !key.ends_with('_') {
            key.push('_');
        }
    }
    let key = key.trim_matches('_').to_string();
    if key.is_empty() || key.as_bytes().first().is_some_and(u8::is_ascii_digit) {
        format!("project_{key}")
    } else {
        key
    }
}
