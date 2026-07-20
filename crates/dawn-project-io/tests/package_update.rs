use std::collections::BTreeMap;
use std::fs;

use camino::{Utf8Path, Utf8PathBuf};
use dawn_language::operator::{OperatorRef, validate_composition_graph};
use dawn_language::sequence::CompositionGraphNodeKind;
use dawn_package::{CacheStore, Dependency, ExportGroup, Lockfile, PackageManifest};
use semver::VersionReq;
use tempfile::tempdir;
use uuid::Uuid;

fn copy_directory(source: &Utf8Path, destination: &Utf8Path) {
    fs::create_dir_all(destination).unwrap();
    for entry in fs::read_dir(source).unwrap() {
        let entry = entry.unwrap();
        let source_path = Utf8PathBuf::from_path_buf(entry.path()).unwrap();
        let destination_path = destination.join(entry.file_name().to_string_lossy().as_ref());
        if entry.file_type().unwrap().is_dir() {
            copy_directory(&source_path, &destination_path);
        } else {
            fs::copy(source_path, destination_path).unwrap();
        }
    }
}

#[test]
fn candidate_operator_removal_can_reach_reconciliation_before_validation() {
    let directory = tempdir().unwrap();
    let root = Utf8Path::from_path(directory.path())
        .unwrap()
        .join("project");
    let starter = Utf8Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/starter");
    copy_directory(&starter, &root);

    let dependency_root = root.join("modules/operator-pack");
    fs::create_dir_all(&dependency_root).unwrap();
    for document in ["gain.operator.dawn", "time-warp.operator.dawn"] {
        fs::copy(
            root.join("operators").join(document),
            dependency_root.join(document),
        )
        .unwrap();
    }
    let dependency_module_id = Uuid::new_v4();
    PackageManifest {
        manifest_version: dawn_package::MANIFEST_VERSION,
        module_id: dependency_module_id,
        language_version: "0.1".to_string(),
        requires_dawn: VersionReq::parse(">=0.1.0, <1.0.0").unwrap(),
        project: None,
        publication: None,
        exports: BTreeMap::from([(
            "operators".to_string(),
            ExportGroup {
                documents: vec![
                    "gain.operator.dawn".to_string(),
                    "time-warp.operator.dawn".to_string(),
                ],
            },
        )]),
        dependencies: BTreeMap::new(),
        assets: BTreeMap::new(),
    }
    .write(&dependency_root)
    .unwrap();

    let mut root_manifest = PackageManifest::read(&root).unwrap();
    root_manifest.dependencies.insert(
        "operators".to_string(),
        Dependency::Path {
            path: "modules/operator-pack".to_string(),
        },
    );
    root_manifest.write(&root).unwrap();
    let sequence_path = root.join("sequences/layer_test.sequence.dawn");
    let sequence = fs::read_to_string(&sequence_path)
        .unwrap()
        .replace("\r\n", "\n");
    let local_import = "\
- from:
    documents:
    - operators/gain.operator.dawn
    - operators/time-warp.operator.dawn
  as: operators";
    let dependency_import = "\
- from:
    dependency: operators
    export: operators
  as: operators";
    assert!(sequence.contains(local_import));
    fs::write(
        &sequence_path,
        sequence.replace(local_import, dependency_import),
    )
    .unwrap();

    let cache = CacheStore::new(root.join("unused-cache"));
    let current_lock =
        Lockfile::from_directory(&root_manifest, &root, "https://registry.dawn.dev").unwrap();
    let current = dawn_project_io::load_package_with_cache(
        &root,
        root_manifest.clone(),
        current_lock,
        &cache,
    )
    .unwrap()
    .session;
    let old_operator = current
        .project
        .definitions
        .operators
        .definitions
        .keys()
        .find(|id| id.0.module_id() == dependency_module_id && id.0.object() == "TimeWarp")
        .cloned()
        .unwrap();

    fs::write(
        dependency_root.join("time-warp.operator.dawn"),
        "\
operator Warp {
  input Signal signal;

  color sample() {
    return signal.at(seconds());
  }
}
",
    )
    .unwrap();
    let candidate_lock =
        Lockfile::from_directory(&root_manifest, &root, "https://registry.dawn.dev").unwrap();
    assert!(
        dawn_project_io::load_package_with_cache(
            &root,
            root_manifest.clone(),
            candidate_lock.clone(),
            &cache,
        )
        .is_err()
    );

    let candidate = dawn_project_io::load_package_for_operator_reconciliation_with_cache(
        &root,
        root_manifest,
        candidate_lock,
        &cache,
        &current.project.definitions.operators,
    )
    .unwrap()
    .session;
    assert!(
        candidate
            .project
            .definitions
            .operators
            .get(&old_operator)
            .is_none()
    );
    assert!(
        candidate
            .project
            .definitions
            .operators
            .definitions
            .keys()
            .any(|id| id.0.module_id() == dependency_module_id && id.0.object() == "Warp")
    );
    assert!(candidate.project.sequences.values().any(|sequence| {
        sequence.composition_graph.nodes.iter().any(|node| {
            matches!(
                &node.kind,
                CompositionGraphNodeKind::Operator(operator)
                    if matches!(&operator.operator, OperatorRef::Custom(id) if id == &old_operator)
            )
        })
    }));
    let sequence = candidate
        .project
        .sequences
        .values()
        .find(|sequence| {
            sequence.composition_graph.nodes.iter().any(|node| {
                matches!(
                    &node.kind,
                    CompositionGraphNodeKind::Operator(operator)
                        if matches!(&operator.operator, OperatorRef::Custom(id) if id == &old_operator)
                )
            })
        })
        .unwrap();
    assert!(
        validate_composition_graph(
            &sequence.composition_graph,
            &candidate.project.definitions.operators,
        )
        .is_err()
    );
}
