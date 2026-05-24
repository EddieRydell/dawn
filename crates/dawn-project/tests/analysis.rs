use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use dawn_project::{
    analyze_project, analyze_project_with_overlays, apply_fixture_document_edit,
    apply_layout_document_edit, get_fixture_document, get_layout_document, DiagnosticCode,
    DocumentEditResult, FixtureDefinitionDocument, FixtureDocument, Geometry, ProjectOverlay,
    ProjectPath,
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
    assert!(project.display.layout.fixtures.len() >= 8);
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
    assert!(analysis
        .files
        .contains_key(&ProjectPath::new(&project_path)));
    assert_eq!(analysis.diagnostics.len(), 1);
    assert_eq!(analysis.diagnostics[0].code, DiagnosticCode::Import);
    assert_eq!(
        analysis.diagnostics[0].path,
        ProjectPath::new(&project_path)
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
            path: ProjectPath::new(&project_path),
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
  fixtures:
    - id: imported
      fixture:
        import: fixtures.dawn::pixel_bar
      transform:
        position: { x: 0.0, y: 0.0, z: 0.0 }
    - id: inline
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
  fixtures:
    - id: imported
      fixture:
        import: fixtures.dawn::pixel_bar
      transform:
        position: { x: 0.0, y: 0.0, z: 0.0 }
  groups: []
"#,
    )
    .unwrap();

    let overlay = ProjectOverlay {
        path: ProjectPath::new(&fixture_path),
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
  fixtures:
    - id: missing
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
    document
        .fixtures
        .push(dawn_project::LayoutFixturePlacement {
            id: "pixel_01".to_string(),
            fixture: dawn_project::LayoutFixtureRef::Import {
                import: "layout.dawn::pixel".to_string(),
                object_key: Some("pixel".to_string()),
                source_path: Some(ProjectPath::new(&layout_path).to_slash_string()),
            },
            resolved_fixture: dawn_project::ResolvedLayoutFixture {
                name: catalog_item.display_name,
                color_model: catalog_item.color_model,
                bulb_size: catalog_item.bulb_size,
                geometry: catalog_item.geometry,
                geometry_summary: catalog_item.geometry_summary,
                source_path: catalog_item.source_path,
                object_key: Some(catalog_item.object_key),
            },
            transform: dawn_project::Transform {
                position: dawn_project::Point3 {
                    x: 1.0,
                    y: 2.0,
                    z: 0.0,
                },
                rotation: Default::default(),
                scale: Default::default(),
            },
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

    let DocumentEditResult::Applied(outcome) = result else {
        panic!("layout edit should apply");
    };
    assert!(outcome.serialized_content.contains("# leading comment"));
    assert!(outcome
        .serialized_content
        .contains("# fixture comment stays put"));
    assert!(outcome.serialized_content.contains("id: pixel_01"));
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
  fixtures:
    - id: old
      fixture:
        name: Pixel
        color_model: rgb
        geometry:
          type: points
          points:
            - { x: 0.0, y: 0.0, z: 0.0 }
      transform:
        position: { x: 0.0, y: 0.0, z: 0.0 }
    - id: removed
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
      members: [old, removed]
"#,
    )
    .unwrap();
    let base_content = fs::read_to_string(&layout_path).unwrap();
    let mut document =
        get_layout_document(&layout_path, "stage", &project_path, Vec::new()).unwrap();
    document.fixtures[0].id = "new_id".to_string();
    document.fixtures.pop();

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

    let DocumentEditResult::Applied(outcome) = result else {
        panic!("layout edit should apply");
    };
    assert!(outcome.serialized_content.contains("- new_id"));
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
  fixtures:
    - id: imported
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
        path: ProjectPath::new(&fixture_path).to_slash_string(),
        selected_object_key: Some("arc".to_string()),
        fixtures: vec![FixtureDefinitionDocument {
            object_key: "arc".to_string(),
            name: "Arc".to_string(),
            color_model: dawn_project::ColorModel::Rgb,
            bulb_size: 1.0,
            geometry: Geometry::Arc {
                center: dawn_project::Point3::default(),
                radius: 1.0,
                start_degrees: 0.0,
                end_degrees: 180.0,
                pixels: 8,
            },
            geometry_summary: String::new(),
        }],
    };

    let blocked = apply_fixture_document_edit(
        &fixture_path,
        document.clone(),
        base_content.clone(),
        Vec::new(),
        &project_path,
        false,
    )
    .unwrap();
    assert!(matches!(blocked, DocumentEditResult::Blocked(_)));

    let applied = apply_fixture_document_edit(
        &fixture_path,
        document,
        base_content,
        Vec::new(),
        &project_path,
        true,
    )
    .unwrap();
    let DocumentEditResult::Applied(outcome) = applied else {
        panic!("fixture edit should apply with override");
    };
    assert!(outcome.serialized_content.contains("# keep me"));
    assert!(outcome.serialized_content.contains("bulb_size: 1.0"));
    assert!(outcome.serialized_content.contains("type: arc"));
    assert_eq!(
        get_fixture_document(
            &fixture_path,
            Some("arc"),
            vec![ProjectOverlay {
                path: ProjectPath::new(&fixture_path),
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
        path: ProjectPath::new(&fixture_path).to_slash_string(),
        selected_object_key: Some("pixel_bar".to_string()),
        fixtures: vec![FixtureDefinitionDocument {
            object_key: "pixel_bar".to_string(),
            name: "PixelBar".to_string(),
            color_model: dawn_project::ColorModel::Rgb,
            bulb_size: 1.0,
            geometry: Geometry::Lines {
                points: vec![
                    dawn_project::Point3 {
                        x: -0.5,
                        y: 0.0,
                        z: 0.0,
                    },
                    dawn_project::Point3 {
                        x: 0.5,
                        y: 0.0,
                        z: 0.0,
                    },
                ],
                pixels: 50,
            },
            geometry_summary: String::new(),
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

    let DocumentEditResult::Applied(outcome) = result else {
        panic!("fixture edit should apply");
    };
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
        - id: bar
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
        - id: bar
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
