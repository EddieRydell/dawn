mod common;

use camino::{Utf8Path, Utf8PathBuf};
use dawn_language::values::DawnDuration;
use dawn_project_io::{export_project, load_package, save_project};
use std::fs;
use std::time::Duration;

use common::{load_project_package, write_project_package};

#[test]
fn audio_reference_cannot_escape_its_module() {
    let temp = tempfile::tempdir().unwrap();
    let temp_root = Utf8PathBuf::from_path_buf(temp.path().to_path_buf()).unwrap();
    let project_root = temp_root.join("project");
    fs::create_dir(&project_root).unwrap();
    fs::write(temp_root.join("external.wav"), b"audio").unwrap();
    fs::write(
        project_root.join("project.dawn"),
        "imports:\n- from:\n    documents:\n    - setup.dawn\n  as: setups\n- from:\n    documents:\n    - sequence.dawn\n  as: sequences\nmain:\n  type: project\n  setup: setups.main\n  sequences: [sequences.main]\n",
    )
    .unwrap();
    fs::write(
        project_root.join("setup.dawn"),
        "imports:\n- from:\n    documents:\n    - display.dawn\n  as: display\n- from:\n    documents:\n    - patch.dawn\n  as: patches\nmain:\n  type: setup\n  elements: display.elements\n  preview: display.preview\n  patch: patches.main\n  controllers: []\n",
    )
    .unwrap();
    fs::write(
        project_root.join("display.dawn"),
        "elements:\n  type: element_tree\n  roots: [1]\n  nodes:\n  - id: 1\n    name: Pixel\n    type: color\n    cells: 1\n    capability: { type: rgb }\npreview:\n  type: preview_layout\n  element_tree: elements\n  props: []\n",
    )
    .unwrap();
    fs::write(
        project_root.join("patch.dawn"),
        "main:\n  type: patch\n  nodes: []\n  edges: []\n",
    )
    .unwrap();
    fs::write(
        project_root.join("sequence.dawn"),
        "main:\n  type: sequence\n  duration: 1s\n  frame_rate: 30\n  audio: ../external.wav\n  mark_collections: []\n  layers: []\n  effects: []\n  composition_graph:\n    nodes:\n    - id: 1\n      position: { x: 0, y: 0 }\n      type: output\n    edges: []\n  automation_clips: []\n",
    )
    .unwrap();
    write_project_package(&project_root);

    assert!(load_package(&project_root).is_err());
}

#[test]
fn same_named_definitions_in_different_documents_keep_distinct_identities() {
    let workspace_root = Utf8Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Utf8Path::parent)
        .unwrap();
    let starter = load_project_package(&workspace_root.join("examples/starter"));
    let temp = tempfile::tempdir().unwrap();
    let root = Utf8PathBuf::from_path_buf(temp.path().to_path_buf()).unwrap();
    export_project(&starter, &root).unwrap();
    write_project_package(&root);

    fs::create_dir_all(root.join("identity-a")).unwrap();
    fs::create_dir_all(root.join("identity-b")).unwrap();
    fs::write(
        root.join("identity-a/shared.effect.dawn"),
        "effect Shared { color sample() { return #ff0000; } }",
    )
    .unwrap();
    fs::write(
        root.join("identity-b/shared.effect.dawn"),
        "effect Shared { color sample() { return #0000ff; } }",
    )
    .unwrap();
    let entrypoint = root.join("project.dawn");
    let project_text = fs::read_to_string(&entrypoint).unwrap();
    fs::write(
        &entrypoint,
        format!(
            "imports:\n- from:\n    documents:\n    - identity-a/shared.effect.dawn\n  as: identity-a\n- from:\n    documents:\n    - identity-b/shared.effect.dawn\n  as: identity-b\n{}",
            project_text.strip_prefix("imports:\n").unwrap()
        ),
    )
    .unwrap();

    let loaded = load_project_package(&root);
    let identities = loaded
        .project
        .definitions
        .effects
        .definitions
        .keys()
        .filter(|id| id.0.object() == "Shared")
        .map(|id| id.0.document().to_path_buf())
        .collect::<std::collections::BTreeSet<_>>();
    assert_eq!(
        identities,
        [
            Utf8PathBuf::from("identity-a/shared.effect.dawn"),
            Utf8PathBuf::from("identity-b/shared.effect.dawn"),
        ]
        .into_iter()
        .collect()
    );
}

#[test]
fn typed_sequence_insertion_roundtrips_nested_paths() {
    let workspace_root = Utf8Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Utf8Path::parent)
        .unwrap();
    let starter = load_project_package(&workspace_root.join("examples/starter"));
    let temp = tempfile::tempdir().unwrap();
    let root = Utf8PathBuf::from_path_buf(temp.path().to_path_buf()).unwrap();
    export_project(&starter, &root).unwrap();
    write_project_package(&root);
    let mut session = load_project_package(&root);

    let id = dawn_project_io::insert_sequence(
        &mut session,
        "sequences/nested/new.sequence.dawn".into(),
        "nested_sequence".to_string(),
        DawnDuration(Duration::from_secs(30)),
        60,
    )
    .unwrap();
    save_project(&session).unwrap();

    let reloaded = load_project_package(&root);
    assert!(reloaded.project.sequences.contains_key(&id));
    assert!(reloaded.project.root.sequences.contains(&id));
    assert!(root.join(id.0.document()).is_file());
}

#[test]
fn local_document_import_cannot_escape_module() {
    let workspace_root = Utf8Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Utf8Path::parent)
        .unwrap();
    let starter = load_project_package(&workspace_root.join("examples/starter"));
    let temp = tempfile::tempdir().unwrap();
    let temp_root = Utf8PathBuf::from_path_buf(temp.path().to_path_buf()).unwrap();
    let root = temp_root.join("project");
    export_project(&starter, &root).unwrap();
    write_project_package(&root);
    fs::write(
        temp_root.join("dependency.effect.dawn"),
        "effect Dependency { color sample() { return #ffffff; } }",
    )
    .unwrap();
    let entrypoint = root.join("project.dawn");
    let project_text = fs::read_to_string(&entrypoint).unwrap();
    fs::write(
        &entrypoint,
        format!(
            "imports:\n- from:\n    documents:\n    - ../dependency.effect.dawn\n  as: dependency\n{}",
            project_text.strip_prefix("imports:\n").unwrap()
        ),
    )
    .unwrap();

    assert!(load_package(&root).is_err());
}

#[test]
fn fixture_behavior_rules_roundtrip() {
    let temp = tempfile::tempdir().unwrap();
    let root = Utf8PathBuf::from_path_buf(temp.path().to_path_buf()).unwrap();
    write_fixture_profile_project(&root, "behavior_rules");

    let loaded = load_project_package(&root);
    assert_eq!(
        loaded
            .project
            .definitions
            .fixture_profiles
            .definitions
            .values()
            .next()
            .unwrap()
            .behavior_rules
            .len(),
        1
    );
    save_project(&loaded).unwrap();

    let saved_profile = fs::read_to_string(root.join("profile.dawn")).unwrap();
    assert!(saved_profile.contains("behavior_rules:"));
    let reloaded = load_project_package(&root);
    assert_eq!(loaded.project, reloaded.project);
}

#[test]
fn legacy_fixture_rule_field_is_rejected() {
    let temp = tempfile::tempdir().unwrap();
    let root = Utf8PathBuf::from_path_buf(temp.path().to_path_buf()).unwrap();
    let legacy_field = ["auto", "mation"].concat();
    write_fixture_profile_project(&root, &legacy_field);

    assert!(load_package(&root).is_err());
}

fn write_fixture_profile_project(root: &Utf8Path, rule_field: &str) {
    fs::write(
        root.join("project.dawn"),
        "imports:\n- from:\n    documents:\n    - setup.dawn\n  as: setups\n- from:\n    documents:\n    - sequence.dawn\n  as: sequences\nmain:\n  type: project\n  setup: setups.main\n  sequences: [sequences.main]\n",
    )
    .unwrap();
    fs::write(
        root.join("setup.dawn"),
        "imports:\n- from:\n    documents:\n    - layout.dawn\n  as: layouts\n- from:\n    documents:\n    - patch.dawn\n  as: patches\nmain:\n  type: setup\n  elements: layouts.elements\n  preview: layouts.preview\n  patch: patches.main\n  controllers: []\n",
    )
    .unwrap();
    fs::write(
        root.join("layout.dawn"),
        "imports:\n- from:\n    documents:\n    - profile.dawn\n  as: profiles\nelements:\n  type: element_tree\n  roots: [1]\n  nodes:\n  - id: 1\n    name: Fixture\n    type: fixture\n    profile: profiles.basic\npreview:\n  type: preview_layout\n  element_tree: elements\n  props: []\n",
    )
    .unwrap();
    fs::write(
        root.join("patch.dawn"),
        "main:\n  type: patch\n  nodes: []\n  edges: []\n",
    )
    .unwrap();
    fs::write(
        root.join("sequence.dawn"),
        "main:\n  type: sequence\n  duration: 1s\n  frame_rate: 30\n  audio: null\n  mark_collections: []\n  layers: []\n  effects: []\n  composition_graph:\n    nodes:\n    - id: 1\n      position: { x: 0, y: 0 }\n      type: output\n    edges: []\n  automation_clips: []\n",
    )
    .unwrap();
    fs::write(
        root.join("profile.dawn"),
        format!(
            "basic:\n  type: fixture_profile\n  functions:\n  - id: 1\n    name: Dimmer\n    type: range\n    curve: {{ type: linear }}\n  channels:\n  - slot: 0\n    role: coarse\n    function: 1\n    curve: {{ type: linear }}\n  {rule_field}:\n  - type: dimmer\n    function: 1\n    off: 0.0\n    on: 1.0\n"
        ),
    )
    .unwrap();
    write_project_package(root);
}
