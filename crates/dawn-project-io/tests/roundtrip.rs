use camino::{Utf8Path, Utf8PathBuf};
use dawn_language::identity::SourceIdentity;
use dawn_language::operator::{OperatorDefinitionId, OperatorRef};
use dawn_language::sequence::CompositionGraphNodeKind;
use dawn_language::values::DawnDuration;
use dawn_project_io::{
    SourceDocumentKind, SourceObjectId, SourceObjectKind, export_project, load_project,
    save_project, source_file_list,
};
use std::fs;
use std::time::Duration;

#[test]
fn example_projects_roundtrip() {
    for entrypoint in example_entrypoints() {
        let original = load_project(&entrypoint).unwrap_or_else(|error| {
            panic!("failed to load {entrypoint}: {error}");
        });
        let temp = tempfile::tempdir().unwrap();
        let output = Utf8PathBuf::from_path_buf(temp.path().to_path_buf()).unwrap();
        export_project(&original, &output).unwrap_or_else(|error| {
            panic!("failed to export {entrypoint}: {error}");
        });
        let exported_entrypoint = output.join(&original.source.entrypoint);
        let exported = load_project(&exported_entrypoint).unwrap_or_else(|error| {
            panic!("failed to reload {exported_entrypoint}: {error}");
        });
        let save_report = save_project(&exported).unwrap_or_else(|error| {
            panic!("failed to save {exported_entrypoint}: {error}");
        });
        assert_eq!(
            exported.source.documents.len(),
            save_report.written_files.len(),
            "{entrypoint}"
        );
        let saved = load_project(&exported_entrypoint).unwrap_or_else(|error| {
            panic!("failed to reload saved {exported_entrypoint}: {error}");
        });

        assert_eq!(original.project, exported.project, "{entrypoint}");
        assert_eq!(exported.project, saved.project, "{entrypoint}");
        assert_eq!(
            source_file_list(&original),
            source_file_list(&exported),
            "{entrypoint}"
        );
        for (path, original_document) in &original.source.documents {
            let exported_document = &exported.source.documents[path];
            assert_eq!(
                original_document.objects(),
                exported_document.objects(),
                "{path}"
            );
            assert_eq!(
                original_document.imports(),
                exported_document.imports(),
                "{path}"
            );
        }

        for (path, document) in &original.source.documents {
            if matches!(
                document.kind(),
                SourceDocumentKind::Effect { .. } | SourceDocumentKind::Operator { .. }
            ) {
                let original_bytes = fs::read(original.source.source_root.join(path)).unwrap();
                let exported_bytes = fs::read(output.join(path)).unwrap();
                assert_eq!(original_bytes, exported_bytes, "{path}");
            }
        }

        if entrypoint.ends_with(Utf8Path::new("examples/starter/project.dawn")) {
            assert!(
                original
                    .project
                    .definitions
                    .operators
                    .get(&OperatorDefinitionId(SourceIdentity::new(
                        "operators/gain.operator.dawn".into(),
                        "Gain".to_string(),
                    )))
                    .is_some()
            );
            assert!(
                original
                    .source
                    .documents
                    .get(Utf8Path::new("operators/gain.operator.dawn"))
                    .is_some_and(|document| document.objects().contains(
                        &SourceObjectId::new(
                            SourceObjectKind::OperatorDefinition,
                            "Gain".to_string(),
                        )
                        .unwrap()
                    ))
            );
            let layer_test = original
                .project
                .sequences
                .values()
                .find(|sequence| sequence.id.0.object() == "layer_test")
                .unwrap();
            assert!(layer_test.composition_graph.nodes.iter().any(|node| {
                matches!(
                    &node.kind,
                    CompositionGraphNodeKind::Operator(operator)
                        if operator.operator
                            == OperatorRef::Custom(OperatorDefinitionId(SourceIdentity::new(
                                "operators/gain.operator.dawn".into(),
                                "Gain".to_string(),
                            )))
                )
            }));
            assert!(
                !original.source.referenced_assets.is_empty(),
                "starter should reference audio"
            );
            for asset in &original.source.referenced_assets {
                assert!(
                    output.join(&asset.relative_path).is_file(),
                    "{}",
                    asset.relative_path
                );
            }
        }
    }
}

#[test]
fn external_audio_does_not_expand_project_ownership_and_exports_inside_destination() {
    let temp = tempfile::tempdir().unwrap();
    let temp_root = Utf8PathBuf::from_path_buf(temp.path().to_path_buf()).unwrap();
    let project_root = temp_root.join("project");
    fs::create_dir(&project_root).unwrap();
    fs::write(temp_root.join("external.wav"), b"audio").unwrap();
    fs::write(
        project_root.join("project.dawn"),
        "imports:\n- from: setup.dawn\n  as: setups\n- from: sequence.dawn\n  as: sequences\nmain:\n  type: project\n  setup: setups.main\n  sequences: [sequences.main]\n",
    )
    .unwrap();
    fs::write(
        project_root.join("setup.dawn"),
        "imports:\n- from: display.dawn\n  as: display\n- from: patch.dawn\n  as: patches\nmain:\n  type: setup\n  elements: display.elements\n  preview: display.preview\n  patch: patches.main\n  controllers: []\n",
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

    let session = load_project(&project_root.join("project.dawn")).unwrap();
    assert_eq!(
        session.source.source_root,
        project_root.canonicalize_utf8().unwrap()
    );
    assert_eq!(
        session.source.referenced_assets[0].relative_path,
        Utf8Path::new("../external.wav")
    );

    let export_root = temp_root.join("export");
    let report = export_project(&session, &export_root).unwrap();
    assert_eq!(
        report.copied_assets,
        vec![Utf8PathBuf::from("assets/1/external.wav")]
    );
    assert!(export_root.join("assets/1/external.wav").is_file());
    load_project(&export_root.join("project.dawn")).unwrap();
}

#[test]
fn same_named_definitions_in_different_documents_keep_distinct_identities() {
    let workspace_root = Utf8Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Utf8Path::parent)
        .unwrap();
    let starter = load_project(&workspace_root.join("examples/starter/project.dawn")).unwrap();
    let temp = tempfile::tempdir().unwrap();
    let root = Utf8PathBuf::from_path_buf(temp.path().to_path_buf()).unwrap();
    export_project(&starter, &root).unwrap();

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
            "imports:\n- from: identity-a/shared.effect.dawn\n  as: identity_a\n- from: identity-b/shared.effect.dawn\n  as: identity_b\n{}",
            project_text.strip_prefix("imports:\n").unwrap()
        ),
    )
    .unwrap();

    let loaded = load_project(&entrypoint).unwrap();
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
    let starter = load_project(&workspace_root.join("examples/starter/project.dawn")).unwrap();
    let temp = tempfile::tempdir().unwrap();
    let root = Utf8PathBuf::from_path_buf(temp.path().to_path_buf()).unwrap();
    export_project(&starter, &root).unwrap();
    let entrypoint = root.join("project.dawn");
    let mut session = load_project(&entrypoint).unwrap();

    let id = dawn_project_io::insert_sequence(
        &mut session,
        "sequences/nested/new.sequence.dawn".into(),
        "nested_sequence".to_string(),
        DawnDuration(Duration::from_secs(30)),
        60,
    )
    .unwrap();
    save_project(&session).unwrap();

    let reloaded = load_project(&entrypoint).unwrap();
    assert!(reloaded.project.sequences.contains_key(&id));
    assert!(reloaded.project.root.sequences.contains(&id));
    assert!(root.join(id.0.document()).is_file());
}

#[test]
fn external_source_documents_remain_dependencies_and_cannot_escape_export() {
    let workspace_root = Utf8Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Utf8Path::parent)
        .unwrap();
    let starter = load_project(&workspace_root.join("examples/starter/project.dawn")).unwrap();
    let temp = tempfile::tempdir().unwrap();
    let temp_root = Utf8PathBuf::from_path_buf(temp.path().to_path_buf()).unwrap();
    let root = temp_root.join("project");
    export_project(&starter, &root).unwrap();
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
            "imports:\n- from: ../dependency.effect.dawn\n  as: dependency\n{}",
            project_text.strip_prefix("imports:\n").unwrap()
        ),
    )
    .unwrap();

    let loaded = load_project(&entrypoint).unwrap();
    let dependency_path = Utf8Path::new("../dependency.effect.dawn");
    assert!(loaded.source.documents.contains_key(dependency_path));
    assert!(!dawn_project_io::is_project_owned_path(dependency_path));
    assert!(export_project(&loaded, &temp_root.join("export")).is_err());
}

#[test]
fn fixture_behavior_rules_roundtrip() {
    let temp = tempfile::tempdir().unwrap();
    let root = Utf8PathBuf::from_path_buf(temp.path().to_path_buf()).unwrap();
    let entrypoint = write_fixture_profile_project(&root, "behavior_rules");

    let loaded = load_project(&entrypoint).unwrap();
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
    let reloaded = load_project(&entrypoint).unwrap();
    assert_eq!(loaded.project, reloaded.project);
}

#[test]
fn legacy_fixture_rule_field_is_rejected() {
    let temp = tempfile::tempdir().unwrap();
    let root = Utf8PathBuf::from_path_buf(temp.path().to_path_buf()).unwrap();
    let legacy_field = ["auto", "mation"].concat();
    let entrypoint = write_fixture_profile_project(&root, &legacy_field);

    assert!(load_project(&entrypoint).is_err());
}

fn write_fixture_profile_project(root: &Utf8Path, rule_field: &str) -> Utf8PathBuf {
    fs::write(
        root.join("project.dawn"),
        "imports:\n- from: setup.dawn\n  as: setups\n- from: sequence.dawn\n  as: sequences\nmain:\n  type: project\n  setup: setups.main\n  sequences: [sequences.main]\n",
    )
    .unwrap();
    fs::write(
        root.join("setup.dawn"),
        "imports:\n- from: layout.dawn\n  as: layouts\n- from: patch.dawn\n  as: patches\nmain:\n  type: setup\n  elements: layouts.elements\n  preview: layouts.preview\n  patch: patches.main\n  controllers: []\n",
    )
    .unwrap();
    fs::write(
        root.join("layout.dawn"),
        "imports:\n- from: profile.dawn\n  as: profiles\nelements:\n  type: element_tree\n  roots: [1]\n  nodes:\n  - id: 1\n    name: Fixture\n    type: fixture\n    profile: profiles.basic\npreview:\n  type: preview_layout\n  element_tree: elements\n  props: []\n",
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
    root.join("project.dawn")
}

fn example_entrypoints() -> Vec<Utf8PathBuf> {
    let workspace_root = Utf8Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Utf8Path::parent)
        .unwrap();
    vec![workspace_root.join("examples/starter/project.dawn")]
}
