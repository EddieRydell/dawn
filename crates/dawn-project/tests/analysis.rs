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
    get_fixture_document as core_get_fixture_document,
    get_layout_document as core_get_layout_document, DocumentEditResult, FixtureDefinitionDocument,
    FixtureDocument, LayoutDocument, LayoutFixturePlacement, LayoutFixtureRef,
    ResolvedLayoutFixture,
};
use dawn_project::fs::ProjectFs;
use dawn_project::model::{ColorModel, Geometry, Point3, Transform};
use dawn_project::path::ProjectPath;
use dawn_project::render::{GeometryRenderBounds, GeometryRenderGuide, GeometryRenderPlan};

fn project_context(project_path: impl AsRef<Path>) -> (ProjectFs, ProjectPath, PathBuf) {
    let project_path = project_path.as_ref();
    let root = project_path
        .parent()
        .expect("test project path should have a parent")
        .to_path_buf();
    let fs = ProjectFs::open_ambient(&root).expect("test project root should open");
    let relative = relative_project_path(&root, project_path);
    (fs, relative, root)
}

fn relative_project_path(root: &Path, path: &Path) -> ProjectPath {
    ProjectPath::parse(
        path.strip_prefix(root)
            .expect("test path should be inside project root"),
    )
    .expect("test path should parse as project-relative")
}

fn normalize_overlays(root: &Path, overlays: Vec<ProjectOverlay>) -> Vec<ProjectOverlay> {
    overlays
        .into_iter()
        .map(|overlay| ProjectOverlay {
            path: if overlay.path.as_path().is_absolute() {
                relative_project_path(root, overlay.path.as_path())
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
    let fs = ProjectFs::open_ambient(root).expect("test project root should open");
    let path = relative_project_path(root, path.as_ref());
    core_get_fixture_document(
        &fs,
        path,
        selected_object_key,
        normalize_overlays(root, overlays),
    )
}

fn apply_layout_document_edit(
    path: impl AsRef<Path>,
    object_key: &str,
    document: LayoutDocument,
    base_content: String,
    overlays: Vec<ProjectOverlay>,
    project_path: impl AsRef<Path>,
    allow_breaking_references: bool,
) -> Result<DocumentEditResult<LayoutDocument>, String> {
    let (fs, project_path, root) = project_context(project_path);
    let path = relative_project_path(&root, path.as_ref());
    core_apply_layout_document_edit(
        &fs,
        path,
        object_key,
        document,
        base_content,
        normalize_overlays(&root, overlays),
        project_path,
        allow_breaking_references,
    )
}

fn apply_fixture_document_edit(
    path: impl AsRef<Path>,
    document: FixtureDocument,
    base_content: String,
    overlays: Vec<ProjectOverlay>,
    project_path: impl AsRef<Path>,
    allow_breaking_references: bool,
) -> Result<DocumentEditResult<FixtureDocument>, String> {
    let (fs, project_path, root) = project_context(project_path);
    let path = relative_project_path(&root, path.as_ref());
    core_apply_fixture_document_edit(
        &fs,
        path,
        document,
        base_content,
        normalize_overlays(&root, overlays),
        project_path,
        allow_breaking_references,
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
        .contains_key(&ProjectPath::new("project.dawn")));
    assert_eq!(analysis.diagnostics.len(), 1);
    assert_eq!(analysis.diagnostics[0].code, DiagnosticCode::Import);
    assert_eq!(
        analysis.diagnostics[0].path,
        ProjectPath::new("project.dawn")
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
            path: ProjectPath::new("project.dawn"),
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
        path: ProjectPath::new("fixtures.dawn"),
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
  fixtures:
    - id: pixel
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
fn panel_files_do_not_reintroduce_geometry_helpers() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let viewer_paths = [
        root.join("apps/desktop/src/ui/layout_viewer.rs"),
        root.join("apps/desktop/src/ui/fixture_viewer.rs"),
    ];
    let forbidden = [
        "sample_polyline_points",
        "sample_arc_points",
        "bulb_radius",
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
    document.fixtures.push(LayoutFixturePlacement {
        id: "pixel_01".to_string(),
        fixture: LayoutFixtureRef::Import {
            import: "layout.dawn::pixel".to_string(),
            object_key: Some("pixel".to_string()),
            source_path: Some(ProjectPath::new("layout.dawn").to_slash_string()),
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
        path: ProjectPath::new("fixtures.dawn").to_slash_string(),
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
                path: ProjectPath::new("fixtures.dawn"),
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
        path: ProjectPath::new("fixtures.dawn").to_slash_string(),
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
        .contains_key(&ProjectPath::new("shows/layout.dawn")));
    assert!(analysis.resolved.is_some());
}

#[test]
fn absolute_imports_are_rejected_as_project_containment_errors() {
    let dir = temp_dir("absolute-import");
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
            dir.join("display.dawn")
                .to_string_lossy()
                .replace('\\', "/")
        ),
    )
    .unwrap();

    let analysis = analyze_project(dir.join("project.dawn"), "club");

    assert!(analysis.resolved.is_none());
    assert_eq!(analysis.diagnostics[0].code, DiagnosticCode::Import);
    assert!(analysis.diagnostics[0]
        .message
        .contains("absolute imports are not allowed"));
}

#[test]
fn escaping_relative_imports_are_rejected() {
    let dir = temp_dir("escaping-import");
    fs::write(
        dir.join("project.dawn"),
        r#"
club:
  type: project
  name: club
  display:
    import: ../display.dawn::main
"#,
    )
    .unwrap();

    let analysis = analyze_project(dir.join("project.dawn"), "club");

    assert!(analysis.resolved.is_none());
    assert_eq!(analysis.diagnostics[0].code, DiagnosticCode::Import);
    assert!(analysis.diagnostics[0]
        .message
        .contains("escapes the project root"));
}

#[test]
fn sequence_assets_cannot_resolve_outside_project() {
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

    assert!(analysis.resolved.is_none());
    assert_eq!(analysis.diagnostics[0].code, DiagnosticCode::Lower);
    assert!(analysis.diagnostics[0]
        .message
        .contains("escapes the project root"));
}

#[test]
fn document_path_parsing_rejects_absolute_and_escaping_paths() {
    assert!(ProjectPath::parse("/tmp/project.dawn").is_err());
    assert!(ProjectPath::parse("../project.dawn").is_err());
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
