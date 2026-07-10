use camino::{Utf8Path, Utf8PathBuf};
use dawn_language::operator::{OperatorDefinitionId, OperatorRef};
use dawn_language::sequence::CompositionGraphNodeKind;
use dawn_project_io::{
    SourceDocumentKind, SourceObjectId, SourceObjectKind, export_project, load_project,
    save_project, source_file_list,
};
use std::fs;

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
        assert_eq!(
            original.source.import_graph, exported.source.import_graph,
            "{entrypoint}"
        );
        assert_eq!(
            original.source.source_map, exported.source.source_map,
            "{entrypoint}"
        );

        for (path, document) in &original.source.documents {
            if matches!(
                document.kind,
                SourceDocumentKind::Effect { .. } | SourceDocumentKind::Operator { .. }
            ) {
                let original_bytes = fs::read(original.source.source_root.join(path)).unwrap();
                let exported_bytes = fs::read(output.join(path)).unwrap();
                assert_eq!(original_bytes, exported_bytes, "{path}");
            }
        }

        if entrypoint.ends_with(Utf8Path::new(
            "examples/thirty-output-controller/project.dawn",
        )) {
            assert!(
                original
                    .project
                    .definitions
                    .operators
                    .get(&OperatorDefinitionId("Gain".to_string()))
                    .is_some()
            );
            assert!(
                original
                    .source
                    .source_map
                    .objects
                    .contains_key(&SourceObjectId {
                        kind: SourceObjectKind::OperatorDefinition,
                        id: "Gain".to_string(),
                    })
            );
            let layer_test = original
                .project
                .sequences
                .values()
                .find(|sequence| sequence.id.0 == "layer_test")
                .unwrap();
            assert!(layer_test.composition_graph.nodes.iter().any(|node| {
                matches!(
                    &node.kind,
                    CompositionGraphNodeKind::Operator(operator)
                        if operator.operator
                            == OperatorRef::Custom(OperatorDefinitionId("Gain".to_string()))
                )
            }));
            assert!(
                !original.source.referenced_assets.is_empty(),
                "thirty-output-controller should reference audio"
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

fn example_entrypoints() -> Vec<Utf8PathBuf> {
    let workspace_root = Utf8Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Utf8Path::parent)
        .unwrap();
    vec![
        workspace_root.join("examples/starter/project.dawn"),
        workspace_root.join("examples/christmas-house/project.dawn"),
        workspace_root.join("examples/thirty-output-controller/project.dawn"),
    ]
}
