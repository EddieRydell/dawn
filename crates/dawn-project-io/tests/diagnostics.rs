use camino::{Utf8Path, Utf8PathBuf};
use dawn_project_io::{
    IoDiagnosticCode, IoDiagnosticSeverity, TextRange, check_document_text, check_project,
    check_project_document_text,
};
use std::fs;

#[test]
fn invalid_yaml_reports_parser_range() {
    let temp = tempfile::tempdir().unwrap();
    let root = Utf8PathBuf::from_path_buf(temp.path().to_path_buf()).unwrap();
    let entrypoint = root.join("project.dawn");
    fs::write(
        &entrypoint,
        "broken:\n  type: project\n  setup: [\n  sequences: []\n",
    )
    .unwrap();

    let report = check_project(&entrypoint);
    let diagnostic = report
        .diagnostics
        .iter()
        .find(|diagnostic| diagnostic.code == IoDiagnosticCode::YamlParse)
        .unwrap();

    assert!(report.session.is_none());
    assert_eq!(diagnostic.path, Utf8Path::new("project.dawn"));
    assert_eq!(diagnostic.severity, IoDiagnosticSeverity::Error);
    assert!(
        diagnostic.range.is_some(),
        "YAML parser diagnostics should include a source range"
    );
}

#[test]
fn invalid_effect_dsl_reports_exact_span() {
    let diagnostics = check_document_text(
        Utf8Path::new("effects/bad.effect.dawn"),
        "effect Bad {\n  color sample() {\n    return @;\n  }\n}\n",
    );
    let diagnostic = diagnostics
        .iter()
        .find(|diagnostic| diagnostic.code == IoDiagnosticCode::EffectCompile)
        .unwrap();
    let range = diagnostic.range.as_ref().unwrap();

    assert_eq!(diagnostic.path, Utf8Path::new("effects/bad.effect.dawn"));
    assert_eq!(diagnostic.severity, IoDiagnosticSeverity::Error);
    assert_eq!(range.start.line, 2);
    assert_eq!(range.start.character, 11);
    assert_eq!(range.end.line, 2);
    assert_eq!(range.end.character, 12);
}

#[test]
fn invalid_operator_dsl_reports_operator_compile_diagnostic() {
    let diagnostics = check_document_text(
        Utf8Path::new("operators/bad.operator.dawn"),
        "operator Bad { input Signal source; color sample() { return source.at(true); } }",
    );
    let diagnostic = diagnostics
        .iter()
        .find(|diagnostic| diagnostic.code == IoDiagnosticCode::OperatorCompile)
        .unwrap();
    assert_eq!(
        diagnostic.path,
        Utf8Path::new("operators/bad.operator.dawn")
    );
    assert_eq!(diagnostic.severity, IoDiagnosticSeverity::Error);
    assert!(diagnostic.range.is_some());
}

#[test]
fn invalid_reference_reports_dawn_reference_diagnostic() {
    let temp = tempfile::tempdir().unwrap();
    let root = Utf8PathBuf::from_path_buf(temp.path().to_path_buf()).unwrap();
    let entrypoint = root.join("project.dawn");
    fs::write(
        &entrypoint,
        "main:\n  type: project\n  setup: missing.setup\n  sequences: []\n",
    )
    .unwrap();

    let report = check_project(&entrypoint);
    let diagnostic = report
        .diagnostics
        .iter()
        .find(|diagnostic| diagnostic.code == IoDiagnosticCode::DawnReference)
        .unwrap();

    assert!(report.session.is_none());
    assert_eq!(diagnostic.path, Utf8Path::new("project.dawn"));
    assert_eq!(diagnostic.severity, IoDiagnosticSeverity::Error);
    assert_range(diagnostic.range.as_ref().unwrap(), 2, 9, 2, 22);
    assert!(diagnostic.message.contains("missing.setup"));
}

#[test]
fn repeated_reference_text_reports_the_failing_occurrence() {
    let temp = tempfile::tempdir().unwrap();
    let root = Utf8PathBuf::from_path_buf(temp.path().to_path_buf()).unwrap();
    let entrypoint = root.join("project.dawn");
    fs::write(
        &entrypoint,
        "imports:\n- from: setup.dawn\n  as: shared\nmain:\n  type: project\n  setup: shared.main\n  sequences: [shared.main]\n",
    )
    .unwrap();
    fs::write(
        root.join("setup.dawn"),
        "imports:\n- from: display.dawn\n  as: display\n- from: patch.dawn\n  as: patches\nmain:\n  type: setup\n  elements: display.elements\n  preview: display.preview\n  patch: patches.main\n  controllers: []\n",
    )
    .unwrap();
    fs::write(root.join("display.dawn"), "elements:\n  type: element_tree\n  roots: [1]\n  nodes:\n  - id: 1\n    name: Pixel\n    type: color\n    cells: 1\n    capability: { type: rgb }\npreview:\n  type: preview_layout\n  element_tree: elements\n  props: []\n").unwrap();
    fs::write(
        root.join("patch.dawn"),
        "main:\n  type: patch\n  nodes: []\n  edges: []\n",
    )
    .unwrap();

    let report = check_project(&entrypoint);
    let diagnostic = report
        .diagnostics
        .iter()
        .find(|diagnostic| diagnostic.code == IoDiagnosticCode::DawnReference)
        .unwrap();
    let range = diagnostic.range.as_ref().unwrap();
    assert_eq!(range.start.line, 6);
    assert!(range.start.character >= 14);
}

#[test]
fn project_document_override_runs_semantic_validation() {
    let temp = tempfile::tempdir().unwrap();
    let root = Utf8PathBuf::from_path_buf(temp.path().to_path_buf()).unwrap();
    write_imported_sequence_project(
        &root,
        "  duration: 1s\n  frame_rate: 60\n  audio: null\n  mark_collections: []\n  layers: []\n  effects: []\n  composition_graph: { nodes: [], edges: [] }\n  automation_clips: []\n",
    );
    let diagnostics = check_project_document_text(
        &root.join("project.dawn"),
        Utf8Path::new("sequence.dawn"),
        "main:\n  type: sequence\n  duration: invalid\n  frame_rate: 60\n  audio: null\n  mark_collections: []\n  layers: []\n  effects: []\n  composition_graph: { nodes: [], edges: [] }\n  automation_clips: []\n",
    );
    assert!(diagnostics.iter().any(|diagnostic| {
        diagnostic.path == Utf8Path::new("sequence.dawn")
            && diagnostic.code == IoDiagnosticCode::DawnLoad
    }));
}

#[test]
fn missing_required_field_reports_containing_object_range() {
    let temp = tempfile::tempdir().unwrap();
    let root = Utf8PathBuf::from_path_buf(temp.path().to_path_buf()).unwrap();
    let entrypoint = root.join("project.dawn");
    fs::write(&entrypoint, "main:\n  type: project\n  sequences: []\n").unwrap();

    let report = check_project(&entrypoint);
    let diagnostic = report
        .diagnostics
        .iter()
        .find(|diagnostic| {
            diagnostic.code == IoDiagnosticCode::DawnLoad
                && diagnostic.message.contains("missing field `setup`")
        })
        .unwrap();

    assert_range(diagnostic.range.as_ref().unwrap(), 1, 6, 3, 0);
}

#[test]
fn wrong_field_type_reports_bad_value_range() {
    let temp = tempfile::tempdir().unwrap();
    let root = Utf8PathBuf::from_path_buf(temp.path().to_path_buf()).unwrap();
    let entrypoint = root.join("project.dawn");
    fs::write(
        &entrypoint,
        "main:\n  type: project\n  setup: [bad]\n  sequences: []\n",
    )
    .unwrap();

    let report = check_project(&entrypoint);
    let diagnostic = report
        .diagnostics
        .iter()
        .find(|diagnostic| diagnostic.message == "field `setup` must be a string")
        .unwrap();

    assert_range(diagnostic.range.as_ref().unwrap(), 2, 9, 2, 13);
}

#[test]
fn unsupported_enum_string_reports_that_string_range() {
    let temp = tempfile::tempdir().unwrap();
    let root = Utf8PathBuf::from_path_buf(temp.path().to_path_buf()).unwrap();
    let entrypoint = root.join("project.dawn");
    fs::write(&entrypoint, "main:\n  type: nope\n").unwrap();

    let report = check_project(&entrypoint);
    let diagnostic = report
        .diagnostics
        .iter()
        .find(|diagnostic| diagnostic.message == "unsupported object type `nope`")
        .unwrap();

    assert_range(diagnostic.range.as_ref().unwrap(), 1, 8, 1, 12);
}

#[test]
fn nested_invalid_color_reports_nested_scalar_range() {
    let temp = tempfile::tempdir().unwrap();
    let root = Utf8PathBuf::from_path_buf(temp.path().to_path_buf()).unwrap();
    let entrypoint = root.join("project.dawn");
    write_imported_sequence_project(
        &root,
        "  duration: 1s\n  frame_rate: 30\n  mark_collections:\n    - key: beats\n      name: Beats\n      color: bad-color\n      marks: []\n",
    );

    let report = check_project(&entrypoint);
    let diagnostic = report
        .diagnostics
        .iter()
        .find(|diagnostic| diagnostic.message == "invalid color: bad-color")
        .unwrap();

    assert_range(diagnostic.range.as_ref().unwrap(), 7, 13, 7, 22);
}

#[test]
fn nested_invalid_duration_reports_nested_scalar_range() {
    let temp = tempfile::tempdir().unwrap();
    let root = Utf8PathBuf::from_path_buf(temp.path().to_path_buf()).unwrap();
    let entrypoint = root.join("project.dawn");
    write_imported_sequence_project(
        &root,
        "  duration: soon\n  frame_rate: 30\n  layers: []\n  effects: []\n  composition_graph:\n    nodes: []\n    edges: []\n",
    );

    let report = check_project(&entrypoint);
    let diagnostic = report
        .diagnostics
        .iter()
        .find(|diagnostic| diagnostic.message == "duration must end in `s`: soon")
        .unwrap();

    assert_range(diagnostic.range.as_ref().unwrap(), 2, 12, 2, 16);
}

#[test]
fn imported_effect_errors_keep_exact_spans_without_aggregate_marker() {
    let temp = tempfile::tempdir().unwrap();
    let root = Utf8PathBuf::from_path_buf(temp.path().to_path_buf()).unwrap();
    let entrypoint = root.join("project.dawn");
    fs::write(
        &entrypoint,
        "imports:\n  - from: bad.effect.dawn\n    as: fx\nmain:\n  type: project\n  setup: missing.setup\n  sequences: []\n",
    )
    .unwrap();
    fs::write(
        root.join("bad.effect.dawn"),
        "effect Bad {\n  color sample() {\n    return @;\n  }\n}\n",
    )
    .unwrap();

    let report = check_project(&entrypoint);
    let effect_diagnostics = report
        .diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.code == IoDiagnosticCode::EffectCompile)
        .collect::<Vec<_>>();

    assert!(
        effect_diagnostics
            .iter()
            .all(|diagnostic| diagnostic.range.is_some())
    );
    assert!(effect_diagnostics.iter().any(|diagnostic| diagnostic.path
        == Utf8Path::new("bad.effect.dawn")
        && diagnostic.range.as_ref().is_some_and(|range| {
            range.start.line == 2
                && range.start.character == 11
                && range.end.line == 2
                && range.end.character == 12
        })));
    assert!(
        effect_diagnostics
            .iter()
            .all(|diagnostic| diagnostic.range.as_ref().is_none_or(|range| {
                range.start.line != 0 || range.start.character != 0 || range.end.character != 1
            }))
    );
}

#[test]
fn invalid_entrypoint_reports_no_range() {
    let temp = tempfile::tempdir().unwrap();
    let root = Utf8PathBuf::from_path_buf(temp.path().to_path_buf()).unwrap();
    let report = check_project(&root.join("missing.dawn"));
    let diagnostic = report.diagnostics.first().unwrap();

    assert_eq!(diagnostic.code, IoDiagnosticCode::IoRead);
    assert_eq!(diagnostic.range, None);
}

#[test]
fn valid_example_project_loads_without_diagnostics() {
    let workspace_root = Utf8Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Utf8Path::parent)
        .unwrap();
    let entrypoint = workspace_root.join("examples/thirty-output-controller/project.dawn");

    let report = check_project(&entrypoint);

    assert!(report.session.is_some());
    assert_eq!(report.diagnostics, Vec::new());
}

fn write_imported_sequence_project(root: &Utf8Path, sequence_body: &str) {
    fs::write(
        root.join("project.dawn"),
        "imports:\n  - from: setup.dawn\n    as: setups\n  - from: sequence.dawn\n    as: sequences\nmain:\n  type: project\n  setup: setups.main\n  sequences: [sequences.main]\n",
    )
    .unwrap();
    fs::write(
        root.join("setup.dawn"),
        "imports:\n  - from: display.dawn\n    as: display\n  - from: patch.dawn\n    as: patches\nmain:\n  type: setup\n  elements: display.elements\n  preview: display.preview\n  patch: patches.main\n  controllers: []\n",
    )
    .unwrap();
    fs::write(root.join("display.dawn"), "elements:\n  type: element_tree\n  roots: [1]\n  nodes:\n  - id: 1\n    name: Pixel\n    type: color\n    cells: 1\n    capability: { type: rgb }\npreview:\n  type: preview_layout\n  element_tree: elements\n  props: []\n").unwrap();
    fs::write(
        root.join("patch.dawn"),
        "main:\n  type: patch\n  nodes: []\n  edges: []\n",
    )
    .unwrap();
    fs::write(
        root.join("sequence.dawn"),
        format!("main:\n  type: sequence\n{sequence_body}"),
    )
    .unwrap();
}

fn assert_range(
    range: &TextRange,
    start_line: u32,
    start_character: u32,
    end_line: u32,
    end_character: u32,
) {
    assert_eq!(
        (
            range.start.line,
            range.start.character,
            range.end.line,
            range.end.character
        ),
        (start_line, start_character, end_line, end_character)
    );
}
