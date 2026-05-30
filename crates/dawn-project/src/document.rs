use std::collections::{BTreeMap, HashMap, HashSet};

use indexmap::IndexMap;
use serde::{Deserialize, Serialize};

use crate::analysis::{
    analyze_project_with_overlays, AnalysisImportResolver, DiagnosticCode, DiagnosticSeverity,
    ProjectAnalysis, ProjectOverlay,
};
use crate::effect_script::{
    compile as compile_effect_script, CompiledEffect, ParamDefault, RuntimeValue, ScriptType,
};
use crate::fs::WorkspaceFs;
use crate::lower::{lower_layout, SymbolResolver};
use crate::model::*;
use crate::parse::parse_dawn_file_with_source_map;
use crate::path::{
    canonicalize_path, resolve_import_path, serialized_import_path, PathStringExt, Utf8PathBuf,
};
use crate::render::{
    geometry_render_plan, geometry_summary, layout_render_bounds, GeometryRenderBounds,
    GeometryRenderPlan,
};

fn parse_dawn_file(text: &str) -> Result<DawnFile, String> {
    parse_dawn_file_with_source_map(text)
        .map(|parsed| parsed.file)
        .map_err(|error| error.to_string())
}

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
    pub audio: Option<SequenceAudioDocument>,
    pub mark_collections: Vec<SequenceMarkCollectionDocument>,
    pub lanes: Vec<SequenceLaneDocument>,
    pub effect_scripts: Vec<SequenceEffectScriptDocument>,
    pub effects: Vec<SequenceEffectDocument>,
    pub degraded: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SequenceMarkCollectionDocument {
    pub key: String,
    pub name: String,
    pub color: String,
    pub marks_ms: Vec<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SequenceAudioDocument {
    pub import: String,
    pub resolved_path: String,
    pub file_name: String,
    pub exists: bool,
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
    pub scope: SequenceEffectScope,
    pub script: String,
    pub script_source: Option<String>,
    pub params: Vec<SequenceEffectParamDocument>,
    pub render: Option<SequenceEffectRenderDocument>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SequenceEffectScriptDocument {
    pub name: String,
    pub path: String,
    pub import: String,
    pub params: Vec<SequenceEffectScriptParamDocument>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SequenceEffectScriptParamDocument {
    pub name: String,
    pub value_type: ScriptType,
}

#[derive(Debug, Clone)]
pub enum SequenceDocumentEdit {
    SetAudio {
        import: Option<String>,
    },
    AddEffect {
        script_path: String,
        target: LayoutTargetDocument,
        scope: SequenceEffectScope,
        start_ms: u64,
        mark_collection_key: Option<String>,
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
    ChangeEffectScript {
        id: u32,
        script_path: String,
    },
    RetargetEffect {
        id: u32,
        target: LayoutTargetDocument,
    },
    SetEffectScope {
        id: u32,
        scope: SequenceEffectScope,
    },
    UpdateEffectParam {
        id: u32,
        name: String,
        value: SequenceEffectParamEditValue,
    },
    CreateMarkCollection {
        key: String,
        name: String,
        color: String,
    },
    RenameMarkCollection {
        key: String,
        name: String,
    },
    DeleteMarkCollection {
        key: String,
    },
    SetMarkCollectionColor {
        key: String,
        color: String,
    },
    AddMark {
        collection_key: String,
        time_ms: u64,
    },
    MoveMark {
        collection_key: String,
        index: usize,
        time_ms: u64,
    },
    DeleteMark {
        collection_key: String,
        index: usize,
    },
}

#[derive(Debug, Clone)]
pub enum SequenceEffectParamEditValue {
    Integer(u64),
    Float(f64),
    Boolean(bool),
    Color(String),
    Enum(String),
    Flags(Vec<String>),
    FloatCurve(Vec<SequenceEffectParamCurvePointEditValue>),
    ColorCurve(Vec<SequenceEffectParamCurvePointEditValue>),
    Marks(String),
}

#[derive(Debug, Clone)]
pub struct SequenceEffectParamCurvePointEditValue {
    pub time: f64,
    pub value: SequenceEffectParamCurveValueEditValue,
}

#[derive(Debug, Clone)]
pub enum SequenceEffectParamCurveValueEditValue {
    Float(f64),
    Color(String),
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
    pub pixel_count: usize,
}

#[derive(Debug, Clone)]
pub struct DocumentEditOutcome<T> {
    pub serialized_content: String,
    pub refreshed_document: T,
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
    let file: DawnFile = parse_dawn_file(&text)?;
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
    let file: DawnFile = parse_dawn_file(&text)?;
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
    let file: DawnFile = parse_dawn_file(&text)?;
    let object = file
        .get(object_key)
        .ok_or_else(|| format!("layout object `{object_key}` was not found"))?;
    let DawnObject::Layout(layout) = object else {
        return Err(format!("object `{object_key}` is not a layout"));
    };
    let catalog = fixture_catalog_from_analysis(&analysis, &path);
    let mut resolver = AnalysisImportResolver {
        files: &analysis.files,
        scripts: &analysis.scripts,
    };
    let resolved_layout = lower_layout(layout, &path, &mut resolver)
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
    let file: DawnFile = parse_dawn_file(&text)?;
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
    _overlays: Vec<ProjectOverlay>,
) -> Result<DocumentEditOutcome<LayoutDocument>, String> {
    let _path = canonicalize_path(&fs.resolve(&path));
    let file: DawnFile = parse_dawn_file(&base_content)?;
    let Some(DawnObject::Layout(current_layout)) = file.get(object_key) else {
        return Err(format!("layout object `{object_key}` was not found"));
    };
    let refreshed_document = document.clone();
    let mut layout = document_to_layout(document)?;
    repair_layout_group_members(current_layout, &mut layout);
    validate_layout_identifiers(&layout)?;
    let object = DawnObject::Layout(layout);
    let serialized = replace_top_level_object(&base_content, object_key, &object)?;
    Ok(DocumentEditOutcome {
        serialized_content: serialized,
        refreshed_document,
    })
}

pub fn apply_fixture_document_edit(
    fs: &WorkspaceFs,
    path: Utf8PathBuf,
    document: FixtureDocument,
    base_content: String,
    _overlays: Vec<ProjectOverlay>,
) -> Result<DocumentEditOutcome<FixtureDocument>, String> {
    let path = canonicalize_path(&fs.resolve(&path));
    validate_fixture_document(&document)?;
    let edited_fixtures = document
        .fixtures
        .iter()
        .map(|fixture| (fixture.object_key.clone(), document_to_fixture(fixture)))
        .collect::<Vec<_>>();
    let refreshed_document = refresh_fixture_document(
        &path,
        document.selected_object_key.as_deref(),
        &edited_fixtures,
    );
    let file: DawnFile = parse_dawn_file(&base_content)?;
    let mut replacements = BTreeMap::new();
    for (key, object) in &file {
        if matches!(object, DawnObject::Fixture(_)) {
            replacements.insert(key.clone(), None);
        }
    }
    for (object_key, fixture) in &edited_fixtures {
        replacements.insert(
            object_key.clone(),
            Some(serialize_top_level_object(
                object_key,
                &DawnObject::Fixture(fixture.clone()),
            )?),
        );
    }
    let serialized = replace_top_level_objects(&base_content, replacements)?;
    Ok(DocumentEditOutcome {
        serialized_content: serialized,
        refreshed_document,
    })
}

fn refresh_fixture_document(
    path: &Utf8PathBuf,
    selected_object_key: Option<&str>,
    fixtures: &[(String, Fixture)],
) -> FixtureDocument {
    let fixtures = fixtures
        .iter()
        .map(|(object_key, fixture)| fixture_to_document(object_key, fixture))
        .collect::<Vec<_>>();
    let selected_object_key = selected_object_key
        .filter(|key| fixtures.iter().any(|fixture| fixture.object_key == *key))
        .map(str::to_string)
        .or_else(|| fixtures.first().map(|fixture| fixture.object_key.clone()));

    FixtureDocument {
        path: path.to_slash_string(),
        selected_object_key,
        fixtures,
    }
}

pub fn apply_sequence_document_edit(
    fs: &WorkspaceFs,
    path: Utf8PathBuf,
    object_key: &str,
    edit: SequenceDocumentEdit,
    base_content: String,
    overlays: Vec<ProjectOverlay>,
    analysis: &ProjectAnalysis,
) -> Result<DocumentEditOutcome<SequenceDocument>, String> {
    let path = canonicalize_path(&fs.resolve(&path));
    let file: DawnFile = parse_dawn_file(&base_content)?;
    let Some(DawnObject::Sequence(current_sequence)) = file.get(object_key) else {
        return Err(format!("sequence object `{object_key}` was not found"));
    };
    let mut sequence = current_sequence.clone();
    let import_to_add = apply_sequence_edit_operation(
        fs,
        &path,
        &file.imports,
        analysis,
        &overlays,
        &mut sequence,
        edit,
    )?;
    sort_sequence_mark_collections(&mut sequence);
    sort_sequence_effects(
        &mut sequence,
        analysis
            .resolved
            .as_ref()
            .map(|project| &project.display.layout),
    );

    let object = DawnObject::Sequence(sequence);
    let mut serialized = replace_top_level_object(&base_content, object_key, &object)?;
    if let Some(import) = import_to_add {
        serialized = ensure_top_level_import(&serialized, import)?;
    }
    let next_text: DawnFile = parse_dawn_file(&serialized)?;
    let Some(DawnObject::Sequence(sequence)) = next_text.get(object_key) else {
        return Err(format!(
            "sequence object `{object_key}` was not found after edit"
        ));
    };
    let refreshed_document = sequence_to_document(
        fs,
        &path,
        object_key,
        sequence,
        analysis
            .resolved
            .as_ref()
            .map(|project| &project.display.layout),
        Some(analysis),
        &[ProjectOverlay {
            path: path.clone(),
            content: serialized.clone(),
        }],
    );
    Ok(DocumentEditOutcome {
        serialized_content: serialized,
        refreshed_document,
    })
}

fn apply_sequence_edit_operation(
    fs: &WorkspaceFs,
    path: &Utf8PathBuf,
    imports: &[DawnImport],
    analysis: &ProjectAnalysis,
    overlays: &[ProjectOverlay],
    sequence: &mut Sequence<Authored>,
    edit: SequenceDocumentEdit,
) -> Result<Option<DawnImport>, String> {
    let mut import_to_add = None;
    match edit {
        SequenceDocumentEdit::SetAudio { import } => {
            sequence.audio = import
                .map(|import| AssetPath::new(import).map(Some))
                .transpose()?
                .flatten();
        }
        SequenceDocumentEdit::AddEffect {
            script_path,
            target,
            scope,
            start_ms,
            mark_collection_key,
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
            let script_path = Utf8PathBuf::from(script_path);
            let (alias, import) = module_import_for_path(path, &script_path, imports);
            sequence.effects.push(SequenceEffect {
                id,
                start: Time {
                    milliseconds: start_ms,
                },
                duration: Time {
                    milliseconds: duration_ms,
                },
                target: authored_target_from_document(&target, analysis)?,
                scope,
                params: materialized_effect_params(script, mark_collection_key.as_deref()),
                script: InlineScriptOrRef::Ref(SymbolRef::new(format!(
                    "{}.{}",
                    alias, script.name
                ))?),
            });
            import_to_add = import;
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
        SequenceDocumentEdit::ChangeEffectScript { id, script_path } => {
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
            let script_path = Utf8PathBuf::from(script_path);
            let (alias, import) = module_import_for_path(path, &script_path, imports);
            let mark_collection_key = sequence
                .mark_collections
                .first()
                .map(|collection| collection.key.as_str());
            let Some(effect) = sequence.effects.iter_mut().find(|effect| effect.id == id) else {
                return Err(format!("sequence effect `{id}` was not found"));
            };
            let mut params = IndexMap::with_capacity(script.params.len());
            for schema in &script.params {
                let next = effect
                    .params
                    .get(&schema.name)
                    .filter(|param| authored_param_matches_schema(schema, param, path, analysis))
                    .cloned()
                    .unwrap_or_else(|| {
                        default_param_for_schema_with_marks(schema, mark_collection_key)
                    });
                params.insert(schema.name.clone(), next);
            }
            effect.script =
                InlineScriptOrRef::Ref(SymbolRef::new(format!("{}.{}", alias, script.name))?);
            effect.params = params;
            import_to_add = import;
        }
        SequenceDocumentEdit::RetargetEffect { id, target } => {
            let next_target = authored_target_from_document(&target, analysis)?;
            let Some(effect) = sequence.effects.iter_mut().find(|effect| effect.id == id) else {
                return Err(format!("sequence effect `{id}` was not found"));
            };
            effect.target = next_target;
        }
        SequenceDocumentEdit::SetEffectScope { id, scope } => {
            let Some(effect) = sequence.effects.iter_mut().find(|effect| effect.id == id) else {
                return Err(format!("sequence effect `{id}` was not found"));
            };
            effect.scope = scope;
        }
        SequenceDocumentEdit::UpdateEffectParam { id, name, value } => {
            let effect_index = sequence
                .effects
                .iter()
                .position(|effect| effect.id == id)
                .ok_or_else(|| format!("sequence effect `{id}` was not found"))?;
            let script = compiled_effect_for_sequence_effect(
                fs,
                path,
                analysis,
                overlays,
                &sequence.effects[effect_index],
            )?;
            let schema = script
                .param(&name)
                .ok_or_else(|| format!("effect parameter `{name}` was not found"))?;
            let edited_param = param_edit_value_to_authored(schema, value)?;
            let mut params = IndexMap::with_capacity(script.params.len());
            for schema in &script.params {
                let next = if schema.name == name {
                    edited_param.clone()
                } else {
                    sequence.effects[effect_index]
                        .params
                        .get(&schema.name)
                        .filter(|param| {
                            authored_param_matches_schema(schema, param, path, analysis)
                        })
                        .cloned()
                        .unwrap_or_else(|| default_param_for_schema(schema))
                };
                params.insert(schema.name.clone(), next);
            }
            sequence.effects[effect_index].params = params;
        }
        SequenceDocumentEdit::CreateMarkCollection { key, name, color } => {
            validate_mark_collection_key(&key)?;
            Color::parse(&color)?;
            if sequence
                .mark_collections
                .iter()
                .any(|collection| collection.key == key)
            {
                return Err(format!("mark collection `{key}` already exists"));
            }
            sequence.mark_collections.push(SequenceMarkCollection {
                key,
                name,
                color,
                marks: Vec::new(),
            });
        }
        SequenceDocumentEdit::RenameMarkCollection { key, name } => {
            let collection = mark_collection_mut(sequence, &key)?;
            collection.name = name;
        }
        SequenceDocumentEdit::DeleteMarkCollection { key } => {
            let original_len = sequence.mark_collections.len();
            sequence
                .mark_collections
                .retain(|collection| collection.key != key);
            if sequence.mark_collections.len() == original_len {
                return Err(format!("mark collection `{key}` was not found"));
            }
        }
        SequenceDocumentEdit::SetMarkCollectionColor { key, color } => {
            Color::parse(&color)?;
            let collection = mark_collection_mut(sequence, &key)?;
            collection.color = color;
        }
        SequenceDocumentEdit::AddMark {
            collection_key,
            time_ms,
        } => {
            let duration_ms = sequence.duration.milliseconds;
            let collection = mark_collection_mut(sequence, &collection_key)?;
            collection.marks.push(Time {
                milliseconds: time_ms.min(duration_ms),
            });
        }
        SequenceDocumentEdit::MoveMark {
            collection_key,
            index,
            time_ms,
        } => {
            let duration_ms = sequence.duration.milliseconds;
            let collection = mark_collection_mut(sequence, &collection_key)?;
            let mark = mark_at_sorted_index_mut(collection, index)?;
            mark.milliseconds = time_ms.min(duration_ms);
        }
        SequenceDocumentEdit::DeleteMark {
            collection_key,
            index,
        } => {
            let collection = mark_collection_mut(sequence, &collection_key)?;
            let original_index = sorted_mark_original_index(collection, index)?;
            collection.marks.remove(original_index);
        }
    }
    Ok(import_to_add)
}

fn mark_collection_mut<'a>(
    sequence: &'a mut Sequence<Authored>,
    key: &str,
) -> Result<&'a mut SequenceMarkCollection, String> {
    sequence
        .mark_collections
        .iter_mut()
        .find(|collection| collection.key == key)
        .ok_or_else(|| format!("mark collection `{key}` was not found"))
}

fn mark_at_sorted_index_mut(
    collection: &mut SequenceMarkCollection,
    sorted_index: usize,
) -> Result<&mut Time, String> {
    let original_index = sorted_mark_original_index(collection, sorted_index)?;
    collection
        .marks
        .get_mut(original_index)
        .ok_or_else(|| format!("mark `{sorted_index}` was not found"))
}

fn sorted_mark_original_index(
    collection: &SequenceMarkCollection,
    sorted_index: usize,
) -> Result<usize, String> {
    let mut indexed_marks = collection
        .marks
        .iter()
        .enumerate()
        .map(|(index, mark)| (index, mark.milliseconds))
        .collect::<Vec<_>>();
    indexed_marks.sort_by_key(|(index, time_ms)| (*time_ms, *index));
    indexed_marks
        .get(sorted_index)
        .map(|(index, _)| *index)
        .ok_or_else(|| format!("mark `{sorted_index}` was not found"))
}

fn compiled_effect_for_sequence_effect(
    fs: &WorkspaceFs,
    path: &Utf8PathBuf,
    analysis: &ProjectAnalysis,
    overlays: &[ProjectOverlay],
    effect: &SequenceEffect<Authored>,
) -> Result<CompiledEffect, String> {
    match &effect.script {
        InlineScriptOrRef::Inline { inline } => compile_effect_script(inline)
            .map_err(|diagnostics| script_diagnostics_message(&diagnostics)),
        InlineScriptOrRef::Ref(reference) => {
            let mut resolver = AnalysisImportResolver {
                files: &analysis.files,
                scripts: &analysis.scripts,
            };
            let script_path = resolver
                .resolve_effect(path, reference)
                .map_err(|error| error.to_string())?
                .source_path;
            let script_key = script_path.to_slash_string();
            if let Some(script) = analysis
                .scripts
                .get(&script_key)
                .and_then(|script| script.result.as_ref().ok())
            {
                return Ok(script.clone());
            }
            let source = read_text_with_overlays(fs, &script_path, overlays)?;
            compile_effect_script(&source)
                .map_err(|diagnostics| script_diagnostics_message(&diagnostics))
        }
    }
}

fn default_param_for_schema(
    schema: &crate::effect_script::EffectParamSchema,
) -> EffectParam<Authored> {
    default_param_for_schema_with_marks(schema, None)
}

fn default_param_for_schema_with_marks(
    schema: &crate::effect_script::EffectParamSchema,
    mark_collection_key: Option<&str>,
) -> EffectParam<Authored> {
    if schema.value_type == ScriptType::Marks {
        return EffectParam::Marks {
            key: mark_collection_key.unwrap_or("marks").to_string(),
        };
    }
    match &schema.default {
        Some(ParamDefault::Value(value)) => runtime_value_to_authored_param(value),
        None => type_default_param(schema.value_type, &schema.options),
    }
}

fn authored_param_matches_schema(
    schema: &crate::effect_script::EffectParamSchema,
    param: &EffectParam<Authored>,
    source_path: &Utf8PathBuf,
    analysis: &ProjectAnalysis,
) -> bool {
    let resolved = match param {
        EffectParam::Curve {
            curve: InlineOrRef::Ref(_),
        } => {
            let mut resolver = AnalysisImportResolver {
                files: &analysis.files,
                scripts: &analysis.scripts,
            };
            lower_effect_param_document(source_path, param, &mut resolver).ok()
        }
        _ => authored_param_to_resolved(param),
    };
    resolved.as_ref().is_some_and(|param| {
        schema.value_type.matches_param(param) && param_options_match(schema, param)
    })
}

fn authored_param_to_resolved(param: &EffectParam<Authored>) -> Option<EffectParam<Resolved>> {
    Some(match param {
        EffectParam::Integer { value } => EffectParam::Integer { value: *value },
        EffectParam::Float { value } if value.is_finite() => EffectParam::Float { value: *value },
        EffectParam::Float { .. } => return None,
        EffectParam::Boolean { value } => EffectParam::Boolean { value: *value },
        EffectParam::Enum { value } => EffectParam::Enum {
            value: value.clone(),
        },
        EffectParam::Flags { value } => EffectParam::Flags {
            value: value.clone(),
        },
        EffectParam::Color { value } => EffectParam::Color { value: *value },
        EffectParam::Curve {
            curve: InlineOrRef::Inline(curve),
        } if curve.points.iter().all(|point| point.time.is_finite()) => EffectParam::Curve {
            curve: curve.clone(),
        },
        EffectParam::Curve { .. } => return None,
        EffectParam::Marks { key } => EffectParam::Marks { key: key.clone() },
    })
}

fn param_options_match(
    schema: &crate::effect_script::EffectParamSchema,
    param: &EffectParam<Resolved>,
) -> bool {
    match param {
        EffectParam::Enum { value } => schema.options.contains(value),
        EffectParam::Flags { value } => value
            .values
            .iter()
            .all(|candidate| schema.options.contains(candidate)),
        _ => true,
    }
}

fn param_edit_value_to_authored(
    schema: &crate::effect_script::EffectParamSchema,
    value: SequenceEffectParamEditValue,
) -> Result<EffectParam<Authored>, String> {
    match (schema.value_type, value) {
        (ScriptType::Int, SequenceEffectParamEditValue::Integer(value)) => {
            Ok(EffectParam::Integer { value })
        }
        (ScriptType::Float, SequenceEffectParamEditValue::Float(value)) if value.is_finite() => {
            Ok(EffectParam::Float { value })
        }
        (ScriptType::Bool, SequenceEffectParamEditValue::Boolean(value)) => {
            Ok(EffectParam::Boolean { value })
        }
        (ScriptType::Color, SequenceEffectParamEditValue::Color(value)) => Ok(EffectParam::Color {
            value: Color::parse(&value)?,
        }),
        (ScriptType::Enum, SequenceEffectParamEditValue::Enum(value)) => {
            if !schema.options.contains(&value) {
                return Err(format!(
                    "`{value}` is not a valid option for `{}`",
                    schema.name
                ));
            }
            Ok(EffectParam::Enum { value })
        }
        (ScriptType::Flags, SequenceEffectParamEditValue::Flags(values)) => {
            for value in &values {
                if !schema.options.contains(value) {
                    return Err(format!(
                        "`{value}` is not a valid flag for `{}`",
                        schema.name
                    ));
                }
            }
            Ok(EffectParam::Flags {
                value: Flags { values },
            })
        }
        (ScriptType::CurveFloat, SequenceEffectParamEditValue::FloatCurve(points)) => {
            Ok(EffectParam::Curve {
                curve: InlineOrRef::Inline(edit_points_to_curve(CurveValueType::Float, points)?),
            })
        }
        (ScriptType::CurveColor, SequenceEffectParamEditValue::ColorCurve(points)) => {
            Ok(EffectParam::Curve {
                curve: InlineOrRef::Inline(edit_points_to_curve(CurveValueType::Color, points)?),
            })
        }
        (ScriptType::Marks, SequenceEffectParamEditValue::Marks(key)) => {
            validate_mark_collection_key(&key)?;
            Ok(EffectParam::Marks { key })
        }
        _ => Err(format!(
            "`{}` expects a {} parameter value",
            schema.name, schema.value_type
        )),
    }
}

fn edit_points_to_curve(
    value_type: CurveValueType,
    points: Vec<SequenceEffectParamCurvePointEditValue>,
) -> Result<Curve, String> {
    if points.is_empty() {
        return Err("curve parameters require at least one point".to_string());
    }
    let mut points = points
        .into_iter()
        .map(|point| {
            if !point.time.is_finite() {
                return Err("curve point time must be finite".to_string());
            }
            let value = match (value_type, point.value) {
                (CurveValueType::Float, SequenceEffectParamCurveValueEditValue::Float(value))
                    if value.is_finite() =>
                {
                    CurveValue::Float(value)
                }
                (CurveValueType::Float, SequenceEffectParamCurveValueEditValue::Float(_)) => {
                    return Err("curve point value must be finite".to_string())
                }
                (CurveValueType::Color, SequenceEffectParamCurveValueEditValue::Color(value)) => {
                    CurveValue::Color(Color::parse(&value)?)
                }
                _ => return Err("curve point value type does not match the curve type".to_string()),
            };
            Ok(CurvePoint {
                time: point.time.clamp(0.0, 1.0),
                value,
            })
        })
        .collect::<Result<Vec<_>, String>>()?;
    points.sort_by(|left, right| left.time.total_cmp(&right.time));
    Ok(Curve { value_type, points })
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
                members: group.members.to_vec(),
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
            let script_source =
                sequence_effect_script_details(path, effect, analysis).map(|(_, source)| source);
            let params = analysis
                .and_then(|analysis| sequence_effect_param_documents(path, effect, analysis))
                .unwrap_or_default();
            let render = layout.and_then(|layout| {
                analysis.and_then(|analysis| {
                    sequence_effect_render_document(path, effect, layout, analysis)
                })
            });
            SequenceEffectDocument {
                index,
                id: effect.id,
                start_ms: effect.start.milliseconds,
                duration_ms: effect.duration.milliseconds,
                target,
                target_label,
                scope: effect.scope,
                script: sequence_effect_script_label(&effect.script),
                script_source,
                params,
                render,
            }
        })
        .collect();
    SequenceDocument {
        path: path.to_slash_string(),
        object_key: object_key.to_string(),
        duration_ms: sequence.duration.milliseconds,
        frame_rate: sequence.frame_rate,
        audio: sequence_audio_document(fs, path, sequence.audio.as_ref()),
        mark_collections: sequence_mark_collection_documents(sequence),
        lanes,
        effect_scripts: sequence_effect_script_catalog(fs, path, overlays),
        effects,
        degraded: layout.is_none(),
    }
}

fn sequence_mark_collection_documents(
    sequence: &Sequence<Authored>,
) -> Vec<SequenceMarkCollectionDocument> {
    sequence
        .mark_collections
        .iter()
        .map(|collection| {
            let mut marks_ms = collection
                .marks
                .iter()
                .map(|mark| mark.milliseconds)
                .collect::<Vec<_>>();
            marks_ms.sort();
            SequenceMarkCollectionDocument {
                key: collection.key.clone(),
                name: collection.name.clone(),
                color: collection.color.clone(),
                marks_ms,
            }
        })
        .collect()
}

fn sequence_audio_document(
    fs: &WorkspaceFs,
    sequence_path: &Utf8PathBuf,
    audio: Option<&AssetPath>,
) -> Option<SequenceAudioDocument> {
    let audio = audio?;
    let resolved_path = resolve_import_path(sequence_path, audio.path());
    let file_name = resolved_path
        .file_name()
        .map(str::to_string)
        .unwrap_or_else(|| audio.raw().to_string());
    Some(SequenceAudioDocument {
        import: audio.raw().to_string(),
        resolved_path: resolved_path.to_slash_string(),
        file_name,
        exists: fs.exists(&resolved_path),
    })
}

fn sequence_effect_script_catalog(
    fs: &WorkspaceFs,
    sequence_path: &Utf8PathBuf,
    overlays: &[ProjectOverlay],
) -> Vec<SequenceEffectScriptDocument> {
    let mut by_path = BTreeMap::new();
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
                    params: compiled
                        .params
                        .into_iter()
                        .map(|param| SequenceEffectScriptParamDocument {
                            name: param.name,
                            value_type: param.value_type,
                        })
                        .collect(),
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

fn alias_for_import_path(path: &Utf8PathBuf) -> String {
    path.parent()
        .and_then(|parent| parent.file_name())
        .or_else(|| path.file_stem())
        .map(sanitize_alias)
        .filter(|alias| !alias.is_empty())
        .unwrap_or_else(|| "module".to_string())
}

fn module_import_for_path(
    source_path: &Utf8PathBuf,
    target_path: &Utf8PathBuf,
    imports: &[DawnImport],
) -> (String, Option<DawnImport>) {
    let target_dir = target_path
        .parent()
        .map(Utf8PathBuf::from)
        .unwrap_or_else(|| target_path.clone());
    let import_from = Utf8PathBuf::from(serialized_import_path(source_path, &target_dir));
    if let Some(existing) = imports.iter().find(|import| {
        resolve_import_path(source_path, &import.from)
            == resolve_import_path(source_path, &import_from)
    }) {
        return (existing.alias.clone(), None);
    }

    let base_alias = alias_for_import_path(target_path);
    let mut alias = base_alias.clone();
    let mut suffix = 2u32;
    while imports.iter().any(|import| import.alias == alias) {
        alias = format!("{base_alias}{suffix}");
        suffix += 1;
    }
    (
        alias.clone(),
        Some(DawnImport {
            from: import_from,
            alias,
        }),
    )
}

fn ensure_top_level_import(text: &str, import: DawnImport) -> Result<String, String> {
    let mut file: DawnFile = parse_dawn_file(text)?;
    if file
        .imports
        .iter()
        .any(|existing| existing.alias == import.alias && existing.from == import.from)
    {
        return Ok(text.to_string());
    }
    file.imports.push(import);
    serde_yaml::to_string(&file).map_err(|error| error.to_string())
}

fn sanitize_alias(value: &str) -> String {
    let mut alias = String::new();
    for character in value.chars() {
        if character.is_ascii_alphanumeric() || character == '_' {
            alias.push(character);
        } else if character == '-' {
            alias.push('_');
        }
    }
    if alias
        .chars()
        .next()
        .is_none_or(|character| !(character.is_ascii_alphabetic() || character == '_'))
    {
        alias.insert(0, '_');
    }
    alias
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

fn materialized_effect_params(
    script: &CompiledEffect,
    mark_collection_key: Option<&str>,
) -> IndexMap<String, EffectParam<Authored>> {
    script
        .params
        .iter()
        .map(|schema| {
            let param = if schema.value_type == ScriptType::Marks {
                EffectParam::Marks {
                    key: mark_collection_key.unwrap_or("marks").to_string(),
                }
            } else {
                match &schema.default {
                    Some(ParamDefault::Value(value)) => runtime_value_to_authored_param(value),
                    None => type_default_param(schema.value_type, &schema.options),
                }
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
            curve: InlineOrRef::Inline(curve.clone()),
        },
        RuntimeValue::Enum(value) => EffectParam::Enum {
            value: value.clone(),
        },
        RuntimeValue::Flags(value) => EffectParam::Flags {
            value: value.clone(),
        },
        RuntimeValue::Marks(_) => unreachable!("marks params cannot declare defaults"),
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
            curve: InlineOrRef::Inline(Curve {
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
            curve: InlineOrRef::Inline(Curve {
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
        ScriptType::Marks => EffectParam::Marks {
            key: "marks".to_string(),
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

fn sort_sequence_mark_collections(sequence: &mut Sequence<Authored>) {
    for collection in &mut sequence.mark_collections {
        collection.marks.sort_by_key(|mark| mark.milliseconds);
    }
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
    let (script_key, script_source) =
        sequence_effect_script_details(sequence_path, effect, Some(analysis))?;
    if analysis
        .scripts
        .get(&script_key)
        .is_none_or(|script| script.result.is_err())
    {
        return None;
    }
    let params = sequence_effect_param_documents(sequence_path, effect, analysis)?;

    Some(SequenceEffectRenderDocument {
        script_key,
        script_source,
        params,
        target_pixels,
    })
}

fn sequence_effect_script_details(
    sequence_path: &Utf8PathBuf,
    effect: &SequenceEffect<Authored>,
    analysis: Option<&ProjectAnalysis>,
) -> Option<(String, String)> {
    match &effect.script {
        InlineScriptOrRef::Inline { inline } => Some((
            format!(
                "inline:{}:{}",
                analysis?.root_path.to_slash_string(),
                effect.id
            ),
            inline.clone(),
        )),
        InlineScriptOrRef::Ref(reference) => {
            let analysis = analysis?;
            let mut resolver = AnalysisImportResolver {
                files: &analysis.files,
                scripts: &analysis.scripts,
            };
            let path = resolver
                .resolve_effect(sequence_path, reference)
                .ok()?
                .source_path;
            let key = path.to_slash_string();
            let source = analysis
                .files
                .get(&path)
                .and_then(|file| file.text.clone())
                .unwrap_or_else(|| key.clone());
            Some((key, source))
        }
    }
}

fn sequence_effect_param_documents(
    sequence_path: &Utf8PathBuf,
    effect: &SequenceEffect<Authored>,
    analysis: &ProjectAnalysis,
) -> Option<Vec<SequenceEffectParamDocument>> {
    let mut resolver = AnalysisImportResolver {
        files: &analysis.files,
        scripts: &analysis.scripts,
    };
    effect
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
        .ok()
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
                    pixel_count: emitter_count,
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
                InlineOrRef::Inline(curve) => curve.clone(),
                InlineOrRef::Ref(reference) => {
                    let resolved = resolver
                        .resolve_object(source_path, reference, ObjectKind::Curve)
                        .map_err(|error| error.to_string())?;
                    let DawnObject::Curve(curve) = resolved.object else {
                        return Err(format!("reference `{}` is not a curve", reference.raw()));
                    };
                    curve
                }
            },
        },
        EffectParam::Marks { key } => EffectParam::Marks { key: key.clone() },
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

fn sequence_effect_script_label(script: &InlineScriptOrRef) -> String {
    match script {
        InlineScriptOrRef::Inline { inline } => {
            inline.lines().next().unwrap_or("Inline").to_string()
        }
        InlineScriptOrRef::Ref(reference) => reference.raw().to_string(),
    }
}

fn placement_to_document(
    placement: &FixturePlacement<Authored>,
    resolved: &FixturePlacement<Resolved>,
    source_path: &Utf8PathBuf,
) -> LayoutFixturePlacement {
    let (fixture, resolved_source_path, resolved_object_key) = match &placement.fixture {
        InlineOrRef::Inline(fixture) => (
            LayoutFixtureRef::Inline {
                name: fixture.name.clone(),
                color_model: fixture.color_model,
                bulb_size: fixture.bulb_size,
                geometry: fixture.geometry.clone(),
            },
            source_path.to_slash_string(),
            None,
        ),
        InlineOrRef::Ref(reference) => (
            LayoutFixtureRef::Import {
                import: reference.raw().to_string(),
                object_key: Some(reference.name().as_str().to_string()),
                source_path: None,
            },
            source_path.to_slash_string(),
            Some(reference.name().as_str().to_string()),
        ),
    };

    LayoutFixturePlacement {
        id: placement.id,
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
    let visible_imports = analysis
        .files
        .get(importing_source_path)
        .and_then(|analyzed| analyzed.file.as_ref())
        .map(|file| file.imports.as_slice())
        .unwrap_or(&[]);
    for (path, analyzed) in &analysis.files {
        let Some(file) = analyzed.file.as_ref() else {
            continue;
        };
        for (key, object) in file {
            let DawnObject::Fixture(fixture) = object else {
                continue;
            };
            let source_path = path.to_slash_string();
            let import_string = visible_imports
                .iter()
                .find_map(|import| {
                    let resolved = resolve_import_path(importing_source_path, &import.from);
                    if &resolved == path || path.parent() == Some(resolved.as_path()) {
                        Some(format!("{}.{}", import.alias, key))
                    } else {
                        None
                    }
                })
                .unwrap_or_else(|| {
                    let import_path = serialized_import_path(importing_source_path, path);
                    format!("{import_path}::{key}")
                });
            catalog.push(FixtureCatalogItem {
                object_key: key.clone(),
                source_path: source_path.clone(),
                import_string,
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
        LayoutFixtureRef::Import { import, .. } => InlineOrRef::Ref(SymbolRef::new(import)?),
        LayoutFixtureRef::Inline {
            name,
            color_model,
            bulb_size,
            geometry,
        } => InlineOrRef::Inline(Fixture {
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

fn validate_mark_collection_key(value: &str) -> Result<(), String> {
    let mut chars = value.chars();
    let Some(first) = chars.next() else {
        return Err("mark collection key cannot be empty".to_string());
    };
    if !first.is_ascii_lowercase() {
        return Err("mark collection key must start with a lowercase ASCII letter".to_string());
    }
    if chars.any(|character| {
        !(character.is_ascii_lowercase() || character.is_ascii_digit() || character == '_')
    }) {
        return Err(
            "mark collection key may only contain lowercase ASCII letters, numbers, and underscores"
                .to_string(),
        );
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
