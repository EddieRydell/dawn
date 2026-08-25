use std::collections::BTreeMap;
use std::fs;

use camino::Utf8Path;
use dawn_package::{ExportGroup, Lockfile, PackageManifest, ProjectManifest, canonical_json};
use semver::VersionReq;
use uuid::Uuid;

pub(crate) struct ProjectBoilerplateFile {
    path: &'static str,
    text: String,
}

pub(crate) fn new_project_files(project_name: &str) -> Result<Vec<ProjectBoilerplateFile>, String> {
    let project_id = object_key_from_name(project_name);
    let manifest = PackageManifest {
        manifest_version: dawn_package::MANIFEST_VERSION,
        module_id: Uuid::new_v4(),
        language_version: "0.1".to_string(),
        requires_dawn: VersionReq::parse(">=0.1.0, <1.0.0").map_err(|error| error.to_string())?,
        project: Some(ProjectManifest {
            entrypoint: "project.dawn".to_string(),
        }),
        publication: None,
        exports: BTreeMap::from([(
            "project".to_string(),
            ExportGroup {
                documents: vec!["project.dawn".to_string()],
            },
        )]),
        dependencies: BTreeMap::new(),
        assets: BTreeMap::new(),
    };
    let lockfile =
        Lockfile::new(&manifest, "https://registry.dawn.dev").map_err(|error| error.to_string())?;
    let manifest_text =
        String::from_utf8(canonical_json(&manifest).map_err(|error| error.to_string())?)
            .map_err(|error| error.to_string())?;
    let lockfile_text =
        String::from_utf8(canonical_json(&lockfile).map_err(|error| error.to_string())?)
            .map_err(|error| error.to_string())?;

    Ok(vec![
        ProjectBoilerplateFile {
            path: dawn_package::MANIFEST_FILE,
            text: manifest_text,
        },
        ProjectBoilerplateFile {
            path: dawn_package::LOCK_FILE,
            text: lockfile_text,
        },
        ProjectBoilerplateFile {
            path: "project.dawn",
            text: format!(
                "imports:\n- from:\n    documents:\n    - setups/main.setup.dawn\n  as: setups\n- from:\n    documents:\n    - sequences/main.sequence.dawn\n  as: sequences\n{project_id}:\n  type: project\n  setup: setups.main\n  sequences:\n  - sequences.main\n"
            ),
        },
        ProjectBoilerplateFile {
            path: "setups/main.setup.dawn",
            text: "imports:\n- from:\n    documents:\n    - display/main.display.dawn\n  as: display\n- from:\n    documents:\n    - patches/main.patch.dawn\n  as: patches\nmain:\n  type: setup\n  elements: display.elements\n  preview: display.preview\n  patch: patches.main\n  controllers:\n  - output_controller\noutput_controller:\n  type: controller\n  protocol:\n    type: e131\n    source_name: Dawn\n    bind_address: 0.0.0.0\n    priority: 100\n    mode: multicast\n  ports:\n  - id: 1\n    universe: 1\n    slot_count: 3\n"
                .to_string(),
        },
        ProjectBoilerplateFile {
            path: "display/pixel.prop.dawn",
            text: "pixel:\n  type: prop\n  bulb_diameter: 0.08\n  geometry:\n    type: points\n    points:\n    - x: 0.0\n      y: 0.0\n      z: 0.0\n"
                .to_string(),
        },
        ProjectBoilerplateFile {
            path: "display/main.display.dawn",
            text: "imports:\n- from:\n    documents:\n    - display/pixel.prop.dawn\n  as: props\nelements:\n  type: element_tree\n  roots: [1]\n  nodes:\n  - id: 1\n    name: Pixel\n    type: color\n    cells: 1\n    capability:\n      type: rgb\npreview:\n  type: preview_layout\n  element_tree: elements\n  props:\n  - id: 1\n    name: Pixel\n    prop: props.pixel\n    transform:\n      position: { x: 0.0, y: 0.0, z: 0.0 }\n      rotation: { x: 0.0, y: 0.0, z: 0.0 }\n      scale: { x: 1.0, y: 1.0, z: 1.0 }\n    bindings:\n    - { node: 1, cell: 0 }\n"
                .to_string(),
        },
        ProjectBoilerplateFile {
            path: "patches/main.patch.dawn",
            text: "imports:\n- from:\n    documents:\n    - display/main.display.dawn\n  as: display\n- from:\n    documents:\n    - setups/main.setup.dawn\n  as: setups\nmain:\n  type: patch\n  nodes:\n  - id: 1\n    type: source\n    selection: { tree: display.elements, node: 1 }\n    output: color\n    width: 1\n  - id: 2\n    type: filter\n    filter: color_breakdown\n    capability: { type: rgb }\n    cell_count: 1\n  - id: 3\n    type: filter\n    filter: quantize_8\n    width: 3\n  - id: 4\n    type: sink\n    controller: setups.output_controller\n    port: 1\n    start_slot: 0\n    slot_count: 3\n  edges:\n  - { from: 1, from_port: 0, to: 2, to_port: 0 }\n  - { from: 2, from_port: 0, to: 3, to_port: 0 }\n  - { from: 3, from_port: 0, to: 4, to_port: 0 }\n".to_string(),
        },
        ProjectBoilerplateFile {
            path: "sequences/main.sequence.dawn",
            text: sequence_boilerplate("main", 60.0, 60),
        },
    ])
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
        "{object_key}:\n  type: sequence\n  duration: {}s\n  frame_rate: {frame_rate}\n  audio: null\n  mark_collections:\n  - key: marks\n    name: Marks\n    color: '#38bdf8'\n    marks: []\n  layers:\n  - id: 0\n    name: Default\n    color: '#38bdf8'\n    enabled: true\n  effects: []\n  composition_graph:\n    nodes:\n    - id: 1\n      position:\n        x: 80.0\n        y: 80.0\n      type: layer\n      layer_id: 0\n    - id: 2\n      position:\n        x: 420.0\n        y: 80.0\n      type: output\n    edges:\n    - from: 1\n      from_port: output\n      to: 2\n      to_port: input\n  automation_clips: []\n  control_clips: []\n",
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

#[cfg(test)]
mod tests {
    use std::time::{SystemTime, UNIX_EPOCH};

    use camino::Utf8PathBuf;

    use super::*;

    #[test]
    fn new_project_template_loads_as_complete_output_project() {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root =
            Utf8PathBuf::from_path_buf(std::env::temp_dir().join(format!("dawn-template-{nonce}")))
                .unwrap();
        let files = new_project_files("Template Test").unwrap();
        write_new_project_files(&root, &files).unwrap();
        let session = dawn_project_io::load_package(&root).unwrap().session;
        let setup = session
            .project
            .setups
            .get(&session.project.root.setup)
            .unwrap();
        assert!(session.project.element_trees.contains_key(&setup.elements));
        assert!(session.project.preview_layouts.contains_key(&setup.preview));
        assert!(session.project.patches.contains_key(&setup.patch));
        assert_eq!(setup.controllers.len(), 1);
        fs::remove_dir_all(&root).unwrap();
    }
}
