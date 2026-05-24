use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use dawn_project::{
    analyze_project, analyze_project_with_overlays, DiagnosticCode, ProjectOverlay,
};

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
    assert_eq!(project.display.layout.fixtures.len(), 8);
    assert_eq!(project.display.patch.routes.len(), 8);
    assert_eq!(project.sequences[0].effects.len(), 2);
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
      fixtures:
        - id: bar_01
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
        - id: fx
          start: 0s
          duration: 1s
          target:
            type: group
            name: MissingGroup
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
    assert!(analysis.files.contains_key(&project_path));
    assert_eq!(analysis.diagnostics.len(), 1);
    assert_eq!(analysis.diagnostics[0].code, DiagnosticCode::Import);
    assert_eq!(analysis.diagnostics[0].path, project_path);
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
            path: project_path.clone(),
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

fn temp_dir(label: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let path = std::env::temp_dir().join(format!("dawn-project-{label}-{nanos}"));
    fs::create_dir_all(&path).unwrap();
    path
}
