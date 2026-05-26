use std::collections::{BTreeMap, HashMap, HashSet};

use indexmap::IndexMap;
use serde::{Deserialize, Serialize};

use crate::analysis::{
    analyze_project_with_overlays, AnalysisImportResolver, DiagnosticCode, DiagnosticSeverity,
    ProjectAnalysis, ProjectDiagnostic, ProjectOverlay,
};
use crate::effect_script::{
    compile as compile_effect_script, CompiledEffect, ParamDefault, RuntimeValue, ScriptType,
};
use crate::fs::WorkspaceFs;
use crate::lower::lower_layout;
use crate::model::*;
use crate::path::{
    canonicalize_path, resolve_import_path, serialized_import_path, PathStringExt, Utf8PathBuf,
};
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
    Sequence,
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
    pub target_order: Vec<LayoutTargetDocument>,
    pub render_bounds: GeometryRenderBounds,
    pub fixtures: Vec<LayoutFixturePlacement>,
    pub groups: Vec<LayoutGroupDocument>,
    pub fixture_catalog: Vec<FixtureCatalogItem>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LayoutTargetDocument {
    pub kind: LayoutTargetKind,
    pub name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LayoutFixturePlacement {
    pub id: FixtureId,
    pub name: String,
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
    pub members: Vec<FixtureId>,
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

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SequenceDocument {
    pub path: String,
    pub object_key: String,
    pub duration_ms: u64,
    pub frame_rate: u32,
    pub lanes: Vec<SequenceLaneDocument>,
    pub effect_scripts: Vec<SequenceEffectScriptDocument>,
    pub effects: Vec<SequenceEffectDocument>,
    pub degraded: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SequenceLaneDocument {
    pub target: LayoutTargetDocument,
    pub label: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SequenceEffectDocument {
    pub index: usize,
    pub id: u32,
    pub start_ms: u64,
    pub duration_ms: u64,
    pub target: LayoutTargetDocument,
    pub target_label: String,
    pub script: String,
    pub render: Option<SequenceEffectRenderDocument>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SequenceEffectScriptDocument {
    pub name: String,
    pub path: String,
    pub import: String,
}

#[derive(Debug, Clone)]
pub enum SequenceDocumentEdit {
    AddEffect {
        script_path: String,
        target: LayoutTargetDocument,
        start_ms: u64,
    },
    DuplicateEffect {
        id: u32,
    },
    DeleteEffect {
        id: u32,
    },
    MoveEffect {
        id: u32,
        start_ms: u64,
        target: Option<LayoutTargetDocument>,
    },
    ResizeEffect {
        id: u32,
        start_ms: u64,
        duration_ms: u64,
    },
    RetargetEffect {
        id: u32,
        target: LayoutTargetDocument,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SequenceEffectRenderDocument {
    pub script_key: String,
    pub script_source: String,
    pub params: Vec<SequenceEffectParamDocument>,
    pub target_pixels: Vec<SequenceEffectPixelDocument>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SequenceEffectParamDocument {
    pub name: String,
    pub value: EffectParam<Resolved>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SequenceEffectPixelDocument {
    pub fixture_index: usize,
    pub pixel_index: usize,
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
    fs: &WorkspaceFs,
    path: Utf8PathBuf,
    overlays: Vec<ProjectOverlay>,
) -> Result<DocumentDescriptor, String> {
    let path = canonicalize_path(&fs.resolve(&path));
    if is_effect_script_path(&path) {
        return Ok(DocumentDescriptor {
            path: path.to_slash_string(),
            objects: Vec::new(),
            available_views: vec![DocumentViewId::Text],
            default_object_keys: BTreeMap::new(),
        });
    }
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
    if let Some(key) = file.iter().find_map(|(key, object)| match object {
        DawnObject::Sequence(_) => Some(key.clone()),
        _ => None,
    }) {
        available_views.push(DocumentViewId::Sequence);
        default_object_keys.insert(DocumentViewId::Sequence, key);
    }

    Ok(DocumentDescriptor {
        path: path.to_slash_string(),
        objects,
        available_views,
        default_object_keys,
    })
}

fn is_effect_script_path(path: &Utf8PathBuf) -> bool {
    path.file_name()
        .is_some_and(|name| name.ends_with(".effect.dawn"))
}

pub fn get_fixture_document(
    fs: &WorkspaceFs,
    path: Utf8PathBuf,
    selected_object_key: Option<&str>,
    overlays: Vec<ProjectOverlay>,
) -> Result<FixtureDocument, String> {
    let path = canonicalize_path(&fs.resolve(&path));
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
    fs: &WorkspaceFs,
    path: Utf8PathBuf,
    object_key: &str,
    project_path: Utf8PathBuf,
    overlays: Vec<ProjectOverlay>,
) -> Result<LayoutDocument, String> {
    let path = canonicalize_path(&fs.resolve(&path));
    let project_path = canonicalize_path(&fs.resolve(&project_path));
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

pub fn get_sequence_document(
    fs: &WorkspaceFs,
    path: Utf8PathBuf,
    object_key: &str,
    project_path: Utf8PathBuf,
    overlays: Vec<ProjectOverlay>,
) -> Result<SequenceDocument, String> {
    let path = canonicalize_path(&fs.resolve(&path));
    let project_path = canonicalize_path(&fs.resolve(&project_path));
    let text = read_text_with_overlays(fs, &path, &overlays)?;
    let file: DawnFile = serde_yaml::from_str(&text).map_err(|error| error.to_string())?;
    let object = file
        .get(object_key)
        .ok_or_else(|| format!("sequence object `{object_key}` was not found"))?;
    let DawnObject::Sequence(sequence) = object else {
        return Err(format!("object `{object_key}` is not a sequence"));
    };

    let analysis = analyze_project_with_overlays(fs, project_path, None, overlays.clone());
    let layout = analysis
        .resolved
        .as_ref()
        .map(|project| &project.display.layout);
    Ok(sequence_to_document(
        fs,
        &path,
        object_key,
        sequence,
        layout,
        Some(&analysis),
        &overlays,
    ))
}

pub fn apply_layout_document_edit(
    fs: &WorkspaceFs,
    path: Utf8PathBuf,
    object_key: &str,
    document: LayoutDocument,
    base_content: String,
    overlays: Vec<ProjectOverlay>,
    project_path: Utf8PathBuf,
    allow_breaking_references: bool,
) -> Result<DocumentEditResult<LayoutDocument>, String> {
    let path = canonicalize_path(&fs.resolve(&path));
    let project_path = canonicalize_path(&fs.resolve(&project_path));
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
    fs: &WorkspaceFs,
    path: Utf8PathBuf,
    document: FixtureDocument,
    base_content: String,
    overlays: Vec<ProjectOverlay>,
    project_path: Utf8PathBuf,
    allow_breaking_references: bool,
) -> Result<DocumentEditResult<FixtureDocument>, String> {
    let path = canonicalize_path(&fs.resolve(&path));
    let project_path = canonicalize_path(&fs.resolve(&project_path));
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

pub fn apply_sequence_document_edit(
    fs: &WorkspaceFs,
    path: Utf8PathBuf,
    object_key: &str,
    edit: SequenceDocumentEdit,
    base_content: String,
    overlays: Vec<ProjectOverlay>,
    project_path: Utf8PathBuf,
) -> Result<DocumentEditResult<SequenceDocument>, String> {
    let path = canonicalize_path(&fs.resolve(&path));
    let project_path = canonicalize_path(&fs.resolve(&project_path));
    let file: DawnFile = serde_yaml::from_str(&base_content).map_err(|error| error.to_string())?;
    let Some(DawnObject::Sequence(current_sequence)) = file.get(object_key) else {
        return Err(format!("sequence object `{object_key}` was not found"));
    };
    let mut sequence = current_sequence.clone();
    let base_analysis = analyze_project_with_overlays(
        fs,
        project_path.clone(),
        None,
        overlay_after_save(path.clone(), base_content.clone(), overlays.clone()),
    );
    apply_sequence_edit_operation(fs, &path, &base_analysis, &overlays, &mut sequence, edit)?;
    sort_sequence_effects(
        &mut sequence,
        base_analysis
            .resolved
            .as_ref()
            .map(|project| &project.display.layout),
    );

    let object = DawnObject::Sequence(sequence);
    let serialized = replace_top_level_object(&base_content, object_key, &object)?;
    let next_overlays = overlay_after_save(path.clone(), serialized.clone(), overlays);
    let analysis = analyze_project_with_overlays(fs, project_path, None, next_overlays);
    let refreshed_document = get_sequence_document(
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

fn apply_sequence_edit_operation(
    fs: &WorkspaceFs,
    path: &Utf8PathBuf,
    analysis: &ProjectAnalysis,
    overlays: &[ProjectOverlay],
    sequence: &mut Sequence<Authored>,
    edit: SequenceDocumentEdit,
) -> Result<(), String> {
    match edit {
        SequenceDocumentEdit::AddEffect {
            script_path,
            target,
            start_ms,
        } => {
            let id = next_sequence_effect_id(sequence)
                .ok_or_else(|| "no sequence effect IDs are available".to_string())?;
            let script_key = Utf8PathBuf::from(script_path.clone()).to_slash_string();
            let compiled_script;
            let script = match analysis
                .scripts
                .get(&script_key)
                .and_then(|script| script.result.as_ref().ok())
            {
                Some(script) => script,
                None => {
                    let script_path = Utf8PathBuf::from(script_path.clone());
                    let source = read_text_with_overlays(fs, &script_path, overlays)?;
                    compiled_script = compile_effect_script(&source)
                        .map_err(|diagnostics| script_diagnostics_message(&diagnostics))?;
                    &compiled_script
                }
            };
            let start_ms = start_ms.min(sequence.duration.milliseconds.saturating_sub(1));
            let duration_ms = 1_000
                .min(sequence.duration.milliseconds.saturating_sub(start_ms))
                .max(1);
            sequence.effects.push(SequenceEffect {
                id,
                start: Time {
                    milliseconds: start_ms,
                },
                duration: Time {
                    milliseconds: duration_ms,
                },
                target: authored_target_from_document(&target, analysis)?,
                params: materialized_effect_params(script),
                script: InlineOrImport::Import {
                    import: ImportRef::new(serialized_import_path(
                        path,
                        &Utf8PathBuf::from(script_path),
                    ))?,
                },
            });
        }
        SequenceDocumentEdit::DuplicateEffect { id } => {
            let Some(source) = sequence
                .effects
                .iter()
                .find(|effect| effect.id == id)
                .cloned()
            else {
                return Err(format!("sequence effect `{id}` was not found"));
            };
            let new_id = next_sequence_effect_id(sequence)
                .ok_or_else(|| "no sequence effect IDs are available".to_string())?;
            let mut duplicate = source;
            duplicate.id = new_id;
            sequence.effects.push(duplicate);
        }
        SequenceDocumentEdit::DeleteEffect { id } => {
            sequence.effects.retain(|effect| effect.id != id);
            for clip in &mut sequence.automation_clips {
                clip.targets.retain(|target| *target != id);
            }
        }
        SequenceDocumentEdit::MoveEffect {
            id,
            start_ms,
            target,
        } => {
            let duration = sequence
                .effects
                .iter()
                .find(|effect| effect.id == id)
                .map(|effect| effect.duration.milliseconds)
                .ok_or_else(|| format!("sequence effect `{id}` was not found"))?;
            let max_start = sequence.duration.milliseconds.saturating_sub(duration);
            let next_target = target
                .as_ref()
                .map(|target| authored_target_from_document(target, analysis))
                .transpose()?;
            let Some(effect) = sequence.effects.iter_mut().find(|effect| effect.id == id) else {
                return Err(format!("sequence effect `{id}` was not found"));
            };
            effect.start = Time {
                milliseconds: start_ms.min(max_start),
            };
            if let Some(target) = next_target {
                effect.target = target;
            }
        }
        SequenceDocumentEdit::ResizeEffect {
            id,
            start_ms,
            duration_ms,
        } => {
            let Some(effect) = sequence.effects.iter_mut().find(|effect| effect.id == id) else {
                return Err(format!("sequence effect `{id}` was not found"));
            };
            let start_ms = start_ms.min(sequence.duration.milliseconds.saturating_sub(1));
            effect.start = Time {
                milliseconds: start_ms,
            };
            effect.duration = Time {
                milliseconds: duration_ms.max(1).min(
                    sequence
                        .duration
                        .milliseconds
                        .saturating_sub(start_ms)
                        .max(1),
                ),
            };
        }
        SequenceDocumentEdit::RetargetEffect { id, target } => {
            let next_target = authored_target_from_document(&target, analysis)?;
            let Some(effect) = sequence.effects.iter_mut().find(|effect| effect.id == id) else {
                return Err(format!("sequence effect `{id}` was not found"));
            };
            effect.target = next_target;
        }
    }
    Ok(())
}

fn read_text_with_overlays(
    fs: &WorkspaceFs,
    path: &Utf8PathBuf,
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
    path: &Utf8PathBuf,
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
        target_order: layout
            .target_order
            .iter()
            .map(|target| LayoutTargetDocument {
                kind: target.kind,
                name: target.name.clone(),
            })
            .collect(),
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
                members: group.members.iter().copied().collect(),
            })
            .collect(),
        fixture_catalog: catalog.to_vec(),
    })
}

fn sequence_to_document(
    fs: &WorkspaceFs,
    path: &Utf8PathBuf,
    object_key: &str,
    sequence: &Sequence<Authored>,
    layout: Option<&Layout<Resolved>>,
    analysis: Option<&ProjectAnalysis>,
    overlays: &[ProjectOverlay],
) -> SequenceDocument {
    let fixture_names_by_id = layout
        .map(|layout| {
            layout
                .fixtures
                .iter()
                .map(|fixture| (fixture.id, fixture.name.clone()))
                .collect::<HashMap<_, _>>()
        })
        .unwrap_or_default();
    let lanes = if let Some(layout) = layout {
        layout
            .target_order
            .iter()
            .map(|target| SequenceLaneDocument {
                target: LayoutTargetDocument {
                    kind: target.kind,
                    name: target.name.clone(),
                },
                label: target_label(target.kind, &target.name),
            })
            .collect()
    } else {
        fallback_sequence_lanes(sequence, &fixture_names_by_id)
    };
    let effects = sequence
        .effects
        .iter()
        .enumerate()
        .map(|(index, effect)| {
            let target = effect_target_document(&effect.target, &fixture_names_by_id);
            let target_label = target_label(target.kind, &target.name);
            let render = layout.and_then(|layout| {
                analysis.and_then(|analysis| {
                    sequence_effect_render_document(path, effect, layout, analysis)
                })
            });
            SequenceEffectDocument {
                index,
                id: effect.id.clone(),
                start_ms: effect.start.milliseconds,
                duration_ms: effect.duration.milliseconds,
                target,
                target_label,
                script: sequence_effect_script_label(&effect.script),
                render,
            }
        })
        .collect();
    SequenceDocument {
        path: path.to_slash_string(),
        object_key: object_key.to_string(),
        duration_ms: sequence.duration.milliseconds,
        frame_rate: sequence.frame_rate,
        lanes,
        effect_scripts: sequence_effect_script_catalog(fs, path, analysis, overlays),
        effects,
        degraded: layout.is_none(),
    }
}

fn sequence_effect_script_catalog(
    fs: &WorkspaceFs,
    sequence_path: &Utf8PathBuf,
    analysis: Option<&ProjectAnalysis>,
    overlays: &[ProjectOverlay],
) -> Vec<SequenceEffectScriptDocument> {
    let mut by_path = BTreeMap::new();
    if let Some(analysis) = analysis {
        for (path, script) in &analysis.scripts {
            if !path.ends_with(".effect.dawn") {
                continue;
            }
            let path = Utf8PathBuf::from(path.clone());
            if let Ok(compiled) = &script.result {
                by_path.insert(
                    path.clone(),
                    SequenceEffectScriptDocument {
                        name: compiled.name.clone(),
                        path: path.to_slash_string(),
                        import: serialized_import_path(sequence_path, &path),
                    },
                );
            }
        }
    }
    if let Ok(entries) = fs.list_entries() {
        for entry in entries {
            if !entry
                .path
                .file_name()
                .is_some_and(|name| name.ends_with(".effect.dawn"))
            {
                continue;
            }
            let path = canonicalize_path(&fs.resolve(&entry.path));
            if by_path.contains_key(&path) {
                continue;
            }
            let Ok(source) = read_text_with_overlays(fs, &path, overlays) else {
                continue;
            };
            let Ok(compiled) = compile_effect_script(&source) else {
                continue;
            };
            by_path.insert(
                path.clone(),
                SequenceEffectScriptDocument {
                    name: compiled.name,
                    path: path.to_slash_string(),
                    import: serialized_import_path(sequence_path, &path),
                },
            );
        }
    }
    let mut scripts = by_path.into_values().collect::<Vec<_>>();
    scripts.sort_by(|left, right| {
        left.name
            .cmp(&right.name)
            .then_with(|| left.path.cmp(&right.path))
    });
    scripts
}

fn script_diagnostics_message(diagnostics: &[crate::effect_script::ScriptDiagnostic]) -> String {
    diagnostics
        .first()
        .map(|diagnostic| diagnostic.message.clone())
        .unwrap_or_else(|| "effect script did not compile".to_string())
}

fn next_sequence_effect_id(sequence: &Sequence<Authored>) -> Option<u32> {
    let existing = sequence
        .effects
        .iter()
        .map(|effect| effect.id)
        .chain(sequence.automation_clips.iter().map(|clip| clip.id))
        .collect::<HashSet<_>>();
    (1..=u32::MAX).find(|id| !existing.contains(id))
}

fn materialized_effect_params(script: &CompiledEffect) -> IndexMap<String, EffectParam<Authored>> {
    script
        .params
        .iter()
        .map(|schema| {
            let param = match &schema.default {
                Some(ParamDefault::Value(value)) => runtime_value_to_authored_param(value),
                None => type_default_param(schema.value_type, &schema.options),
            };
            (schema.name.clone(), param)
        })
        .collect()
}

fn runtime_value_to_authored_param(value: &RuntimeValue) -> EffectParam<Authored> {
    match value {
        RuntimeValue::Float(value) => EffectParam::Float { value: *value },
        RuntimeValue::Int(value) => EffectParam::Integer {
            value: (*value).max(0) as u64,
        },
        RuntimeValue::Bool(value) => EffectParam::Boolean { value: *value },
        RuntimeValue::Color(value) => EffectParam::Color { value: *value },
        RuntimeValue::Curve(curve) => EffectParam::Curve {
            curve: InlineOrImport::Inline(curve.clone()),
        },
        RuntimeValue::Enum(value) => EffectParam::Enum {
            value: value.clone(),
        },
        RuntimeValue::Flags(value) => EffectParam::Flags {
            value: value.clone(),
        },
        RuntimeValue::Fixture(_) | RuntimeValue::Pixel(_) => {
            unreachable!("params cannot default to context values")
        }
    }
}

fn type_default_param(value_type: ScriptType, options: &[String]) -> EffectParam<Authored> {
    match value_type {
        ScriptType::Float => EffectParam::Float { value: 0.0 },
        ScriptType::Int => EffectParam::Integer { value: 0 },
        ScriptType::Bool => EffectParam::Boolean { value: false },
        ScriptType::Color => EffectParam::Color {
            value: Color::new(255, 255, 255),
        },
        ScriptType::CurveFloat => EffectParam::Curve {
            curve: InlineOrImport::Inline(Curve {
                value_type: CurveValueType::Float,
                points: vec![
                    CurvePoint {
                        time: 0.0,
                        value: CurveValue::Float(1.0),
                    },
                    CurvePoint {
                        time: 1.0,
                        value: CurveValue::Float(0.0),
                    },
                ],
            }),
        },
        ScriptType::CurveColor => EffectParam::Curve {
            curve: InlineOrImport::Inline(Curve {
                value_type: CurveValueType::Color,
                points: vec![CurvePoint {
                    time: 0.0,
                    value: CurveValue::Color(Color::new(255, 255, 255)),
                }],
            }),
        },
        ScriptType::Enum => EffectParam::Enum {
            value: options.first().cloned().unwrap_or_default(),
        },
        ScriptType::Flags => EffectParam::Flags {
            value: Flags { values: Vec::new() },
        },
        ScriptType::Fixture | ScriptType::Pixel | ScriptType::Void => {
            unreachable!("context and void types are not params")
        }
    }
}

fn authored_target_from_document(
    target: &LayoutTargetDocument,
    analysis: &ProjectAnalysis,
) -> Result<EffectTarget<Authored>, String> {
    match target.kind {
        LayoutTargetKind::Group => Ok(EffectTarget::Group {
            name: GroupRef::new(target.name.clone()),
        }),
        LayoutTargetKind::Fixture => {
            let id = analysis
                .resolved
                .as_ref()
                .and_then(|project| {
                    project
                        .display
                        .layout
                        .fixtures
                        .iter()
                        .find(|fixture| fixture.name == target.name)
                        .map(|fixture| fixture.id)
                })
                .or_else(|| target.name.parse::<u32>().ok().map(FixtureId))
                .ok_or_else(|| format!("fixture target `{}` was not found", target.name))?;
            Ok(EffectTarget::Fixture { id })
        }
    }
}

fn sort_sequence_effects(sequence: &mut Sequence<Authored>, layout: Option<&Layout<Resolved>>) {
    let lane_order = layout
        .map(|layout| {
            layout
                .target_order
                .iter()
                .enumerate()
                .map(|(index, target)| ((target.kind, target.name.clone()), index))
                .collect::<HashMap<_, _>>()
        })
        .unwrap_or_default();
    sequence.effects.sort_by_key(|effect| {
        (
            effect.start.milliseconds,
            lane_order
                .get(&authored_target_sort_key(&effect.target, layout))
                .copied()
                .unwrap_or(usize::MAX),
            effect.id,
        )
    });
}

fn authored_target_sort_key(
    target: &EffectTarget<Authored>,
    layout: Option<&Layout<Resolved>>,
) -> (LayoutTargetKind, String) {
    match target {
        EffectTarget::Group { name } => (LayoutTargetKind::Group, name.as_str().to_string()),
        EffectTarget::Fixture { id } => {
            let name = layout
                .and_then(|layout| {
                    layout
                        .fixtures
                        .iter()
                        .find(|fixture| fixture.id == *id)
                        .map(|fixture| fixture.name.clone())
                })
                .unwrap_or_else(|| id.to_string());
            (LayoutTargetKind::Fixture, name)
        }
    }
}

fn fallback_sequence_lanes(
    sequence: &Sequence<Authored>,
    fixture_names_by_id: &HashMap<FixtureId, String>,
) -> Vec<SequenceLaneDocument> {
    let mut seen = HashSet::new();
    let mut lanes = Vec::new();
    for effect in &sequence.effects {
        let target = effect_target_document(&effect.target, fixture_names_by_id);
        if seen.insert(target.clone()) {
            lanes.push(SequenceLaneDocument {
                label: target_label(target.kind, &target.name),
                target,
            });
        }
    }
    lanes
}

fn sequence_effect_render_document(
    sequence_path: &Utf8PathBuf,
    effect: &SequenceEffect<Authored>,
    layout: &Layout<Resolved>,
    analysis: &ProjectAnalysis,
) -> Option<SequenceEffectRenderDocument> {
    let target_pixels = target_pixels_for_effect(&effect.target, layout)?;
    if target_pixels.is_empty() {
        return None;
    }
    let (script_key, script_source) = match &effect.script {
        InlineOrImport::Inline(source) => (
            format!(
                "inline:{}:{}",
                analysis.root_path.to_slash_string(),
                effect.id
            ),
            source.clone(),
        ),
        InlineOrImport::Import { import } => {
            let path = resolve_import_path(sequence_path, import.path());
            let key = path.to_slash_string();
            let source = analysis
                .files
                .get(&path)
                .and_then(|file| file.text.clone())
                .unwrap_or_else(|| key.clone());
            (key, source)
        }
    };
    if !analysis
        .scripts
        .get(&script_key)
        .is_some_and(|script| script.result.is_ok())
    {
        return None;
    }

    let mut resolver = AnalysisImportResolver {
        files: &analysis.files,
    };
    let params = effect
        .params
        .iter()
        .map(|(name, param)| {
            lower_effect_param_document(sequence_path, param, &mut resolver).map(|value| {
                SequenceEffectParamDocument {
                    name: name.clone(),
                    value,
                }
            })
        })
        .collect::<Result<Vec<_>, _>>()
        .ok()?;

    Some(SequenceEffectRenderDocument {
        script_key,
        script_source,
        params,
        target_pixels,
    })
}

fn target_pixels_for_effect(
    target: &EffectTarget<Authored>,
    layout: &Layout<Resolved>,
) -> Option<Vec<SequenceEffectPixelDocument>> {
    let fixture_indices = match target {
        EffectTarget::Fixture { id } => {
            let index = layout
                .fixtures
                .iter()
                .position(|fixture| fixture.id == *id)?;
            vec![FixtureIndex(index)]
        }
        EffectTarget::Group { name } => layout
            .groups
            .iter()
            .find(|group| group.name == name.as_str())?
            .members
            .clone(),
    };

    Some(
        fixture_indices
            .into_iter()
            .flat_map(|fixture_index| {
                let emitter_count = layout
                    .fixture(fixture_index)
                    .map(|fixture| {
                        geometry_render_plan(&fixture.fixture.geometry, fixture.fixture.bulb_size)
                            .emitters
                            .len()
                    })
                    .unwrap_or_default();
                (0..emitter_count).map(move |pixel_index| SequenceEffectPixelDocument {
                    fixture_index: fixture_index.0,
                    pixel_index,
                })
            })
            .collect(),
    )
}

fn lower_effect_param_document(
    source_path: &Utf8PathBuf,
    param: &EffectParam<Authored>,
    resolver: &mut AnalysisImportResolver<'_>,
) -> Result<EffectParam<Resolved>, String> {
    Ok(match param {
        EffectParam::Integer { value } => EffectParam::Integer { value: *value },
        EffectParam::Float { value } => EffectParam::Float { value: *value },
        EffectParam::Boolean { value } => EffectParam::Boolean { value: *value },
        EffectParam::Enum { value } => EffectParam::Enum {
            value: value.clone(),
        },
        EffectParam::Flags { value } => EffectParam::Flags {
            value: value.clone(),
        },
        EffectParam::Color { value } => EffectParam::Color { value: *value },
        EffectParam::Curve { curve } => EffectParam::Curve {
            curve: match curve {
                InlineOrImport::Inline(curve) => curve.clone(),
                InlineOrImport::Import { import } => {
                    let resolved = resolver
                        .resolve(source_path, import, ObjectKind::Curve)
                        .map_err(|error| error.to_string())?;
                    let DawnObject::Curve(curve) = resolved.object else {
                        return Err(format!("import `{}` is not a curve", import.raw()));
                    };
                    curve
                }
            },
        },
    })
}

fn effect_target_document(
    target: &EffectTarget<Authored>,
    fixture_names_by_id: &HashMap<FixtureId, String>,
) -> LayoutTargetDocument {
    match target {
        EffectTarget::Group { name } => LayoutTargetDocument {
            kind: LayoutTargetKind::Group,
            name: name.as_str().to_string(),
        },
        EffectTarget::Fixture { id } => LayoutTargetDocument {
            kind: LayoutTargetKind::Fixture,
            name: fixture_names_by_id
                .get(id)
                .cloned()
                .unwrap_or_else(|| id.to_string()),
        },
    }
}

fn target_label(kind: LayoutTargetKind, name: &str) -> String {
    match kind {
        LayoutTargetKind::Group => format!("Group {name}"),
        LayoutTargetKind::Fixture => format!("Fixture {name}"),
    }
}

fn sequence_effect_script_label(script: &InlineOrImport<String>) -> String {
    match script {
        InlineOrImport::Inline(script) => script.lines().next().unwrap_or("Inline").to_string(),
        InlineOrImport::Import { import } => import.raw().to_string(),
    }
}

fn placement_to_document(
    placement: &FixturePlacement<Authored>,
    resolved: &FixturePlacement<Resolved>,
    source_path: &Utf8PathBuf,
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
            let resolved_path = resolve_import_path(source_path, import.path()).to_slash_string();
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
        name: placement.name.clone(),
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
    importing_source_path: &Utf8PathBuf,
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
            let import_path = serialized_import_path(importing_source_path, path);
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
        target_order: document
            .target_order
            .into_iter()
            .map(|target| LayoutTargetRef {
                kind: target.kind,
                name: target.name,
            })
            .collect(),
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
                members: group.members,
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
        name: placement.name,
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
    let mut names = HashSet::new();
    for fixture in &layout.fixtures {
        if !ids.insert(fixture.id) {
            return Err(format!("duplicate fixture placement id `{}`", fixture.id));
        }
        let name = fixture.name.trim();
        if name.is_empty() {
            return Err("fixture placement name cannot be empty".to_string());
        }
        if !names.insert(name.to_string()) {
            return Err(format!("duplicate fixture placement name `{name}`"));
        }
    }
    Ok(())
}

fn repair_layout_group_members(current: &Layout<Authored>, next: &mut Layout<Authored>) {
    let mut renamed_by_index = HashMap::new();
    for (current_fixture, next_fixture) in current.fixtures.iter().zip(&next.fixtures) {
        if current_fixture.id != next_fixture.id {
            renamed_by_index.insert(current_fixture.id, next_fixture.id);
        }
    }
    let next_ids = next
        .fixtures
        .iter()
        .map(|fixture| fixture.id)
        .collect::<HashSet<_>>();

    for group in &mut next.groups {
        let mut seen = HashSet::new();
        group.members = group
            .members
            .iter()
            .filter_map(|member| {
                let repaired = renamed_by_index.get(member).copied().unwrap_or(*member);
                if next_ids.contains(&repaired) && seen.insert(repaired) {
                    Some(repaired)
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
    saved_path: Utf8PathBuf,
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
