use std::collections::{BTreeMap, HashMap, HashSet};

use serde::{Deserialize, Serialize};

use crate::analysis::{
    analyze_project_with_overlays, AnalysisImportResolver, DiagnosticCode, DiagnosticSeverity,
    ProjectAnalysis, ProjectDiagnostic, ProjectOverlay,
};
use crate::fs::ProjectFs;
use crate::lower::lower_layout;
use crate::model::*;
use crate::path::{relative_import_path, resolve_import_file_path, ProjectPath};
use crate::render::{
    geometry_render_plan, geometry_summary, layout_render_bounds, GeometryRenderBounds,
    GeometryRenderPlan,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DocumentViewId {
    Text,
    Layout,
    Fixture,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DocumentDescriptor {
    pub path: String,
    pub objects: Vec<DocumentObjectDescriptor>,
    pub available_views: Vec<DocumentViewId>,
    pub default_object_keys: BTreeMap<DocumentViewId, String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DocumentObjectDescriptor {
    pub key: String,
    pub kind: ObjectKind,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LayoutDocument {
    pub path: String,
    pub object_key: String,
    pub name: String,
    pub units: DistanceUnit,
    pub render_bounds: GeometryRenderBounds,
    pub fixtures: Vec<LayoutFixturePlacement>,
    pub groups: Vec<LayoutGroupDocument>,
    pub fixture_catalog: Vec<FixtureCatalogItem>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LayoutFixturePlacement {
    pub id: String,
    pub fixture: LayoutFixtureRef,
    pub resolved_fixture: ResolvedLayoutFixture,
    pub transform: Transform,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum LayoutFixtureRef {
    Import {
        import: String,
        object_key: Option<String>,
        source_path: Option<String>,
    },
    Inline {
        name: String,
        color_model: ColorModel,
        bulb_size: f64,
        geometry: Geometry,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LayoutGroupDocument {
    pub name: String,
    pub members: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResolvedLayoutFixture {
    pub name: String,
    pub color_model: ColorModel,
    pub bulb_size: f64,
    pub geometry: Geometry,
    pub geometry_summary: String,
    pub render_plan: GeometryRenderPlan,
    pub source_path: String,
    pub object_key: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FixtureCatalogItem {
    pub object_key: String,
    pub source_path: String,
    pub import_string: String,
    pub display_name: String,
    pub color_model: ColorModel,
    pub bulb_size: f64,
    pub geometry: Geometry,
    pub geometry_summary: String,
    pub render_plan: GeometryRenderPlan,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FixtureDocument {
    pub path: String,
    pub selected_object_key: Option<String>,
    pub fixtures: Vec<FixtureDefinitionDocument>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FixtureDefinitionDocument {
    pub object_key: String,
    pub name: String,
    pub color_model: ColorModel,
    pub bulb_size: f64,
    pub geometry: Geometry,
    pub geometry_summary: String,
    pub render_plan: GeometryRenderPlan,
}

#[derive(Debug, Clone)]
pub struct DocumentEditOutcome<T> {
    pub serialized_content: String,
    pub analysis: ProjectAnalysis,
    pub refreshed_document: T,
}

#[derive(Debug, Clone)]
pub struct BlockedDocumentEdit {
    pub diagnostics: Vec<ProjectDiagnostic>,
    pub message: String,
}

#[derive(Debug, Clone)]
pub enum DocumentEditResult<T> {
    Applied(DocumentEditOutcome<T>),
    Blocked(BlockedDocumentEdit),
}

pub fn inspect_document(
    fs: &ProjectFs,
    path: ProjectPath,
    overlays: Vec<ProjectOverlay>,
) -> Result<DocumentDescriptor, String> {
    let text = read_text_with_overlays(fs, &path, &overlays)?;
    let file: DawnFile = serde_yaml::from_str(&text).map_err(|error| error.to_string())?;
    let objects = file
        .iter()
        .map(|(key, object)| DocumentObjectDescriptor {
            key: key.clone(),
            kind: object.kind(),
        })
        .collect::<Vec<_>>();
    let mut available_views = vec![DocumentViewId::Text];
    let mut default_object_keys = BTreeMap::new();
    if let Some(key) = file.iter().find_map(|(key, object)| match object {
        DawnObject::Layout(_) => Some(key.clone()),
        _ => None,
    }) {
        available_views.push(DocumentViewId::Layout);
        default_object_keys.insert(DocumentViewId::Layout, key);
    }
    if let Some(key) = file.iter().find_map(|(key, object)| match object {
        DawnObject::Fixture(_) => Some(key.clone()),
        _ => None,
    }) {
        available_views.push(DocumentViewId::Fixture);
        default_object_keys.insert(DocumentViewId::Fixture, key);
    }

    Ok(DocumentDescriptor {
        path: path.to_slash_string(),
        objects,
        available_views,
        default_object_keys,
    })
}

pub fn get_fixture_document(
    fs: &ProjectFs,
    path: ProjectPath,
    selected_object_key: Option<&str>,
    overlays: Vec<ProjectOverlay>,
) -> Result<FixtureDocument, String> {
    let text = read_text_with_overlays(fs, &path, &overlays)?;
    let file: DawnFile = serde_yaml::from_str(&text).map_err(|error| error.to_string())?;
    let fixtures = file
        .iter()
        .filter_map(|(key, object)| {
            let DawnObject::Fixture(fixture) = object else {
                return None;
            };
            Some(fixture_to_document(key, fixture))
        })
        .collect::<Vec<_>>();
    let selected_object_key = selected_object_key
        .filter(|key| fixtures.iter().any(|fixture| fixture.object_key == *key))
        .map(str::to_string)
        .or_else(|| fixtures.first().map(|fixture| fixture.object_key.clone()));

    Ok(FixtureDocument {
        path: path.to_slash_string(),
        selected_object_key,
        fixtures,
    })
}

pub fn get_layout_document(
    fs: &ProjectFs,
    path: ProjectPath,
    object_key: &str,
    project_path: ProjectPath,
    overlays: Vec<ProjectOverlay>,
) -> Result<LayoutDocument, String> {
    let analysis = analyze_project_with_overlays(fs, project_path, None, overlays.clone());
    if let Some(diagnostic) = analysis.diagnostics.iter().find(|diagnostic| {
        diagnostic.severity == DiagnosticSeverity::Error
            && diagnostic.path == path
            && matches!(diagnostic.code, DiagnosticCode::Io | DiagnosticCode::Yaml)
    }) {
        return Err(format!(
            "could not load layout `{object_key}`: {}",
            diagnostic.message
        ));
    }

    let text = read_text_with_overlays(fs, &path, &overlays)?;
    let file: DawnFile = serde_yaml::from_str(&text).map_err(|error| error.to_string())?;
    let object = file
        .get(object_key)
        .ok_or_else(|| format!("layout object `{object_key}` was not found"))?;
    let DawnObject::Layout(layout) = object else {
        return Err(format!("object `{object_key}` is not a layout"));
    };
    let catalog = fixture_catalog_from_analysis(&analysis, &path);
    let mut resolver = AnalysisImportResolver {
        files: &analysis.files,
    };
    let resolved_layout = lower_layout(layout, &path, &mut |source_path, import, expected| {
        resolver.resolve(source_path, import, expected)
    })
    .map_err(|error| format!("could not load layout `{object_key}`: {error}"))?;

    layout_to_document(&path, object_key, layout, &resolved_layout, &catalog)
}

pub fn apply_layout_document_edit(
    fs: &ProjectFs,
    path: ProjectPath,
    object_key: &str,
    document: LayoutDocument,
    base_content: String,
    overlays: Vec<ProjectOverlay>,
    project_path: ProjectPath,
    allow_breaking_references: bool,
) -> Result<DocumentEditResult<LayoutDocument>, String> {
    let file: DawnFile = serde_yaml::from_str(&base_content).map_err(|error| error.to_string())?;
    let Some(DawnObject::Layout(current_layout)) = file.get(object_key) else {
        return Err(format!("layout object `{object_key}` was not found"));
    };
    let mut layout = document_to_layout(document)?;
    repair_layout_group_members(current_layout, &mut layout);
    validate_layout_identifiers(&layout)?;
    let object = DawnObject::Layout(layout);
    let serialized = replace_top_level_object(&base_content, object_key, &object)?;
    let next_overlays = overlay_after_save(path.clone(), serialized.clone(), overlays.clone());
    let analysis = analyze_project_with_overlays(fs, project_path.clone(), None, next_overlays);
    let introduced_errors = introduced_error_diagnostics(
        &analyze_project_with_overlays(
            fs,
            project_path.clone(),
            None,
            overlay_after_save(path.clone(), base_content, overlays),
        ),
        &analysis,
    );
    if !allow_breaking_references && !introduced_errors.is_empty() {
        return Ok(DocumentEditResult::Blocked(BlockedDocumentEdit {
            diagnostics: introduced_errors,
            message: "This edit introduces project reference errors.".to_string(),
        }));
    }

    let refreshed_document = get_layout_document(
        fs,
        path.clone(),
        object_key,
        analysis.root_path.clone(),
        vec![ProjectOverlay {
            path: path.clone(),
            content: serialized.clone(),
        }],
    )?;
    Ok(DocumentEditResult::Applied(DocumentEditOutcome {
        serialized_content: serialized,
        analysis,
        refreshed_document,
    }))
}

pub fn apply_fixture_document_edit(
    fs: &ProjectFs,
    path: ProjectPath,
    document: FixtureDocument,
    base_content: String,
    overlays: Vec<ProjectOverlay>,
    project_path: ProjectPath,
    allow_breaking_references: bool,
) -> Result<DocumentEditResult<FixtureDocument>, String> {
    validate_fixture_document(&document)?;
    let file: DawnFile = serde_yaml::from_str(&base_content).map_err(|error| error.to_string())?;
    let mut replacements = BTreeMap::new();
    for (key, object) in &file {
        if matches!(object, DawnObject::Fixture(_)) {
            replacements.insert(key.clone(), None);
        }
    }
    for fixture in &document.fixtures {
        replacements.insert(
            fixture.object_key.clone(),
            Some(serialize_top_level_object(
                &fixture.object_key,
                &DawnObject::Fixture(document_to_fixture(fixture)),
            )?),
        );
    }
    let serialized = replace_top_level_objects(&base_content, replacements)?;
    let next_overlays = overlay_after_save(path.clone(), serialized.clone(), overlays.clone());
    let analysis = analyze_project_with_overlays(fs, project_path.clone(), None, next_overlays);
    let introduced_errors = introduced_error_diagnostics(
        &analyze_project_with_overlays(
            fs,
            project_path,
            None,
            overlay_after_save(path.clone(), base_content, overlays),
        ),
        &analysis,
    );
    if !allow_breaking_references && !introduced_errors.is_empty() {
        return Ok(DocumentEditResult::Blocked(BlockedDocumentEdit {
            diagnostics: introduced_errors,
            message: "This edit introduces project reference errors.".to_string(),
        }));
    }

    let refreshed_document = get_fixture_document(
        fs,
        path.clone(),
        document.selected_object_key.as_deref(),
        vec![ProjectOverlay {
            path: path.clone(),
            content: serialized.clone(),
        }],
    )?;
    Ok(DocumentEditResult::Applied(DocumentEditOutcome {
        serialized_content: serialized,
        analysis,
        refreshed_document,
    }))
}

fn read_text_with_overlays(
    fs: &ProjectFs,
    path: &ProjectPath,
    overlays: &[ProjectOverlay],
) -> Result<String, String> {
    overlays
        .iter()
        .find(|overlay| overlay.path == *path)
        .map(|overlay| overlay.content.clone())
        .map(Ok)
        .unwrap_or_else(|| fs.read_to_string(path).map_err(|error| error.to_string()))
}

fn layout_to_document(
    path: &ProjectPath,
    object_key: &str,
    layout: &Layout<Authored>,
    resolved_layout: &Layout<Resolved>,
    catalog: &[FixtureCatalogItem],
) -> Result<LayoutDocument, String> {
    if layout.fixtures.len() != resolved_layout.fixtures.len() {
        return Err("resolved layout fixture count did not match authored layout".to_string());
    }

    Ok(LayoutDocument {
        path: path.to_slash_string(),
        object_key: object_key.to_string(),
        name: layout.name.clone(),
        units: layout.units,
        render_bounds: layout_render_bounds(&resolved_layout.fixtures),
        fixtures: layout
            .fixtures
            .iter()
            .zip(&resolved_layout.fixtures)
            .map(|(fixture, resolved)| placement_to_document(fixture, resolved, path))
            .collect(),
        groups: layout
            .groups
            .iter()
            .map(|group| LayoutGroupDocument {
                name: group.name.clone(),
                members: group
                    .members
                    .iter()
                    .map(|member| member.as_str().to_string())
                    .collect(),
            })
            .collect(),
        fixture_catalog: catalog.to_vec(),
    })
}

fn placement_to_document(
    placement: &FixturePlacement<Authored>,
    resolved: &FixturePlacement<Resolved>,
    source_path: &ProjectPath,
) -> LayoutFixturePlacement {
    let (fixture, resolved_source_path, resolved_object_key) = match &placement.fixture {
        InlineOrImport::Inline(fixture) => (
            LayoutFixtureRef::Inline {
                name: fixture.name.clone(),
                color_model: fixture.color_model,
                bulb_size: fixture.bulb_size,
                geometry: fixture.geometry.clone(),
            },
            source_path.to_slash_string(),
            None,
        ),
        InlineOrImport::Import { import } => {
            let resolved_path = resolve_import_file_path(source_path, import.path())
                .map(|path| path.to_slash_string())
                .unwrap_or_else(|_| import.path().to_slash_string());
            (
                LayoutFixtureRef::Import {
                    import: import.raw().to_string(),
                    object_key: import.object().map(|object| object.as_str().to_string()),
                    source_path: Some(resolved_path.clone()),
                },
                resolved_path,
                import.object().map(|object| object.as_str().to_string()),
            )
        }
    };

    LayoutFixturePlacement {
        id: placement.id.clone(),
        fixture,
        resolved_fixture: ResolvedLayoutFixture {
            name: resolved.fixture.name.clone(),
            color_model: resolved.fixture.color_model,
            bulb_size: resolved.fixture.bulb_size,
            geometry: resolved.fixture.geometry.clone(),
            geometry_summary: geometry_summary(&resolved.fixture.geometry),
            render_plan: geometry_render_plan(
                &resolved.fixture.geometry,
                resolved.fixture.bulb_size,
            ),
            source_path: resolved_source_path,
            object_key: resolved_object_key,
        },
        transform: placement.transform,
    }
}

fn fixture_catalog_from_analysis(
    analysis: &ProjectAnalysis,
    importing_source_path: &ProjectPath,
) -> Vec<FixtureCatalogItem> {
    let mut catalog = Vec::new();
    for (path, analyzed) in &analysis.files {
        let Some(file) = analyzed.file.as_ref() else {
            continue;
        };
        for (key, object) in file {
            let DawnObject::Fixture(fixture) = object else {
                continue;
            };
            let source_path = path.to_slash_string();
            let import_path = relative_import_path(importing_source_path, path);
            catalog.push(FixtureCatalogItem {
                object_key: key.clone(),
                source_path: source_path.clone(),
                import_string: format!("{import_path}::{key}"),
                display_name: fixture.name.clone(),
                color_model: fixture.color_model,
                bulb_size: fixture.bulb_size,
                geometry: fixture.geometry.clone(),
                geometry_summary: geometry_summary(&fixture.geometry),
                render_plan: geometry_render_plan(&fixture.geometry, fixture.bulb_size),
            });
        }
    }
    catalog.sort_by(|left, right| {
        left.display_name
            .cmp(&right.display_name)
            .then_with(|| left.source_path.cmp(&right.source_path))
            .then_with(|| left.object_key.cmp(&right.object_key))
    });
    catalog
}
fn document_to_layout(document: LayoutDocument) -> Result<Layout<Authored>, String> {
    Ok(Layout {
        name: document.name,
        units: document.units,
        fixtures: document
            .fixtures
            .into_iter()
            .map(document_to_placement)
            .collect::<Result<Vec<_>, _>>()?,
        groups: document
            .groups
            .into_iter()
            .map(|group| Group {
                name: group.name,
                members: group.members.into_iter().map(FixtureRef::new).collect(),
            })
            .collect(),
    })
}

fn document_to_placement(
    placement: LayoutFixturePlacement,
) -> Result<FixturePlacement<Authored>, String> {
    let fixture = match placement.fixture {
        LayoutFixtureRef::Import { import, .. } => InlineOrImport::Import {
            import: ImportRef::new(import)?,
        },
        LayoutFixtureRef::Inline {
            name,
            color_model,
            bulb_size,
            geometry,
        } => InlineOrImport::Inline(Fixture {
            name,
            color_model,
            bulb_size,
            geometry,
        }),
    };
    Ok(FixturePlacement {
        id: placement.id,
        fixture,
        transform: placement.transform,
    })
}

fn fixture_to_document(object_key: &str, fixture: &Fixture) -> FixtureDefinitionDocument {
    FixtureDefinitionDocument {
        object_key: object_key.to_string(),
        name: fixture.name.clone(),
        color_model: fixture.color_model,
        bulb_size: fixture.bulb_size,
        geometry: fixture.geometry.clone(),
        geometry_summary: geometry_summary(&fixture.geometry),
        render_plan: geometry_render_plan(&fixture.geometry, fixture.bulb_size),
    }
}

fn document_to_fixture(document: &FixtureDefinitionDocument) -> Fixture {
    Fixture {
        name: document.name.clone(),
        color_model: document.color_model,
        bulb_size: document.bulb_size,
        geometry: document.geometry.clone(),
    }
}

fn validate_fixture_document(document: &FixtureDocument) -> Result<(), String> {
    let mut keys = HashSet::new();
    for fixture in &document.fixtures {
        validate_simple_identifier(&fixture.object_key, "fixture object key")?;
        if !keys.insert(fixture.object_key.as_str()) {
            return Err(format!(
                "duplicate fixture object key `{}`",
                fixture.object_key
            ));
        }
    }
    Ok(())
}

fn validate_layout_identifiers(layout: &Layout<Authored>) -> Result<(), String> {
    let mut ids = HashSet::new();
    for fixture in &layout.fixtures {
        validate_simple_identifier(&fixture.id, "fixture placement id")?;
        if !ids.insert(fixture.id.as_str()) {
            return Err(format!("duplicate fixture placement id `{}`", fixture.id));
        }
    }
    Ok(())
}

fn repair_layout_group_members(current: &Layout<Authored>, next: &mut Layout<Authored>) {
    let mut renamed_by_index = HashMap::new();
    for (current_fixture, next_fixture) in current.fixtures.iter().zip(&next.fixtures) {
        if current_fixture.id != next_fixture.id {
            renamed_by_index.insert(current_fixture.id.as_str(), next_fixture.id.as_str());
        }
    }
    let next_ids = next
        .fixtures
        .iter()
        .map(|fixture| fixture.id.as_str())
        .collect::<HashSet<_>>();

    for group in &mut next.groups {
        let mut seen = HashSet::new();
        group.members = group
            .members
            .iter()
            .filter_map(|member| {
                let current = member.as_str();
                let repaired = renamed_by_index.get(current).copied().unwrap_or(current);
                if next_ids.contains(repaired) && seen.insert(repaired.to_string()) {
                    Some(FixtureRef::new(repaired))
                } else {
                    None
                }
            })
            .collect();
    }
}

fn validate_simple_identifier(value: &str, label: &str) -> Result<(), String> {
    let mut chars = value.chars();
    let Some(first) = chars.next() else {
        return Err(format!("{label} cannot be empty"));
    };
    if !(first.is_ascii_alphabetic() || first == '_') {
        return Err(format!("{label} must start with a letter or underscore"));
    }
    if chars.any(|character| !(character.is_ascii_alphanumeric() || character == '_')) {
        return Err(format!(
            "{label} may only contain letters, numbers, and underscores"
        ));
    }
    Ok(())
}

fn replace_top_level_object(
    text: &str,
    object_key: &str,
    object: &DawnObject<Authored>,
) -> Result<String, String> {
    let mut replacements = BTreeMap::new();
    replacements.insert(
        object_key.to_string(),
        Some(serialize_top_level_object(object_key, object)?),
    );
    replace_top_level_objects(text, replacements)
}

fn serialize_top_level_object(
    object_key: &str,
    object: &DawnObject<Authored>,
) -> Result<String, String> {
    let mut file = DawnFile::new();
    file.insert(object_key.to_string(), object.clone());
    serde_yaml::to_string(&file).map_err(|error| error.to_string())
}

fn replace_top_level_objects(
    text: &str,
    mut replacements: BTreeMap<String, Option<String>>,
) -> Result<String, String> {
    let blocks = top_level_object_blocks(text);
    let mut output = String::with_capacity(text.len());
    let mut cursor = 0;
    for block in blocks {
        output.push_str(&text[cursor..block.start]);
        cursor = block.end;
        match replacements.remove(&block.key) {
            Some(Some(serialized)) => {
                output.push_str(serialized.trim_end_matches('\n'));
                output.push('\n');
            }
            Some(None) => {}
            None => output.push_str(&text[block.start..block.end]),
        }
    }
    output.push_str(&text[cursor..]);
    for serialized in replacements.into_values().flatten() {
        if !output.ends_with('\n') && !output.is_empty() {
            output.push('\n');
        }
        if !output.ends_with("\n\n") && !output.trim().is_empty() {
            output.push('\n');
        }
        output.push_str(serialized.trim_end_matches('\n'));
        output.push('\n');
    }
    Ok(output)
}

#[derive(Debug, Clone)]
struct TopLevelObjectBlock {
    key: String,
    start: usize,
    end: usize,
}

fn top_level_object_blocks(text: &str) -> Vec<TopLevelObjectBlock> {
    #[derive(Debug)]
    struct LineInfo {
        start: usize,
        key: Option<String>,
        comment_or_blank: bool,
    }

    let mut lines = Vec::new();
    let mut offset = 0;
    for line in text.split_inclusive('\n') {
        let line_without_newline = line.trim_end_matches(['\r', '\n']);
        lines.push(LineInfo {
            start: offset,
            key: top_level_key(line_without_newline),
            comment_or_blank: line_without_newline.trim().is_empty()
                || line_without_newline.starts_with('#'),
        });
        offset += line.len();
    }
    if offset < text.len() {
        let line = &text[offset..];
        lines.push(LineInfo {
            start: offset,
            key: top_level_key(line),
            comment_or_blank: line.trim().is_empty() || line.starts_with('#'),
        });
    }
    let keyed_lines = lines
        .iter()
        .enumerate()
        .filter_map(|(index, line)| {
            line.key
                .as_ref()
                .map(|key| (index, key.clone(), line.start))
        })
        .collect::<Vec<_>>();

    keyed_lines
        .iter()
        .enumerate()
        .map(|(index, (line_index, key, start))| {
            let end = keyed_lines
                .get(index + 1)
                .map(|(next_line_index, _, next_start)| {
                    let mut boundary = *next_start;
                    let mut candidate = *next_line_index;
                    while candidate > line_index + 1 && lines[candidate - 1].comment_or_blank {
                        candidate -= 1;
                        boundary = lines[candidate].start;
                    }
                    boundary
                })
                .unwrap_or(text.len());
            TopLevelObjectBlock {
                key: key.clone(),
                start: *start,
                end,
            }
        })
        .collect()
}

fn top_level_key(line: &str) -> Option<String> {
    if line.is_empty() || line.starts_with(char::is_whitespace) || line.starts_with('#') {
        return None;
    }
    let (key, rest) = line.split_once(':')?;
    if key.is_empty()
        || key == "---"
        || key.chars().any(|character| {
            character.is_whitespace() || matches!(character, '[' | ']' | '{' | '}' | ',')
        })
        || !(rest.is_empty() || rest.starts_with(char::is_whitespace))
    {
        return None;
    }
    Some(key.to_string())
}

fn introduced_error_diagnostics(
    before: &ProjectAnalysis,
    after: &ProjectAnalysis,
) -> Vec<ProjectDiagnostic> {
    after
        .diagnostics
        .iter()
        .filter(|diagnostic| {
            diagnostic.severity == DiagnosticSeverity::Error
                && !before.diagnostics.contains(diagnostic)
        })
        .cloned()
        .collect()
}

fn overlay_after_save(
    saved_path: ProjectPath,
    content: String,
    overlays: Vec<ProjectOverlay>,
) -> Vec<ProjectOverlay> {
    let mut next = overlays
        .into_iter()
        .filter(|overlay| overlay.path != saved_path)
        .collect::<Vec<_>>();
    next.push(ProjectOverlay {
        path: saved_path,
        content,
    });
    next
}
