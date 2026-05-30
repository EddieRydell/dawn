use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use dawn_project::analysis::{
    analyze_project as core_analyze_project,
    analyze_project_with_overlays as core_analyze_project_with_overlays, DiagnosticCode,
    ProjectAnalysis, ProjectOverlay,
};
use dawn_project::document::{
    apply_fixture_document_edit as core_apply_fixture_document_edit,
    apply_layout_document_edit as core_apply_layout_document_edit,
    apply_sequence_document_edit as core_apply_sequence_document_edit,
    get_fixture_document as core_get_fixture_document,
    get_layout_document as core_get_layout_document,
    get_sequence_document as core_get_sequence_document, DocumentEditOutcome,
    FixtureDefinitionDocument, FixtureDocument, LayoutDocument, LayoutFixturePlacement,
    LayoutFixtureRef, LayoutTargetDocument, ResolvedLayoutFixture, SequenceDocument,
    SequenceDocumentEdit,
};
use dawn_project::effect_script::{
    compile as compile_effect_script, FixtureContext, PixelContext, RuntimeValue,
};
use dawn_project::fs::WorkspaceFs;
use dawn_project::model::{
    Color, ColorModel, Curve, CurveValue, CurveValueType, FixtureId, Geometry, LayoutTargetKind,
    Point3, Transform,
};
use dawn_project::path::{utf8_path, PathStringExt, Utf8PathBuf};
use dawn_project::render::{GeometryRenderBounds, GeometryRenderGuide, GeometryRenderPlan};

fn project_context(project_path: impl AsRef<Path>) -> (WorkspaceFs, Utf8PathBuf, PathBuf) {
    let project_path = project_path.as_ref();
    let root = project_path
        .parent()
        .expect("test project path should have a parent")
        .to_path_buf();
    let fs = WorkspaceFs::open(&root).expect("test project root should open");
    let relative = relative_project_path(&root, project_path);
    (fs, relative, root)
}

fn relative_project_path(root: &Path, path: &Path) -> Utf8PathBuf {
    utf8_path(
        path.strip_prefix(root)
            .expect("test path should be inside project root"),
    )
    .expect("test path should be valid UTF-8")
}

fn canonical_test_path(path: impl AsRef<Path>) -> Utf8PathBuf {
    utf8_path(path.as_ref().canonicalize().unwrap()).expect("test path should be valid UTF-8")
}

fn normalize_overlays(root: &Path, overlays: Vec<ProjectOverlay>) -> Vec<ProjectOverlay> {
    overlays
        .into_iter()
        .map(|overlay| ProjectOverlay {
            path: if overlay.path.as_std_path().is_absolute() {
                relative_project_path(root, overlay.path.as_std_path())
            } else {
                overlay.path
            },
            content: overlay.content,
        })
        .collect()
}

fn analyze_project(project_path: impl AsRef<Path>, project_key: &str) -> ProjectAnalysis {
    let (fs, project_path, _) = project_context(project_path);
    core_analyze_project(&fs, project_path, project_key)
}

fn analyze_project_with_overlays(
    project_path: impl AsRef<Path>,
    project_key: Option<&str>,
    overlays: Vec<ProjectOverlay>,
) -> ProjectAnalysis {
    let (fs, project_path, root) = project_context(project_path);
    core_analyze_project_with_overlays(
        &fs,
        project_path,
        project_key,
        normalize_overlays(&root, overlays),
    )
}

fn get_layout_document(
    path: impl AsRef<Path>,
    object_key: &str,
    project_path: impl AsRef<Path>,
    overlays: Vec<ProjectOverlay>,
) -> Result<LayoutDocument, String> {
    let (fs, project_path, root) = project_context(project_path);
    let path = relative_project_path(&root, path.as_ref());
    core_get_layout_document(
        &fs,
        path,
        object_key,
        project_path,
        normalize_overlays(&root, overlays),
    )
}

fn get_fixture_document(
    path: impl AsRef<Path>,
    selected_object_key: Option<&str>,
    overlays: Vec<ProjectOverlay>,
) -> Result<FixtureDocument, String> {
    let root = path
        .as_ref()
        .parent()
        .expect("fixture path should have a parent");
    let fs = WorkspaceFs::open(root).expect("test project root should open");
    let path = relative_project_path(root, path.as_ref());
    core_get_fixture_document(
        &fs,
        path,
        selected_object_key,
        normalize_overlays(root, overlays),
    )
}

fn get_sequence_document(
    path: impl AsRef<Path>,
    object_key: &str,
    project_path: impl AsRef<Path>,
    overlays: Vec<ProjectOverlay>,
) -> Result<SequenceDocument, String> {
    let (fs, project_path, root) = project_context(project_path);
    let path = relative_project_path(&root, path.as_ref());
    core_get_sequence_document(
        &fs,
        path,
        object_key,
        project_path,
        normalize_overlays(&root, overlays),
    )
}

fn apply_layout_document_edit(
    path: impl AsRef<Path>,
    object_key: &str,
    document: LayoutDocument,
    base_content: String,
    overlays: Vec<ProjectOverlay>,
    project_path: impl AsRef<Path>,
    _allow_breaking_references: bool,
) -> Result<DocumentEditOutcome<LayoutDocument>, String> {
    let (fs, _project_path, root) = project_context(project_path);
    let path = relative_project_path(&root, path.as_ref());
    core_apply_layout_document_edit(
        &fs,
        path,
        object_key,
        document,
        base_content,
        normalize_overlays(&root, overlays),
    )
}

fn apply_fixture_document_edit(
    path: impl AsRef<Path>,
    document: FixtureDocument,
    base_content: String,
    overlays: Vec<ProjectOverlay>,
    project_path: impl AsRef<Path>,
    _allow_breaking_references: bool,
) -> Result<DocumentEditOutcome<FixtureDocument>, String> {
    let (fs, _project_path, root) = project_context(project_path);
    let path = relative_project_path(&root, path.as_ref());
    core_apply_fixture_document_edit(
        &fs,
        path,
        document,
        base_content,
        normalize_overlays(&root, overlays),
    )
}

fn apply_sequence_document_edit(
    path: impl AsRef<Path>,
    object_key: &str,
    edit: SequenceDocumentEdit,
    base_content: String,
    overlays: Vec<ProjectOverlay>,
    project_path: impl AsRef<Path>,
) -> Result<DocumentEditOutcome<SequenceDocument>, String> {
    let (fs, project_path, root) = project_context(project_path);
    let path = relative_project_path(&root, path.as_ref());
    let overlays = normalize_overlays(&root, overlays);
    let analysis =
        core_analyze_project_with_overlays(&fs, project_path.clone(), None, overlays.clone());
    core_apply_sequence_document_edit(
        &fs,
        path,
        object_key,
        edit,
        base_content,
        overlays,
        &analysis,
    )
}

#[test]
fn analyzes_club_rig_to_resolved_project() {
    let project_path =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/club-rig/project.dawn");
    let analysis = analyze_project(project_path, "club_rig");

    assert!(
        analysis.diagnostics.is_empty(),
        "{:?}",
        analysis.diagnostics
    );
    let project = analysis.resolved.expect("club rig should resolve");
    assert!(project.display.layout.fixtures.len() >= 8);
    assert_eq!(project.display.patch.routes.len(), 8);
    assert_eq!(project.sequences[0].effects.len(), 4);
}

#[test]
fn color_serializes_as_hex_and_rejects_invalid_literals() {
    let color: Color = serde_yaml::from_str("\"#2244ff\"").unwrap();
    assert_eq!(color, Color::new(0x22, 0x44, 0xff));
    assert!(serde_yaml::to_string(&color).unwrap().contains("#2244ff"));

    let error = serde_yaml::from_str::<Color>("\"2244ff\"").unwrap_err();
    assert!(error.to_string().contains("start with `#`"));
}

#[test]
fn typed_curves_parse_serialize_and_interpolate() {
    let float_curve: Curve = serde_yaml::from_str(
        r##"
value_type: float
points:
  - time: 0.0
    value: 0.0
  - time: 1.0
    value: 10.0
"##,
    )
    .unwrap();
    assert_eq!(float_curve.value_type, CurveValueType::Float);
    assert_eq!(float_curve.evaluate_float(0.5), Some(5.0));
    assert!(serde_yaml::to_string(&float_curve)
        .unwrap()
        .contains("value_type: float"));

    let color_curve: Curve = serde_yaml::from_str(
        r##"
value_type: color
points:
  - time: 0.0
    value: "#000000"
  - time: 1.0
    value: "#ffffff"
"##,
    )
    .unwrap();
    assert_eq!(
        color_curve.evaluate(0.5),
        Some(CurveValue::Color(Color::new(128, 128, 128)))
    );

    let mismatch = serde_yaml::from_str::<Curve>(
        r##"
value_type: float
points:
  - time: 0.0
    value: "#ffffff"
"##,
    )
    .unwrap_err();
    assert!(mismatch.to_string().contains("float curve points"));
}

#[test]
fn effect_scripts_compile_and_evaluate_sample() {
    let script = compile_effect_script(
        r##"
effect Pulse {
  param color base = #000000;
  param color accent = #ffffff;
  param float speed = 1.0;

  color sample(float progress, float seconds, Fixture fixture, Pixel pixel) {
    float phase = (sin(seconds * speed) + 1.0) / 2.0;
    return mix(base, accent, phase);
  }
}
"##,
    )
    .unwrap();
    assert_eq!(script.name, "Pulse");
    assert_eq!(script.params.len(), 3);
    let color = script
        .sample(
            0.0,
            0.0,
            FixtureContext { index: 0 },
            PixelContext { index: 0, count: 1 },
            &Default::default(),
        )
        .unwrap();
    assert_eq!(color, Color::new(128, 128, 128));

    let error = compile_effect_script(
        "effect Bad { color nope(float progress, float seconds, Fixture fixture, Pixel pixel) { return #ffffff; } }",
    )
    .unwrap_err();
    assert!(error[0].range.is_some());

    let import_error = compile_effect_script(
        r#"
effect Bad {
  param curve<float> fade = import "../curves/fade-in.curve.dawn::fade_in";
  color sample(float progress, float seconds, Fixture fixture, Pixel pixel) {
    return #ffffff;
  }
}
"#,
    )
    .unwrap_err();
    assert!(import_error[0]
        .message
        .contains("parameter defaults cannot import files"));
}

#[test]
fn project_analysis_loads_effect_scripts_without_yaml_parsing() {
    let project_path =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/club-rig/project.dawn");
    let analysis = analyze_project(project_path, "club_rig");
    let effect_path = analysis
        .files
        .keys()
        .find(|path| path.to_slash_string().ends_with("pulse.effect.dawn"))
        .expect("effect script should be reachable")
        .clone();
    let analyzed = analysis.files.get(&effect_path).unwrap();

    assert!(analyzed.file.is_none());
    assert!(analyzed.script.is_some());
    assert!(analysis.compiled_script_for_path(&effect_path).is_some());
}

#[test]
fn sequence_params_are_validated_against_script_schema() {
    let dir = temp_dir("script-param-validation");
    fs::create_dir_all(dir.join("effects")).unwrap();
    fs::write(
        dir.join("effects/pulse.effect.dawn"),
        r##"
effect Pulse {
  param color base = #000000;
  param float speed;

  color sample(float progress, float seconds, Fixture fixture, Pixel pixel) {
    return base;
  }
}
"##,
    )
    .unwrap();
    fs::write(
        dir.join("project.dawn"),
        r##"
club:
  type: project
  name: club
  display:
    name: main
    controllers: []
    patch:
      routes: []
    layout:
      name: stage
      units: meters
      target_order:
        - type: group
          name: all
        - type: fixture
          name: Pixel
      fixtures:
        - id: 1
          name: Pixel
          fixture:
            name: Pixel
            color_model: rgb
            geometry:
              type: points
              points: []
          transform:
            position: { x: 0.0, y: 0.0, z: 0.0 }
      groups:
        - name: all
          members: [1]
  sequences:
    - duration: 1s
      frame_rate: 60
      audio:
      effects:
        - id: 1
          start: 0s
          duration: 1s
          target:
            type: group
            name: all
          scope: per_fixture
          params:
            base:
              type: float
              value: 1.0
            extra:
              type: color
              value: "#ffffff"
          script:
            import: effects/pulse.effect.dawn
"##,
    )
    .unwrap();

    let analysis = analyze_project(dir.join("project.dawn"), "club");

    assert!(analysis.resolved.is_none());
    assert!(analysis.diagnostics.iter().any(|diagnostic| diagnostic.code
        == DiagnosticCode::Script
        && diagnostic
            .message
            .contains("parameter `base` must be color")));
    assert!(analysis
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.code == DiagnosticCode::Script
            && diagnostic.message.contains("unknown parameter `extra`")));
    assert!(analysis.diagnostics.iter().any(|diagnostic| diagnostic.code
        == DiagnosticCode::Script
        && diagnostic
            .message
            .contains("missing required parameter `speed`")));
}

#[test]
fn effect_script_enum_and_flags_options_parse_defaults_and_validate_values() {
    let script = compile_effect_script(
        r##"
effect Options {
  param enum mode { normal, flash } = flash;
  param enum fallback { first, second };
  param flags mask { red, green, blue } = { red, blue };

  color sample(float progress, float seconds, Fixture fixture, Pixel pixel) {
    return #ffffff;
  }
}
"##,
    )
    .unwrap();

    assert_eq!(script.params[0].options, vec!["normal", "flash"]);
    assert_eq!(
        script.params[0].default,
        Some(dawn_project::effect_script::ParamDefault::Value(
            RuntimeValue::Enum("flash".to_string())
        ))
    );
    assert_eq!(script.params[1].default, None);
    assert_eq!(
        script.params[2].default,
        Some(dawn_project::effect_script::ParamDefault::Value(
            RuntimeValue::Flags(dawn_project::model::Flags {
                values: vec!["red".to_string(), "blue".to_string()]
            })
        ))
    );

    let invalid_default = compile_effect_script(
        r##"
effect Bad {
  param enum mode { normal } = flash;
  color sample(float progress, float seconds, Fixture fixture, Pixel pixel) { return #ffffff; }
}
"##,
    )
    .unwrap_err();
    assert!(invalid_default[0]
        .message
        .contains("is not declared in the option list"));
}

#[test]
fn sequence_numeric_effect_ids_and_automation_targets_are_validated() {
    let duplicate_dir = temp_dir("duplicate-sequence-effect-id");
    fs::write(
        duplicate_dir.join("project.dawn"),
        project_with_inline_sequence(
            r##"
        - id: 1
          start: 0s
          duration: 1s
          target: { type: group, name: all }
          scope: per_fixture
          params: {}
          script: |
            effect One { color sample(float progress, float seconds, Fixture fixture, Pixel pixel) { return #ffffff; } }
        - id: 1
          start: 0s
          duration: 1s
          target: { type: group, name: all }
          scope: per_fixture
          params: {}
          script: |
            effect Two { color sample(float progress, float seconds, Fixture fixture, Pixel pixel) { return #ffffff; } }
"##,
        ),
    )
    .unwrap();
    let duplicate = analyze_project(duplicate_dir.join("project.dawn"), "club");
    assert!(duplicate
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.message.contains("duplicate sequence effect `1`")));

    let unknown_dir = temp_dir("unknown-sequence-effect-target");
    fs::write(
        unknown_dir.join("project.dawn"),
        format!(
            "{}{}",
            project_with_inline_sequence(
                r##"
        - id: 1
          start: 0s
          duration: 1s
          target: { type: group, name: all }
          scope: per_fixture
          params: {}
          script: |
            effect One { color sample(float progress, float seconds, Fixture fixture, Pixel pixel) { return #ffffff; } }
"##
            )
            .trim_end(),
            r#"
      automation_clips:
        - id: 2
          start: 0s
          duration: 1s
          curve:
            value_type: float
            points:
              - time: 0.0
                value: 0.0
          targets: [99]
"#
        ),
    )
    .unwrap();
    let unknown = analyze_project(unknown_dir.join("project.dawn"), "club");
    assert!(unknown
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.message.contains("unknown sequence effect `99`")));
}

#[test]
fn sequence_param_analysis_rejects_undeclared_enum_and_flag_values() {
    let dir = temp_dir("enum-flags-authored-values");
    fs::create_dir_all(dir.join("effects")).unwrap();
    fs::write(
        dir.join("effects/options.effect.dawn"),
        r##"
effect Options {
  param enum mode { normal, flash };
  param flags mask { red, green, blue };
  color sample(float progress, float seconds, Fixture fixture, Pixel pixel) { return #ffffff; }
}
"##,
    )
    .unwrap();
    fs::write(
        dir.join("project.dawn"),
        project_with_inline_sequence(
            r##"
        - id: 1
          start: 0s
          duration: 1s
          target: { type: group, name: all }
          scope: per_fixture
          params:
            mode:
              type: enum
              value: missing
            mask:
              type: flags
              value: [red, missing]
          script:
            import: effects/options.effect.dawn
"##,
        ),
    )
    .unwrap();

    let analysis = analyze_project(dir.join("project.dawn"), "club");

    assert!(analysis.resolved.is_none());
    assert!(analysis.diagnostics.iter().any(|diagnostic| diagnostic
        .message
        .contains("value `missing` is not declared")));
    assert!(analysis.diagnostics.iter().any(|diagnostic| diagnostic
        .message
        .contains("flag `missing` is not declared")));
}

#[test]
fn sequence_document_edit_adds_duplicates_moves_retargets_sorts_and_cleans_targets() {
    let dir = temp_dir("sequence-document-edit-ops");
    fs::create_dir_all(dir.join("effects")).unwrap();
    fs::create_dir_all(dir.join("sequences")).unwrap();
    fs::write(
        dir.join("effects/options.effect.dawn"),
        r##"
effect Options {
  param float amount;
  param int count;
  param bool enabled;
  param color tint;
  param curve<float> fade;
  param curve<color> wash;
  param enum mode { normal, flash };
  param flags mask { red, green, blue };
  color sample(float progress, float seconds, Fixture fixture, Pixel pixel) { return tint; }
}
"##,
    )
    .unwrap();
    fs::write(
        dir.join("project.dawn"),
        r#"
club:
  type: project
  name: club
  display:
    import: display.dawn::main
  sequences:
    - import: sequences/opening.sequence.dawn::opening
"#,
    )
    .unwrap();
    fs::write(
        dir.join("display.dawn"),
        r#"
main:
  type: display
  name: main
  controllers: []
  patch:
    routes: []
  layout:
    name: stage
    units: meters
    target_order:
      - type: group
        name: all
      - type: fixture
        name: Pixel
    fixtures:
      - id: 1
        name: Pixel
        fixture:
          name: Pixel
          color_model: rgb
          geometry:
            type: points
            points: []
        transform:
          position: { x: 0.0, y: 0.0, z: 0.0 }
    groups:
      - name: all
        members: [1]
"#,
    )
    .unwrap();
    let sequence_path = dir.join("sequences/opening.sequence.dawn");
    fs::write(
        &sequence_path,
        r#"
opening:
  type: sequence
  duration: 2s
  frame_rate: 60
  audio:
  effects: []
  automation_clips:
    - id: 50
      start: 0s
      duration: 1s
      curve:
        value_type: float
        points:
          - time: 0.0
            value: 0.0
      targets: [1]
"#,
    )
    .unwrap();
    let project_path = dir.join("project.dawn");
    let document = get_sequence_document(&sequence_path, "opening", &project_path, Vec::new())
        .expect("sequence document should load");
    let script = document.effect_scripts[0].clone();
    let base = fs::read_to_string(&sequence_path).unwrap();
    let added = apply_sequence_document_edit(
        &sequence_path,
        "opening",
        SequenceDocumentEdit::AddEffect {
            script_path: script.path,
            target: LayoutTargetDocument {
                kind: LayoutTargetKind::Group,
                name: "all".to_string(),
            },
            scope: dawn_project::model::SequenceEffectScope::WholeTarget,
            start_ms: 1_500,
            mark_collection_key: None,
        },
        base,
        Vec::new(),
        &project_path,
    )
    .unwrap();
    assert!(added.serialized_content.contains("id: 1"));
    assert!(added.serialized_content.contains("value: 0.0"));
    assert!(added.serialized_content.contains("value: false"));
    assert!(added.serialized_content.contains("#ffffff"));
    assert!(added.serialized_content.contains("value: normal"));
    assert!(added.serialized_content.contains("value: []"));

    let duplicated = apply_sequence_document_edit(
        &sequence_path,
        "opening",
        SequenceDocumentEdit::DuplicateEffect { id: 1 },
        added.serialized_content,
        Vec::new(),
        &project_path,
    )
    .unwrap();
    assert_eq!(duplicated.refreshed_document.effects.len(), 2);
    assert_eq!(
        duplicated.refreshed_document.effects[0].scope,
        dawn_project::model::SequenceEffectScope::WholeTarget
    );
    assert_eq!(
        duplicated.refreshed_document.effects[1].scope,
        dawn_project::model::SequenceEffectScope::WholeTarget
    );

    let moved = apply_sequence_document_edit(
        &sequence_path,
        "opening",
        SequenceDocumentEdit::MoveEffect {
            id: 2,
            start_ms: 0,
            target: Some(LayoutTargetDocument {
                kind: LayoutTargetKind::Fixture,
                name: "1".to_string(),
            }),
        },
        duplicated.serialized_content,
        Vec::new(),
        &project_path,
    )
    .unwrap();
    assert_eq!(moved.refreshed_document.effects[0].id, 2);
    assert_eq!(moved.refreshed_document.effects[0].target.name, "1");
    assert_eq!(
        moved.refreshed_document.effects[0].scope,
        dawn_project::model::SequenceEffectScope::WholeTarget
    );

    let deleted = apply_sequence_document_edit(
        &sequence_path,
        "opening",
        SequenceDocumentEdit::DeleteEffect { id: 1 },
        moved.serialized_content,
        Vec::new(),
        &project_path,
    )
    .unwrap();
    assert!(deleted.serialized_content.contains("automation_clips:"));
    assert!(deleted.serialized_content.contains("targets: []"));
}

#[test]
fn reports_semantic_diagnostics_without_resolved_project() {
    let dir = temp_dir("semantic");
    let project_path = dir.join("project.dawn");
    fs::write(
        &project_path,
        r##"
club:
  type: project
  name: bad
  display:
    name: main
    controllers: []
    patch:
      routes: []
    layout:
      name: stage
      units: meters
      target_order:
        - type: fixture
          name: Bar 01
      fixtures:
        - id: 1
          name: Bar 01
          fixture:
            name: PixelBar
            color_model: rgb
            geometry:
              type: points
              points:
                - { x: 0.0, y: 0.0, z: 0.0 }
          transform:
            position: { x: 0.0, y: 0.0, z: 0.0 }
      groups: []
  sequences:
    - duration: 1s
      frame_rate: 60
      audio:
      effects:
        - id: 1
          start: 0s
          duration: 1s
          target:
            type: group
            name: MissingGroup
          scope: per_fixture
          params: {}
          script: "inline effect"
"##,
    )
    .unwrap();

    let analysis = analyze_project(&project_path, "club");

    assert!(analysis.resolved.is_none());
    assert_eq!(analysis.diagnostics.len(), 1);
    assert_eq!(analysis.diagnostics[0].code, DiagnosticCode::Lower);
    assert!(analysis.diagnostics[0]
        .message
        .contains("unknown group `MissingGroup`"));
    assert!(analysis.diagnostics[0].range.is_some());
}

#[test]
fn resolves_numeric_fixture_references_across_layout_patch_and_sequences() {
    let dir = temp_dir("numeric-fixture-refs");
    let project_path = dir.join("project.dawn");
    fs::write(
        &project_path,
        r##"
club:
  type: project
  name: club
  display:
    name: main
    controllers:
      - name: WallController
        protocol: artnet
        universes: []
    patch:
      routes:
        - fixture: 1
          controller: WallController
          universe: 1
          start: 1
    layout:
      name: stage
      units: meters
      target_order:
        - type: group
          name: WallBars
        - type: fixture
          name: Front Bar 1
      fixtures:
        - id: 1
          name: Front Bar 1
          fixture:
            name: PixelBar
            color_model: rgb
            geometry:
              type: points
              points:
                - { x: 0.0, y: 0.0, z: 0.0 }
          transform:
            position: { x: 0.0, y: 0.0, z: 0.0 }
      groups:
        - name: WallBars
          members: [1]
  sequences:
    - duration: 1s
      frame_rate: 60
      audio:
      effects:
        - id: 1
          start: 0s
          duration: 1s
          target:
            type: group
            name: WallBars
          scope: per_fixture
          params: {}
          script: |
            effect InlineGroup {
              color sample(float progress, float seconds, Fixture fixture, Pixel pixel) {
                return #ffffff;
              }
            }
        - id: 2
          start: 0s
          duration: 1s
          target:
            type: fixture
            id: 1
          scope: per_fixture
          params: {}
          script: |
            effect InlineFixture {
              color sample(float progress, float seconds, Fixture fixture, Pixel pixel) {
                return #ffffff;
              }
            }
"##,
    )
    .unwrap();

    let analysis = analyze_project(&project_path, "club");

    assert!(
        analysis.diagnostics.is_empty(),
        "{:?}",
        analysis.diagnostics
    );
    let project = analysis.resolved.expect("numeric refs should resolve");
    assert_eq!(project.display.layout.groups[0].members[0].0, 0);
    assert_eq!(project.display.patch.routes[0].fixture.0, 0);
    assert_eq!(project.sequences[0].effects.len(), 2);
}

#[test]
fn rejects_duplicate_fixture_ids_and_empty_or_duplicate_fixture_names() {
    for (label, fixture_overrides, expected) in [
        (
            "duplicate-id",
            "        - id: 1\n          name: Second\n",
            "duplicate fixture id `1`",
        ),
        (
            "empty-name",
            "        - id: 2\n          name: '   '\n",
            "fixture name cannot be empty",
        ),
        (
            "duplicate-name",
            "        - id: 2\n          name: First\n",
            "duplicate fixture name `First`",
        ),
    ] {
        let dir = temp_dir(label);
        let project_path = dir.join("project.dawn");
        fs::write(
            &project_path,
            format!(
                r#"
club:
  type: project
  name: club
  display:
    name: main
    controllers: []
    patch:
      routes: []
    layout:
      name: stage
      units: meters
      fixtures:
        - id: 1
          name: First
          fixture:
            name: Pixel
            color_model: rgb
            geometry:
              type: points
              points: []
          transform:
            position: {{ x: 0.0, y: 0.0, z: 0.0 }}
{fixture_overrides}          fixture:
            name: Pixel
            color_model: rgb
            geometry:
              type: points
              points: []
          transform:
            position: {{ x: 1.0, y: 0.0, z: 0.0 }}
      groups: []
"#
            ),
        )
        .unwrap();

        let analysis = analyze_project(&project_path, "club");

        assert!(analysis.resolved.is_none());
        assert!(
            analysis.diagnostics[0].message.contains(expected),
            "{:?}",
            analysis.diagnostics
        );
    }
}

#[test]
fn reports_import_diagnostics_and_keeps_parsed_files() {
    let dir = temp_dir("missing-import");
    let project_path = dir.join("project.dawn");
    fs::write(
        &project_path,
        r#"
club:
  type: project
  name: missing-import
  display:
    import: missing.display.dawn::main
"#,
    )
    .unwrap();

    let analysis = analyze_project(&project_path, "club");

    assert!(analysis.resolved.is_none());
    assert!(analysis
        .files
        .contains_key(&canonical_test_path(dir.join("project.dawn"))));
    assert_eq!(analysis.diagnostics.len(), 1);
    assert_eq!(analysis.diagnostics[0].code, DiagnosticCode::Import);
    assert_eq!(
        analysis.diagnostics[0].path,
        canonical_test_path(dir.join("project.dawn"))
    );
    assert!(analysis.diagnostics[0].range.is_some());
}

#[test]
fn overlay_content_takes_precedence_over_disk() {
    let dir = temp_dir("overlay-precedence");
    let project_path = dir.join("project.dawn");
    fs::write(&project_path, "not: [valid").unwrap();

    let analysis = analyze_project_with_overlays(
        &project_path,
        None,
        vec![ProjectOverlay {
            path: Utf8PathBuf::from("project.dawn"),
            content: minimal_project("club"),
        }],
    );

    assert!(
        analysis.diagnostics.is_empty(),
        "{:?}",
        analysis.diagnostics
    );
    assert!(analysis.resolved.is_some());
    assert_eq!(analysis.project_key, "club");
}

#[test]
fn layout_document_resolves_imported_fixture_geometry_like_inline_geometry() {
    let dir = temp_dir("layout-document-resolved-geometry");
    let project_path = dir.join("project.dawn");
    let fixture_path = dir.join("fixtures.dawn");
    let layout_path = dir.join("layout.dawn");

    fs::write(
        &project_path,
        project_with_layout_import("layout.dawn::stage"),
    )
    .unwrap();
    fs::write(&fixture_path, pixel_fixture_file()).unwrap();
    fs::write(
        &layout_path,
        r#"
stage:
  type: layout
  name: stage
  units: meters
  target_order:
    - type: fixture
      name: Imported
    - type: fixture
      name: Inline
  fixtures:
    - id: 1
      name: Imported
      fixture:
        import: fixtures.dawn::pixel_bar
      transform:
        position: { x: 0.0, y: 0.0, z: 0.0 }
    - id: 2
      name: Inline
      fixture:
        name: PixelBar
        color_model: rgb
        geometry:
          type: lines
          points:
            - { x: -0.5, y: 0.0, z: 0.0 }
            - { x: 0.5, y: 0.0, z: 0.0 }
          pixels: 50
      transform:
        position: { x: 1.0, y: 0.0, z: 0.0 }
  groups: []
"#,
    )
    .unwrap();

    let document = get_layout_document(&layout_path, "stage", &project_path, Vec::new()).unwrap();
    let imported = &document.fixtures[0].resolved_fixture;
    let inline = &document.fixtures[1].resolved_fixture;

    assert_eq!(imported.name, inline.name);
    assert_eq!(imported.color_model, inline.color_model);
    assert!(matches!(
        imported.geometry,
        Geometry::Lines { pixels: 50, .. }
    ));
    assert_eq!(imported.geometry_summary, inline.geometry_summary);
    assert_eq!(imported.object_key.as_deref(), Some("pixel_bar"));
    assert_eq!(
        document.fixture_catalog[0].import_string,
        "fixtures.dawn::pixel_bar"
    );
}

#[test]
fn layout_document_uses_overlay_analysis_resolver_for_imports() {
    let dir = temp_dir("layout-document-overlay-resolver");
    let project_path = dir.join("project.dawn");
    let fixture_path = dir.join("fixtures.dawn");
    let layout_path = dir.join("layout.dawn");

    fs::write(
        &project_path,
        project_with_layout_import("layout.dawn::stage"),
    )
    .unwrap();
    fs::write(&fixture_path, pixel_fixture_file()).unwrap();
    fs::write(
        &layout_path,
        r#"
stage:
  type: layout
  name: stage
  units: meters
  target_order:
    - type: fixture
      name: Imported
  fixtures:
    - id: 1
      name: Imported
      fixture:
        import: fixtures.dawn::pixel_bar
      transform:
        position: { x: 0.0, y: 0.0, z: 0.0 }
  groups: []
"#,
    )
    .unwrap();

    let overlay = ProjectOverlay {
        path: Utf8PathBuf::from("fixtures.dawn"),
        content: r#"
pixel_bar:
  type: fixture
  name: OverlayBar
  color_model: rgbw
  geometry:
    type: points
    points:
      - { x: 2.0, y: 3.0, z: 4.0 }
"#
        .to_string(),
    };

    let document =
        get_layout_document(&layout_path, "stage", &project_path, vec![overlay]).unwrap();
    let fixture = &document.fixtures[0].resolved_fixture;

    assert_eq!(fixture.name, "OverlayBar");
    assert!(matches!(fixture.geometry, Geometry::Points { ref points } if points.len() == 1));
    assert_eq!(document.fixture_catalog[0].display_name, "OverlayBar");
}

#[test]
fn fixture_documents_include_render_plans_for_points_lines_and_arcs() {
    let dir = temp_dir("fixture-render-plans");
    let fixture_path = dir.join("fixtures.dawn");
    fs::write(
        &fixture_path,
        r#"
point_fixture:
  type: fixture
  name: Point
  color_model: rgb
  geometry:
    type: points
    points:
      - { x: 1.0, y: 2.0, z: 0.0 }
line_fixture:
  type: fixture
  name: Line
  color_model: rgb
  geometry:
    type: lines
    points:
      - { x: 0.0, y: 0.0, z: 0.0 }
      - { x: 2.0, y: 0.0, z: 0.0 }
    pixels: 3
arc_fixture:
  type: fixture
  name: Arc
  color_model: rgb
  geometry:
    type: arc
    center: { x: 0.0, y: 0.0, z: 0.0 }
    radius: 1.0
    startDegrees: 0.0
    endDegrees: 270.0
    pixels: 4
"#,
    )
    .unwrap();

    let document = get_fixture_document(&fixture_path, None, Vec::new()).unwrap();
    let point = document
        .fixtures
        .iter()
        .find(|fixture| fixture.object_key == "point_fixture")
        .unwrap();
    let line = document
        .fixtures
        .iter()
        .find(|fixture| fixture.object_key == "line_fixture")
        .unwrap();
    let arc = document
        .fixtures
        .iter()
        .find(|fixture| fixture.object_key == "arc_fixture")
        .unwrap();

    assert_eq!(point.render_plan.emitters.len(), 1);
    assert_eq!(line.render_plan.emitters.len(), 3);
    assert!(matches!(
        line.render_plan.guides.as_slice(),
        [GeometryRenderGuide::Line { .. }]
    ));
    assert_eq!(arc.render_plan.emitters.len(), 4);
    assert!(matches!(
        arc.render_plan.guides.as_slice(),
        [GeometryRenderGuide::Arc {
            large_arc: true,
            ..
        }]
    ));
}

#[test]
fn layout_documents_include_transformed_render_bounds() {
    let dir = temp_dir("layout-render-bounds");
    let project_path = dir.join("project.dawn");
    let layout_path = dir.join("layout.dawn");
    fs::write(
        &project_path,
        project_with_layout_import("layout.dawn::stage"),
    )
    .unwrap();
    fs::write(
        &layout_path,
        r#"
stage:
  type: layout
  name: stage
  units: meters
  target_order:
    - type: fixture
      name: Pixel
  fixtures:
    - id: 1
      name: Pixel
      fixture:
        name: Pixel
        color_model: rgb
        geometry:
          type: points
          points:
            - { x: 1.0, y: 2.0, z: 0.0 }
      transform:
        position: { x: 10.0, y: 20.0, z: 0.0 }
        scale: { x: 2.0, y: 3.0, z: 1.0 }
  groups: []
"#,
    )
    .unwrap();

    let document = get_layout_document(&layout_path, "stage", &project_path, Vec::new()).unwrap();

    assert!(document.render_bounds.min_x < 12.0);
    assert!(document.render_bounds.max_x > 12.0);
    assert!(document.render_bounds.min_y < 26.0);
    assert!(document.render_bounds.max_y > 26.0);
}

#[test]
fn gui_editor_files_do_not_reintroduce_geometry_helpers() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let viewer_paths = [
        root.join("apps/desktop/frontend/src/ui/GuiEditor.tsx"),
        root.join("apps/desktop/frontend/src/ui/PreviewWindow.tsx"),
    ];
    let forbidden = [
        "sample_polyline_points",
        "sample_arc_points",
        "geometry_bounds",
        "nice_scale_length",
        "format_scale_length",
    ];

    for path in viewer_paths {
        let source = fs::read_to_string(&path).unwrap();
        for needle in forbidden {
            assert!(
                !source.contains(needle),
                "{} reintroduced {needle}",
                path.display()
            );
        }
    }
}

#[test]
fn layout_document_reports_unresolved_fixture_import() {
    let dir = temp_dir("layout-document-missing-import");
    let project_path = dir.join("project.dawn");
    let layout_path = dir.join("layout.dawn");

    fs::write(
        &project_path,
        project_with_layout_import("layout.dawn::stage"),
    )
    .unwrap();
    fs::write(
        &layout_path,
        r#"
stage:
  type: layout
  name: stage
  units: meters
  target_order:
    - type: fixture
      name: Missing
  fixtures:
    - id: 1
      name: Missing
      fixture:
        import: missing.dawn::pixel_bar
      transform:
        position: { x: 0.0, y: 0.0, z: 0.0 }
  groups: []
"#,
    )
    .unwrap();

    let error = get_layout_document(&layout_path, "stage", &project_path, Vec::new())
        .expect_err("missing fixture import should fail layout loading");

    assert!(error.contains("could not load layout `stage`"), "{error}");
    assert!(
        error.contains("failed to resolve import `missing.dawn::pixel_bar`"),
        "{error}"
    );
}

#[test]
fn layout_document_edit_rewrites_only_layout_object_block() {
    let dir = temp_dir("layout-edit-preserves-unrelated");
    let project_path = dir.join("project.dawn");
    let layout_path = dir.join("layout.dawn");
    fs::write(
        &project_path,
        project_with_layout_import("layout.dawn::stage"),
    )
    .unwrap();
    fs::write(
        &layout_path,
        r#"# leading comment
stage:
  type: layout
  name: stage
  units: meters
  target_order: []
  fixtures: []
  groups: []

# fixture comment stays put
pixel:
  type: fixture
  name: Pixel
  color_model: rgb
  geometry:
    type: points
    points:
      - { x: 0.0, y: 0.0, z: 0.0 }
"#,
    )
    .unwrap();
    let base_content = fs::read_to_string(&layout_path).unwrap();
    let mut document =
        get_layout_document(&layout_path, "stage", &project_path, Vec::new()).unwrap();
    let catalog_item = document.fixture_catalog[0].clone();
    document.fixtures.push(LayoutFixturePlacement {
        id: FixtureId(1),
        name: "Pixel 01".to_string(),
        fixture: LayoutFixtureRef::Import {
            import: "layout.dawn::pixel".to_string(),
            object_key: Some("pixel".to_string()),
            source_path: Some(Utf8PathBuf::from("layout.dawn").to_slash_string()),
        },
        resolved_fixture: ResolvedLayoutFixture {
            name: catalog_item.display_name,
            color_model: catalog_item.color_model,
            bulb_size: catalog_item.bulb_size,
            geometry: catalog_item.geometry,
            geometry_summary: catalog_item.geometry_summary,
            render_plan: catalog_item.render_plan,
            source_path: catalog_item.source_path,
            object_key: Some(catalog_item.object_key),
        },
        transform: Transform {
            position: Point3 {
                x: 1.0,
                y: 2.0,
                z: 0.0,
            },
            rotation: Default::default(),
            scale: Default::default(),
        },
    });
    document.target_order.push(LayoutTargetDocument {
        kind: LayoutTargetKind::Fixture,
        name: "Pixel 01".to_string(),
    });

    let result = apply_layout_document_edit(
        &layout_path,
        "stage",
        document,
        base_content,
        Vec::new(),
        &project_path,
        false,
    )
    .unwrap();

    let outcome = result;
    assert!(outcome.serialized_content.contains("# leading comment"));
    assert!(outcome
        .serialized_content
        .contains("# fixture comment stays put"));
    assert!(outcome.serialized_content.contains("id: 1"));
    assert!(outcome.serialized_content.contains("name: Pixel 01"));
}

#[test]
fn layout_document_edit_repairs_group_members_on_rename_and_delete() {
    let dir = temp_dir("layout-edit-repairs-groups");
    let project_path = dir.join("project.dawn");
    let layout_path = dir.join("layout.dawn");
    fs::write(
        &project_path,
        project_with_layout_import("layout.dawn::stage"),
    )
    .unwrap();
    fs::write(
        &layout_path,
        r#"
stage:
  type: layout
  name: stage
  units: meters
  target_order:
    - type: group
      name: all
    - type: fixture
      name: Old
    - type: fixture
      name: Removed
  fixtures:
    - id: 1
      name: Old
      fixture:
        name: Pixel
        color_model: rgb
        geometry:
          type: points
          points:
            - { x: 0.0, y: 0.0, z: 0.0 }
      transform:
        position: { x: 0.0, y: 0.0, z: 0.0 }
    - id: 2
      name: Removed
      fixture:
        name: Pixel
        color_model: rgb
        geometry:
          type: points
          points:
            - { x: 0.0, y: 0.0, z: 0.0 }
      transform:
        position: { x: 1.0, y: 0.0, z: 0.0 }
  groups:
    - name: all
      members: [1, 2]
"#,
    )
    .unwrap();
    let base_content = fs::read_to_string(&layout_path).unwrap();
    let mut document =
        get_layout_document(&layout_path, "stage", &project_path, Vec::new()).unwrap();
    document.fixtures[0].id = FixtureId(3);
    document.fixtures.pop();
    document
        .target_order
        .retain(|target| target.name != "Removed");

    let result = apply_layout_document_edit(
        &layout_path,
        "stage",
        document,
        base_content,
        Vec::new(),
        &project_path,
        false,
    )
    .unwrap();

    let outcome = result;
    assert!(outcome.serialized_content.contains("- 3"));
    assert!(!outcome.serialized_content.contains("removed"));
}

#[test]
fn fixture_document_edit_supports_crud_and_blocks_new_reference_errors() {
    let dir = temp_dir("fixture-edit-crud");
    let project_path = dir.join("project.dawn");
    let fixture_path = dir.join("fixtures.dawn");
    let layout_path = dir.join("layout.dawn");
    fs::write(
        &project_path,
        project_with_layout_import("layout.dawn::stage"),
    )
    .unwrap();
    fs::write(
        &layout_path,
        r#"
stage:
  type: layout
  name: stage
  units: meters
  target_order:
    - type: fixture
      name: Imported
  fixtures:
    - id: 1
      name: Imported
      fixture:
        import: fixtures.dawn::pixel
      transform:
        position: { x: 0.0, y: 0.0, z: 0.0 }
  groups: []
"#,
    )
    .unwrap();
    fs::write(
        &fixture_path,
        r#"# keep me
pixel:
  type: fixture
  name: Pixel
  color_model: rgb
  geometry:
    type: points
    points:
      - { x: 0.0, y: 0.0, z: 0.0 }
"#,
    )
    .unwrap();
    let base_content = fs::read_to_string(&fixture_path).unwrap();
    let document = FixtureDocument {
        path: Utf8PathBuf::from("fixtures.dawn").to_slash_string(),
        selected_object_key: Some("arc".to_string()),
        fixtures: vec![FixtureDefinitionDocument {
            object_key: "arc".to_string(),
            name: "Arc".to_string(),
            color_model: ColorModel::Rgb,
            bulb_size: 1.0,
            geometry: Geometry::Arc {
                center: Point3::default(),
                radius: 1.0,
                start_degrees: 0.0,
                end_degrees: 180.0,
                pixels: 8,
            },
            geometry_summary: String::new(),
            render_plan: empty_render_plan(),
        }],
    };

    let outcome = apply_fixture_document_edit(
        &fixture_path,
        document,
        base_content,
        Vec::new(),
        &project_path,
        true,
    )
    .unwrap();
    assert!(outcome.serialized_content.contains("# keep me"));
    assert!(outcome.serialized_content.contains("bulb_size: 1.0"));
    assert!(outcome.serialized_content.contains("type: arc"));
    assert_eq!(
        get_fixture_document(
            &fixture_path,
            Some("arc"),
            vec![ProjectOverlay {
                path: Utf8PathBuf::from("fixtures.dawn"),
                content: outcome.serialized_content,
            }]
        )
        .unwrap()
        .fixtures
        .len(),
        1
    );
}

#[test]
fn fixture_document_edit_serializes_lines_without_legacy_fields() {
    let dir = temp_dir("fixture-edit-lines-serialization");
    let project_path = dir.join("project.dawn");
    let fixture_path = dir.join("fixtures.dawn");
    fs::write(
        &project_path,
        r#"
club:
  type: project
  name: club
  display:
    name: main
    controllers: []
    patch:
      routes: []
    layout:
      name: stage
      units: meters
      fixtures: []
      groups: []
"#,
    )
    .unwrap();
    fs::write(
        &fixture_path,
        r#"
pixel_bar:
  type: fixture
  name: PixelBar
  color_model: rgb
  geometry:
    type: points
    points: []
"#,
    )
    .unwrap();
    let base_content = fs::read_to_string(&fixture_path).unwrap();
    let document = FixtureDocument {
        path: Utf8PathBuf::from("fixtures.dawn").to_slash_string(),
        selected_object_key: Some("pixel_bar".to_string()),
        fixtures: vec![FixtureDefinitionDocument {
            object_key: "pixel_bar".to_string(),
            name: "PixelBar".to_string(),
            color_model: ColorModel::Rgb,
            bulb_size: 1.0,
            geometry: Geometry::Lines {
                points: vec![
                    Point3 {
                        x: -0.5,
                        y: 0.0,
                        z: 0.0,
                    },
                    Point3 {
                        x: 0.5,
                        y: 0.0,
                        z: 0.0,
                    },
                ],
                pixels: 50,
            },
            geometry_summary: String::new(),
            render_plan: empty_render_plan(),
        }],
    };

    let result = apply_fixture_document_edit(
        &fixture_path,
        document,
        base_content,
        Vec::new(),
        &project_path,
        false,
    )
    .unwrap();

    let outcome = result;
    assert!(outcome.serialized_content.contains("type: lines"));
    assert!(outcome.serialized_content.contains("points:"));
    assert!(outcome.serialized_content.contains("pixels: 50"));
    assert!(!outcome.serialized_content.contains("type: line\n"));
    assert!(!outcome.serialized_content.contains("from:"));
    assert!(!outcome.serialized_content.contains("to:"));
    assert!(!outcome.serialized_content.contains("lines:"));
}

#[test]
fn rejects_legacy_line_geometry() {
    let dir = temp_dir("legacy-line-geometry");
    let project_path = dir.join("project.dawn");
    fs::write(
        &project_path,
        r#"
club:
  type: project
  name: club
  display:
    name: main
    controllers: []
    patch:
      routes: []
    layout:
      name: stage
      units: meters
      fixtures:
        - id: 1
          name: Bar
          fixture:
            name: PixelBar
            color_model: rgb
            geometry:
              type: line
              from: { x: -0.5, y: 0.0, z: 0.0 }
              to: { x: 0.5, y: 0.0, z: 0.0 }
              pixels: 50
          transform:
            position: { x: 0.0, y: 0.0, z: 0.0 }
      groups: []
"#,
    )
    .unwrap();

    let analysis = analyze_project(&project_path, "club");

    assert!(analysis.resolved.is_none());
    assert_eq!(analysis.diagnostics.len(), 1);
    assert_eq!(analysis.diagnostics[0].code, DiagnosticCode::Yaml);
}

#[test]
fn rejects_legacy_lines_segment_list() {
    let dir = temp_dir("legacy-lines-segments");
    let project_path = dir.join("project.dawn");
    fs::write(
        &project_path,
        r#"
club:
  type: project
  name: club
  display:
    name: main
    controllers: []
    patch:
      routes: []
    layout:
      name: stage
      units: meters
      fixtures:
        - id: 1
          name: Bar
          fixture:
            name: PixelBar
            color_model: rgb
            geometry:
              type: lines
              points:
                - { x: -0.5, y: 0.0, z: 0.0 }
                - { x: 0.5, y: 0.0, z: 0.0 }
              lines:
                - { from: 0, to: 1 }
          transform:
            position: { x: 0.0, y: 0.0, z: 0.0 }
      groups: []
"#,
    )
    .unwrap();

    let analysis = analyze_project(&project_path, "club");

    assert!(analysis.resolved.is_none());
    assert_eq!(analysis.diagnostics.len(), 1);
    assert_eq!(analysis.diagnostics[0].code, DiagnosticCode::Yaml);
}

#[test]
fn infers_single_root_project_key() {
    let dir = temp_dir("infer-project-key");
    let project_path = dir.join("project.dawn");
    fs::write(&project_path, minimal_project("club")).unwrap();

    let analysis = analyze_project_with_overlays(&project_path, None, Vec::new());

    assert!(
        analysis.diagnostics.is_empty(),
        "{:?}",
        analysis.diagnostics
    );
    assert!(analysis.resolved.is_some());
    assert_eq!(analysis.project_key, "club");
}

#[test]
fn reports_zero_project_objects_when_inference_is_ambiguous() {
    let dir = temp_dir("zero-projects");
    let project_path = dir.join("project.dawn");
    fs::write(
        &project_path,
        r#"
fixture:
  type: fixture
  name: Pixel
  color_model: rgb
  geometry:
    type: points
    points: []
"#,
    )
    .unwrap();

    let analysis = analyze_project_with_overlays(&project_path, None, Vec::new());

    assert!(analysis.resolved.is_none());
    assert_eq!(analysis.diagnostics.len(), 1);
    assert_eq!(analysis.diagnostics[0].code, DiagnosticCode::ProjectKey);
    assert!(analysis.diagnostics[0].message.contains("found none"));
}

#[test]
fn reports_multiple_project_objects_when_inference_is_ambiguous() {
    let dir = temp_dir("multiple-projects");
    let project_path = dir.join("project.dawn");
    fs::write(
        &project_path,
        format!("{}{}", minimal_project("club"), minimal_project("backup")),
    )
    .unwrap();

    let analysis = analyze_project_with_overlays(&project_path, None, Vec::new());

    assert!(analysis.resolved.is_none());
    assert_eq!(analysis.diagnostics.len(), 1);
    assert_eq!(analysis.diagnostics[0].code, DiagnosticCode::ProjectKey);
    assert!(analysis.diagnostics[0].message.contains("found 2"));
}

#[test]
fn source_relative_imports_resolve_inside_nested_directories() {
    let dir = temp_dir("nested-imports");
    fs::create_dir_all(dir.join("shows")).unwrap();
    fs::write(
        dir.join("project.dawn"),
        r#"
club:
  type: project
  name: club
  display:
    import: shows/display.dawn::main
"#,
    )
    .unwrap();
    fs::write(
        dir.join("shows/display.dawn"),
        r#"
main:
  type: display
  name: main
  controllers: []
  patch:
    routes: []
  layout:
    import: layout.dawn::stage
"#,
    )
    .unwrap();
    fs::write(
        dir.join("shows/layout.dawn"),
        r#"
stage:
  type: layout
  name: stage
  units: meters
  fixtures: []
  groups: []
"#,
    )
    .unwrap();

    let analysis = analyze_project(dir.join("project.dawn"), "club");

    assert!(
        analysis.diagnostics.is_empty(),
        "{:?}",
        analysis.diagnostics
    );
    assert!(analysis
        .files
        .contains_key(&canonical_test_path(dir.join("shows/layout.dawn"))));
    assert!(analysis.resolved.is_some());
}

#[test]
fn absolute_imports_are_allowed() {
    let dir = temp_dir("absolute-import");
    let display_path = dir.join("display.dawn");
    fs::write(
        &display_path,
        r#"
main:
  type: display
  name: main
  controllers: []
  patch:
    routes: []
  layout:
    name: stage
    units: meters
    fixtures: []
    groups: []
"#,
    )
    .unwrap();
    fs::write(
        dir.join("project.dawn"),
        format!(
            r#"
club:
  type: project
  name: club
  display:
    import: "{}::main"
"#,
            display_path.to_string_lossy().replace('\\', "/")
        ),
    )
    .unwrap();

    let analysis = analyze_project(dir.join("project.dawn"), "club");

    assert!(
        analysis.diagnostics.is_empty(),
        "{:?}",
        analysis.diagnostics
    );
    assert!(analysis.resolved.is_some());
}

#[test]
fn escaping_relative_imports_are_allowed_when_target_exists() {
    let dir = temp_dir("escaping-import");
    let outside_display = dir.with_file_name(format!(
        "{}-display.dawn",
        dir.file_name().unwrap().to_string_lossy()
    ));
    fs::write(
        &outside_display,
        r#"
main:
  type: display
  name: main
  controllers: []
  patch:
    routes: []
  layout:
    name: stage
    units: meters
    fixtures: []
    groups: []
"#,
    )
    .unwrap();
    fs::write(
        dir.join("project.dawn"),
        format!(
            r#"
club:
  type: project
  name: club
  display:
    import: ../{}::main
"#,
            outside_display.file_name().unwrap().to_string_lossy()
        ),
    )
    .unwrap();

    let analysis = analyze_project(dir.join("project.dawn"), "club");

    assert!(
        analysis.diagnostics.is_empty(),
        "{:?}",
        analysis.diagnostics
    );
    assert!(analysis.resolved.is_some());
}

#[test]
fn sequence_assets_resolve_outside_project() {
    let dir = temp_dir("escaping-sequence-assets");
    fs::write(
        dir.join("project.dawn"),
        r#"
club:
  type: project
  name: club
  display:
    name: main
    controllers: []
    patch:
      routes: []
    layout:
      name: stage
      units: meters
      fixtures: []
      groups: []
  sequences:
    - duration: 1s
      frame_rate: 60
      audio: ../song.wav
      effects: []
"#,
    )
    .unwrap();

    let analysis = analyze_project(dir.join("project.dawn"), "club");

    assert!(
        analysis.diagnostics.is_empty(),
        "{:?}",
        analysis.diagnostics
    );
    assert!(analysis.resolved.is_some());
}

#[test]
fn utf8_paths_allow_absolute_and_escaping_paths() {
    assert!(utf8_path(std::env::temp_dir().join("project.dawn"))
        .unwrap()
        .is_absolute());
    assert_eq!(
        Utf8PathBuf::from("../project.dawn").to_slash_string(),
        "../project.dawn"
    );
}

fn minimal_project(key: &str) -> String {
    format!(
        r#"
{key}:
  type: project
  name: {key}
  display:
    name: main
    controllers: []
    patch:
      routes: []
    layout:
      name: stage
      units: meters
      fixtures: []
      groups: []
"#
    )
}

fn project_with_inline_sequence(effects: &str) -> String {
    format!(
        r#"
club:
  type: project
  name: club
  display:
    name: main
    controllers: []
    patch:
      routes: []
    layout:
      name: stage
      units: meters
      target_order:
        - type: group
          name: all
      fixtures: []
      groups:
        - name: all
          members: []
  sequences:
    - duration: 1s
      frame_rate: 60
      audio:
      effects:
{effects}
"#
    )
}

fn project_with_layout_import(import: &str) -> String {
    format!(
        r#"
club:
  type: project
  name: club
  display:
    name: main
    controllers: []
    patch:
      routes: []
    layout:
      import: {import}
"#
    )
}

fn pixel_fixture_file() -> &'static str {
    r#"
pixel_bar:
  type: fixture
  name: PixelBar
  color_model: rgb
  geometry:
    type: lines
    points:
      - { x: -0.5, y: 0.0, z: 0.0 }
      - { x: 0.5, y: 0.0, z: 0.0 }
    pixels: 50
"#
}

fn temp_dir(label: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let path = std::env::temp_dir().join(format!("dawn-project-{label}-{nanos}"));
    fs::create_dir_all(&path).unwrap();
    path
}

fn empty_render_plan() -> GeometryRenderPlan {
    GeometryRenderPlan {
        emitters: Vec::new(),
        guides: Vec::new(),
        bounds: GeometryRenderBounds {
            min_x: -1.0,
            min_y: -1.0,
            max_x: 1.0,
            max_y: 1.0,
        },
        bulb_radius: 0.035,
    }
}
