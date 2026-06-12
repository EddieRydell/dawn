use std::collections::{BTreeMap, BTreeSet};

use camino::Utf8Path;
use dawn_language::effect::{
    CurveSource, EffectDefinitionId, EffectParamValue, EffectScope, EffectTarget,
};
use dawn_language::effect_dsl::{EffectKind, Type, Value as EffectValue};
use dawn_language::sequence::SequenceId;
use dawn_language::setup::{
    FixtureDefinitionId, FixtureGroupId, FixtureInstanceId, Geometry as DomainGeometry, LayoutId,
    LayoutTarget as DomainLayoutTarget,
};
use dawn_language::values::{Color, CurveValue, Distance, DistanceSpan, Point3};
use dawn_project_io::{
    ProjectSession, SourceDocumentKind, SourceMap, SourceObjectId, SourceObjectKind,
    SourceObjectLocation,
};
use yaml_serde::{Mapping, Value};

use crate::dto::{
    ColorCurvePoint, DiagnosticSeverity, DocumentViewId, EffectScriptReference, FixtureDefinition,
    FixtureGuiDocument, FixtureGuiEdit, FloatCurvePoint, Geometry, GeometryRenderBounds,
    GeometryRenderGuide, GeometryRenderPlan, GeometryRenderPoint, GuiDocument, GuiDocumentRequest,
    GuiEditCommand, GuiObjectRef, LayoutFixturePlacement, LayoutGuiDocument, LayoutGuiEdit,
    LayoutTarget, LayoutTargetKind, ObjectKind, Point3Meters, ProjectDiagnostic,
    ResolvedLayoutFixture, Rotation3Degrees, Scale3, SequenceAudio, SequenceCurveLibraryItem,
    SequenceCurveLibraryPoints, SequenceCurveValueType, SequenceEffect, SequenceEffectParam,
    SequenceEffectParamCurveSource, SequenceEffectParamKind, SequenceEffectParamValue,
    SequenceEffectScope, SequenceEffectScript, SequenceEffectScriptKind, SequenceEffectScriptParam,
    SequenceGuiDocument, SequenceGuiEdit, SequenceLane, SequenceMarkCollection, SequenceMarkRef,
    SequencePasteAnchor, SequenceResizeEdge, SequenceSelection, SequenceSelectionEdit, Transform,
};

#[derive(Debug)]
pub enum GuiMutationError {
    Blocked(String),
    Invalid(String),
}

impl GuiMutationError {
    pub fn message(&self) -> &str {
        match self {
            Self::Blocked(message) | Self::Invalid(message) => message,
        }
    }
}

pub fn blocked(reason: impl Into<String>, diagnostics: Vec<ProjectDiagnostic>) -> GuiDocument {
    GuiDocument::Blocked {
        reason: reason.into(),
        diagnostics,
    }
}

pub fn project_gui_document(
    session: Option<&ProjectSession>,
    request: &GuiDocumentRequest,
) -> GuiDocument {
    let Some(session) = session else {
        return blocked("No project is loaded.", Vec::new());
    };
    let resolved = match resolve_request(session, request) {
        Ok(resolved) => resolved,
        Err(message) => {
            return blocked(
                message.clone(),
                vec![gui_diagnostic(&request.path, "gui.resolve", &message)],
            )
        }
    };
    match request.view {
        DocumentViewId::Sequence => project_sequence(session, &resolved),
        DocumentViewId::Layout => project_layout(session, &resolved),
        DocumentViewId::Fixture => project_fixture(session, &resolved),
        DocumentViewId::Text => blocked(
            "Text documents do not have a GUI projection.",
            vec![gui_diagnostic(
                &request.path,
                "gui.view",
                "Text documents do not have a GUI projection.",
            )],
        ),
    }
}

pub fn affected_paths(
    session: &ProjectSession,
    request: &GuiDocumentRequest,
    edit: &GuiEditCommand,
) -> Result<BTreeSet<String>, GuiMutationError> {
    let resolved = resolve_request(session, request).map_err(GuiMutationError::Invalid)?;
    match (request.view.clone(), edit) {
        (DocumentViewId::Sequence, GuiEditCommand::Sequence { .. })
        | (DocumentViewId::Layout, GuiEditCommand::Layout { .. })
        | (DocumentViewId::Fixture, GuiEditCommand::Fixture { .. }) => {
            Ok(BTreeSet::from([resolved.location.document.to_string()]))
        }
        _ => Err(GuiMutationError::Invalid(
            "GUI edit type does not match the requested document view.".to_string(),
        )),
    }
}

pub fn apply_edit(
    session: &mut ProjectSession,
    request: &GuiDocumentRequest,
    edit: GuiEditCommand,
) -> Result<(), GuiMutationError> {
    let resolved = resolve_request(session, request).map_err(GuiMutationError::Invalid)?;
    match (request.view.clone(), edit) {
        (DocumentViewId::Sequence, GuiEditCommand::Sequence { edit }) => {
            edit_sequence(session, &resolved, edit)
        }
        (DocumentViewId::Layout, GuiEditCommand::Layout { edit }) => {
            edit_layout(session, &resolved.location, edit)
        }
        (DocumentViewId::Fixture, GuiEditCommand::Fixture { edit }) => {
            edit_fixture(session, &resolved.location, edit)
        }
        _ => Err(GuiMutationError::Invalid(
            "GUI edit type does not match the requested document view.".to_string(),
        )),
    }
}

#[derive(Clone)]
pub(crate) enum SequenceClipboard {
    Effects(Vec<ClipboardEffect>),
    Marks(Vec<ClipboardMark>),
}

#[derive(Clone)]
pub(crate) struct ClipboardEffect {
    value: Value,
    start_seconds: f64,
    lane_index: usize,
}

#[derive(Clone)]
pub(crate) struct ClipboardMark {
    collection_key: String,
    time_seconds: f64,
}

pub(crate) struct SequenceSelectionMutation {
    pub selection: Option<SequenceSelection>,
    pub copied_count: u32,
    pub skipped_count: u32,
}

pub(crate) fn apply_sequence_selection_edit(
    session: &mut ProjectSession,
    request: &GuiDocumentRequest,
    edit: SequenceSelectionEdit,
    clipboard: &mut Option<SequenceClipboard>,
) -> Result<SequenceSelectionMutation, GuiMutationError> {
    if !matches!(request.view, DocumentViewId::Sequence) {
        return Err(GuiMutationError::Invalid(
            "Sequence selection edits require a sequence GUI document.".to_string(),
        ));
    }
    let resolved = resolve_request(session, request).map_err(GuiMutationError::Invalid)?;
    let sequence_id = SequenceId(resolved.source_id.id.clone());
    match edit {
        SequenceSelectionEdit::Copy { selection } => {
            let (next_clipboard, copied_count, skipped_count) =
                copy_sequence_selection(session, &resolved.location, &sequence_id, &selection)?;
            *clipboard = next_clipboard;
            Ok(SequenceSelectionMutation {
                selection: Some(selection),
                copied_count,
                skipped_count,
            })
        }
        SequenceSelectionEdit::Cut { selection } => {
            let (next_clipboard, copied_count, skipped_count) =
                copy_sequence_selection(session, &resolved.location, &sequence_id, &selection)?;
            *clipboard = next_clipboard;
            delete_sequence_selection(session, &resolved.location, &selection)?;
            Ok(SequenceSelectionMutation {
                selection: None,
                copied_count,
                skipped_count,
            })
        }
        SequenceSelectionEdit::Delete { selection } => {
            delete_sequence_selection(session, &resolved.location, &selection)?;
            Ok(SequenceSelectionMutation {
                selection: None,
                copied_count: 0,
                skipped_count: 0,
            })
        }
        SequenceSelectionEdit::Paste { anchor } => {
            paste_sequence_clipboard(session, &resolved.location, anchor, clipboard.as_ref())
        }
        SequenceSelectionEdit::MoveEffects {
            ids,
            time_delta_seconds,
            lane_delta,
        } => {
            let moved = move_effect_selection(
                session,
                &resolved.location,
                &sequence_id,
                &ids,
                time_delta_seconds,
                lane_delta,
            )?;
            Ok(SequenceSelectionMutation {
                selection: Some(SequenceSelection::Effects { ids: moved }),
                copied_count: 0,
                skipped_count: 0,
            })
        }
        SequenceSelectionEdit::ResizeEffects {
            ids,
            edge,
            time_delta_seconds,
        } => {
            resize_effect_selection(
                session,
                &resolved.location,
                &sequence_id,
                &ids,
                edge,
                time_delta_seconds,
            )?;
            Ok(SequenceSelectionMutation {
                selection: Some(SequenceSelection::Effects { ids }),
                copied_count: 0,
                skipped_count: 0,
            })
        }
        SequenceSelectionEdit::MoveMarks {
            marks,
            time_delta_seconds,
        } => {
            let moved =
                move_mark_selection(session, &resolved.location, &marks, time_delta_seconds)?;
            Ok(SequenceSelectionMutation {
                selection: Some(SequenceSelection::Marks { marks: moved }),
                copied_count: 0,
                skipped_count: 0,
            })
        }
    }
}

struct ResolvedGuiObject {
    source_ref: GuiObjectRef,
    source_id: SourceObjectId,
    location: SourceObjectLocation,
}

fn resolve_request(
    session: &ProjectSession,
    request: &GuiDocumentRequest,
) -> Result<ResolvedGuiObject, String> {
    let path = Utf8Path::new(&request.path);
    let kind = source_kind_for_view(&request.view)?;
    let requested_key = request.object_key.as_deref();
    let mut matches = session
        .source
        .source_map
        .objects
        .iter()
        .filter(|(id, location)| id.kind == kind && location.document == path)
        .filter(|(_, location)| {
            requested_key.is_none_or(|key| location.object_key.as_str() == key)
        });
    let Some((source_id, location)) = matches.next() else {
        return Err("No matching GUI object was found for this request.".to_string());
    };
    if matches.next().is_some() && requested_key.is_none() {
        return Err("GUI request must include an object key for this document.".to_string());
    }
    Ok(ResolvedGuiObject {
        source_ref: GuiObjectRef {
            path: location.document.to_string(),
            object_key: location.object_key.clone(),
            kind: object_kind_for_source(&source_id.kind),
            id: source_id.id.clone(),
        },
        source_id: source_id.clone(),
        location: location.clone(),
    })
}

fn source_kind_for_view(view: &DocumentViewId) -> Result<SourceObjectKind, String> {
    match view {
        DocumentViewId::Sequence => Ok(SourceObjectKind::Sequence),
        DocumentViewId::Layout => Ok(SourceObjectKind::Layout),
        DocumentViewId::Fixture => Ok(SourceObjectKind::FixtureDefinition),
        DocumentViewId::Text => Err("Text view has no source GUI object kind.".to_string()),
    }
}

fn object_kind_for_source(kind: &SourceObjectKind) -> ObjectKind {
    match kind {
        SourceObjectKind::Project => ObjectKind::Project,
        SourceObjectKind::Setup => ObjectKind::Setup,
        SourceObjectKind::Controller => ObjectKind::Controller,
        SourceObjectKind::Layout => ObjectKind::Layout,
        SourceObjectKind::Patch => ObjectKind::Patch,
        SourceObjectKind::FixtureDefinition => ObjectKind::Fixture,
        SourceObjectKind::Curve => ObjectKind::Curve,
        SourceObjectKind::Sequence => ObjectKind::Sequence,
        SourceObjectKind::EffectDefinition | SourceObjectKind::EffectInstance => ObjectKind::Effect,
    }
}

fn project_sequence(session: &ProjectSession, resolved: &ResolvedGuiObject) -> GuiDocument {
    let id = SequenceId(resolved.source_id.id.clone());
    let Some(sequence) = session.project.sequences.get(&id) else {
        return blocked(
            "Sequence is not available in the checked project model.",
            vec![gui_diagnostic(
                resolved.location.document.as_ref(),
                "gui.sequence",
                "Sequence is not available in the checked project model.",
            )],
        );
    };
    let layout = active_layout_id(session).and_then(|id| session.project.layouts.get(&id));
    let lanes = layout
        .map(|layout| {
            layout
                .target_order
                .iter()
                .map(|target| SequenceLane {
                    target: layout_target(target),
                    label: layout_target_label(session, target),
                })
                .collect()
        })
        .unwrap_or_default();
    let effects = sequence
        .effects
        .iter()
        .enumerate()
        .map(|(index, effect)| SequenceEffect {
            index: index as u32,
            id: effect.id.0,
            start_seconds: effect.start.as_seconds_f64(),
            duration_seconds: effect.duration.as_seconds_f64(),
            target: effect_target(&effect.target),
            target_label: effect_target_label(session, &effect.target),
            scope: match effect.scope {
                EffectScope::PerFixture => SequenceEffectScope::PerFixture,
                EffectScope::WholeTarget => SequenceEffectScope::WholeTarget,
            },
            script: effect.definition.0.clone(),
            script_source: effect_script_ref(session, &effect.definition.0),
            params: effect_params(session, effect),
        })
        .collect();
    GuiDocument::Sequence {
        document: SequenceGuiDocument {
            path: resolved.location.document.to_string(),
            source_ref: resolved.source_ref.clone(),
            object_key: resolved.location.object_key.clone(),
            duration_seconds: sequence.duration.as_seconds_f64(),
            frame_rate: sequence.frame_rate as f64,
            audio: sequence_audio(session, &sequence.audio),
            mark_collections: sequence
                .mark_collections
                .iter()
                .map(|collection| SequenceMarkCollection {
                    key: collection.key.name.clone(),
                    name: collection.name.clone(),
                    color: color_hex(collection.display_color),
                    marks_seconds: collection
                        .marks
                        .iter()
                        .map(|mark| mark.as_seconds_f64())
                        .collect(),
                })
                .collect(),
            lanes,
            effect_scripts: effect_scripts(session),
            curve_library: curve_library(session),
            effects,
            degraded: false,
        },
    }
}

fn project_layout(session: &ProjectSession, resolved: &ResolvedGuiObject) -> GuiDocument {
    let id = LayoutId(resolved.source_id.id.clone());
    let Some(layout) = session.project.layouts.get(&id) else {
        return blocked(
            "Layout is not available in the checked project model.",
            Vec::new(),
        );
    };
    let fixtures = layout
        .fixtures
        .iter()
        .map(|fixture| {
            let definition_ref =
                fixture_source_ref(&session.source.source_map, &fixture.definition);
            let resolved_fixture = session
                .project
                .definitions
                .fixtures
                .get(&fixture.definition)
                .map(|definition| ResolvedLayoutFixture {
                    name: fixture.definition.0.clone(),
                    color_model: "rgb".to_string(),
                    bulb_diameter_meters: distance_span_meters(definition.bulb_radius) * 2.0,
                    geometry_summary: geometry_summary(&definition.geometry),
                    render_plan: render_plan(&definition.geometry, definition.bulb_radius),
                    source_path: definition_ref
                        .as_ref()
                        .map(|source_ref| source_ref.path.clone())
                        .unwrap_or_default(),
                    object_key: definition_ref.map(|source_ref| source_ref.object_key),
                })
                .unwrap_or_else(empty_resolved_fixture);
            LayoutFixturePlacement {
                source_ref: GuiObjectRef {
                    path: resolved.location.document.to_string(),
                    object_key: resolved.location.object_key.clone(),
                    kind: ObjectKind::Fixture,
                    id: fixture.id.0.to_string(),
                },
                id: fixture.id.0,
                name: fixture.name.clone(),
                transform: Transform {
                    position: point3_meters(fixture.position),
                    rotation: Rotation3Degrees {
                        x_degrees: fixture.rotation.x,
                        y_degrees: fixture.rotation.y,
                        z_degrees: fixture.rotation.z,
                    },
                    scale: Scale3 {
                        x: fixture.scale.x,
                        y: fixture.scale.y,
                        z: fixture.scale.z,
                    },
                },
                resolved_fixture,
            }
        })
        .collect::<Vec<_>>();
    let render_bounds = layout_bounds(&fixtures);
    GuiDocument::Layout {
        document: LayoutGuiDocument {
            path: resolved.location.document.to_string(),
            source_ref: resolved.source_ref.clone(),
            object_key: resolved.location.object_key.clone(),
            name: resolved.location.object_key.clone(),
            render_bounds,
            fixtures,
        },
    }
}

fn project_fixture(session: &ProjectSession, resolved: &ResolvedGuiObject) -> GuiDocument {
    let fixtures = session
        .source
        .source_map
        .objects
        .iter()
        .filter(|(id, location)| {
            id.kind == SourceObjectKind::FixtureDefinition
                && location.document == resolved.location.document
        })
        .filter_map(|(id, location)| {
            let definition_id = FixtureDefinitionId(id.id.clone());
            let definition = session.project.definitions.fixtures.get(&definition_id)?;
            let source_ref = GuiObjectRef {
                path: location.document.to_string(),
                object_key: location.object_key.clone(),
                kind: ObjectKind::Fixture,
                id: id.id.clone(),
            };
            Some(FixtureDefinition {
                source_ref,
                object_key: location.object_key.clone(),
                name: location.object_key.clone(),
                color_model: "rgb".to_string(),
                bulb_diameter_meters: distance_span_meters(definition.bulb_radius) * 2.0,
                geometry: geometry(&definition.geometry),
                geometry_summary: geometry_summary(&definition.geometry),
                render_plan: render_plan(&definition.geometry, definition.bulb_radius),
            })
        })
        .collect::<Vec<_>>();
    GuiDocument::Fixture {
        document: FixtureGuiDocument {
            path: resolved.location.document.to_string(),
            source_ref: Some(resolved.source_ref.clone()),
            selected_object_key: Some(resolved.location.object_key.clone()),
            fixtures,
        },
    }
}

fn active_layout_id(session: &ProjectSession) -> Option<LayoutId> {
    session
        .project
        .setups
        .get(&session.project.root.setup)
        .map(|setup| setup.layout.clone())
}

fn sequence_audio(
    session: &ProjectSession,
    audio: &dawn_language::sequence::SequenceAudio,
) -> Option<SequenceAudio> {
    let dawn_language::sequence::SequenceAudio::Asset(id) = audio else {
        return None;
    };
    session
        .source
        .referenced_assets
        .iter()
        .find(|asset| asset.id == *id)
        .map(|asset| SequenceAudio {
            import_path: asset.relative_path.to_string(),
            resolved_path: asset.absolute_path.to_string(),
            file_name: asset
                .relative_path
                .file_name()
                .map(ToString::to_string)
                .unwrap_or_else(|| asset.relative_path.to_string()),
            exists: asset.absolute_path.is_file(),
        })
}

fn layout_target(target: &DomainLayoutTarget) -> LayoutTarget {
    match target {
        DomainLayoutTarget::Fixture(id) => LayoutTarget {
            kind: LayoutTargetKind::Fixture,
            name: id.0.to_string(),
        },
        DomainLayoutTarget::Group(id) => LayoutTarget {
            kind: LayoutTargetKind::Group,
            name: id.0.to_string(),
        },
    }
}

fn effect_target(target: &EffectTarget) -> LayoutTarget {
    match target {
        EffectTarget::Fixture(id) => LayoutTarget {
            kind: LayoutTargetKind::Fixture,
            name: id.0.to_string(),
        },
        EffectTarget::Group(id) => LayoutTarget {
            kind: LayoutTargetKind::Group,
            name: id.0.to_string(),
        },
    }
}

fn layout_target_label(session: &ProjectSession, target: &DomainLayoutTarget) -> String {
    let Some(layout_id) = active_layout_id(session) else {
        return layout_target(target).name;
    };
    let Some(layout) = session.project.layouts.get(&layout_id) else {
        return layout_target(target).name;
    };
    match target {
        DomainLayoutTarget::Fixture(id) => layout
            .fixtures
            .iter()
            .find(|fixture| fixture.id == *id)
            .map(|fixture| fixture.name.clone())
            .unwrap_or_else(|| format!("Fixture {}", id.0)),
        DomainLayoutTarget::Group(id) => layout
            .groups
            .iter()
            .find(|group| group.id == *id)
            .map(|group| group.name.clone())
            .unwrap_or_else(|| format!("Group {}", id.0)),
    }
}

fn effect_target_label(session: &ProjectSession, target: &EffectTarget) -> String {
    match target {
        EffectTarget::Fixture(id) => {
            layout_target_label(session, &DomainLayoutTarget::Fixture(id.clone()))
        }
        EffectTarget::Group(id) => {
            layout_target_label(session, &DomainLayoutTarget::Group(id.clone()))
        }
    }
}

fn effect_script_ref(session: &ProjectSession, id: &str) -> Option<EffectScriptReference> {
    source_location(
        &session.source.source_map,
        SourceObjectKind::EffectDefinition,
        id,
    )
    .map(|location| EffectScriptReference {
        path: location.document.to_string(),
        effect_name: location.object_key,
    })
}

fn effect_scripts(session: &ProjectSession) -> Vec<SequenceEffectScript> {
    session
        .project
        .definitions
        .effects
        .definitions
        .iter()
        .filter_map(|(id, definition)| {
            let source = effect_script_ref(session, &id.0)?;
            Some(SequenceEffectScript {
                name: id.0.clone(),
                kind: match definition.compiled.kind() {
                    EffectKind::Sample => SequenceEffectScriptKind::Sample,
                    EffectKind::Generator => SequenceEffectScriptKind::Generator,
                },
                script: source.clone(),
                import_path: source.path.clone(),
                params: definition
                    .compiled
                    .params()
                    .iter()
                    .filter_map(|param| {
                        Some(SequenceEffectScriptParam {
                            name: param.name.as_str().to_string(),
                            kind: param_kind(&param.ty)?,
                        })
                    })
                    .collect(),
            })
        })
        .collect()
}

fn effect_params(
    session: &ProjectSession,
    effect: &dawn_language::effect::EffectInst,
) -> Vec<SequenceEffectParam> {
    let Some(definition) = session.project.definitions.effects.get(&effect.definition) else {
        return Vec::new();
    };
    definition
        .compiled
        .params()
        .iter()
        .filter_map(|param| {
            let kind = param_kind(&param.ty)?;
            let override_value = effect.param_overrides.get(&param.name);
            let value = override_value
                .map(|value| effect_param_value(session, value))
                .or_else(|| param.default.as_ref().and_then(default_param_value))
                .or_else(|| default_value_for_type(&param.ty))?;
            Some(SequenceEffectParam {
                name: param.name.as_str().to_string(),
                kind,
                options: param_options(&param.ty),
                editable: true,
                curve_source: override_value.and_then(|value| curve_source(session, value)),
                value,
            })
        })
        .collect()
}

fn curve_library(session: &ProjectSession) -> Vec<SequenceCurveLibraryItem> {
    session
        .project
        .definitions
        .curves
        .definitions
        .iter()
        .filter_map(|(id, definition)| {
            let location =
                source_location(&session.source.source_map, SourceObjectKind::Curve, &id.0)?;
            let points = curve_points(&definition.curve.points)?;
            Some(SequenceCurveLibraryItem {
                path: location.document.to_string(),
                object_key: location.object_key.clone(),
                display_name: location.object_key,
                value_type: match &points {
                    SequenceCurveLibraryPoints::Float { .. } => SequenceCurveValueType::Float,
                    SequenceCurveLibraryPoints::Color { .. } => SequenceCurveValueType::Color,
                },
                points,
            })
        })
        .collect()
}

fn source_location(
    source_map: &SourceMap,
    kind: SourceObjectKind,
    id: &str,
) -> Option<SourceObjectLocation> {
    source_map
        .objects
        .get(&SourceObjectId {
            kind,
            id: id.to_string(),
        })
        .cloned()
}

fn fixture_source_ref(source_map: &SourceMap, id: &FixtureDefinitionId) -> Option<GuiObjectRef> {
    source_location(source_map, SourceObjectKind::FixtureDefinition, &id.0).map(|location| {
        GuiObjectRef {
            path: location.document.to_string(),
            object_key: location.object_key,
            kind: ObjectKind::Fixture,
            id: id.0.clone(),
        }
    })
}

fn param_kind(ty: &Type) -> Option<SequenceEffectParamKind> {
    Some(match ty {
        Type::Int => SequenceEffectParamKind::Int,
        Type::Float => SequenceEffectParamKind::Float,
        Type::Bool => SequenceEffectParamKind::Bool,
        Type::Color => SequenceEffectParamKind::Color,
        Type::Enum(_) => SequenceEffectParamKind::Enum,
        Type::Marks => SequenceEffectParamKind::Marks,
        Type::Curve(inner) => match inner.as_ref() {
            Type::Color => SequenceEffectParamKind::ColorCurve,
            _ => SequenceEffectParamKind::FloatCurve,
        },
        Type::Array(inner) => match inner.as_ref() {
            Type::Int => SequenceEffectParamKind::IntArray,
            Type::Float => SequenceEffectParamKind::FloatArray,
            Type::Bool => SequenceEffectParamKind::BoolArray,
            Type::Color => SequenceEffectParamKind::ColorArray,
            Type::Curve(curve_inner) => match curve_inner.as_ref() {
                Type::Color => SequenceEffectParamKind::ColorCurveArray,
                _ => SequenceEffectParamKind::FloatCurveArray,
            },
            _ => SequenceEffectParamKind::FloatArray,
        },
        Type::Void | Type::Timeline | Type::Target | Type::TargetItems | Type::TargetItem => {
            return None;
        }
    })
}

fn param_options(ty: &Type) -> Vec<String> {
    match ty {
        Type::Enum(options) => options
            .iter()
            .map(|option| option.as_str().to_string())
            .collect(),
        _ => Vec::new(),
    }
}

fn effect_param_value(
    session: &ProjectSession,
    value: &EffectParamValue,
) -> SequenceEffectParamValue {
    match value {
        EffectParamValue::Int(value) => SequenceEffectParamValue::Int {
            value: *value as f64,
        },
        EffectParamValue::Float(value) => SequenceEffectParamValue::Float { value: *value },
        EffectParamValue::Bool(value) => SequenceEffectParamValue::Bool { value: *value },
        EffectParamValue::Color(value) => SequenceEffectParamValue::Color {
            value: color_hex(*value),
        },
        EffectParamValue::Enum(value) => SequenceEffectParamValue::Enum {
            value: value.as_str().to_string(),
        },
        EffectParamValue::Marks(value) => SequenceEffectParamValue::Marks {
            key: value.name.clone(),
        },
        EffectParamValue::Curve(source) => match source {
            CurveSource::Inline(curve) => curve_points(&curve.points)
                .map(curve_points_param_value)
                .unwrap_or_else(|| SequenceEffectParamValue::FloatCurve { points: Vec::new() }),
            CurveSource::Reference(id) => session
                .project
                .definitions
                .curves
                .get(id)
                .and_then(|definition| curve_points(&definition.curve.points))
                .map(curve_points_param_value)
                .unwrap_or_else(|| SequenceEffectParamValue::FloatCurve { points: Vec::new() }),
        },
        EffectParamValue::Array(values) => array_param_value(session, values),
    }
}

fn default_param_value(value: &EffectValue) -> Option<SequenceEffectParamValue> {
    Some(match value {
        EffectValue::Int(value) => SequenceEffectParamValue::Int {
            value: *value as f64,
        },
        EffectValue::Float(value) => SequenceEffectParamValue::Float { value: *value },
        EffectValue::Bool(value) => SequenceEffectParamValue::Bool { value: *value },
        EffectValue::Color(value) => SequenceEffectParamValue::Color {
            value: color_hex(*value),
        },
        EffectValue::Enum(value) => SequenceEffectParamValue::Enum {
            value: value.as_str().to_string(),
        },
        EffectValue::Marks(_) => SequenceEffectParamValue::Marks { key: String::new() },
        EffectValue::Curve(curve) => curve_points(&curve.points)
            .map(curve_points_param_value)
            .unwrap_or_else(|| SequenceEffectParamValue::FloatCurve { points: Vec::new() }),
        EffectValue::Array(values) => {
            let converted = values
                .iter()
                .map(default_param_value)
                .collect::<Option<Vec<_>>>()?;
            array_param_from_sequence_values(&converted)
        }
        EffectValue::Void
        | EffectValue::Target(_)
        | EffectValue::TargetItems(_)
        | EffectValue::TargetItem(_) => return None,
    })
}

fn default_value_for_type(ty: &Type) -> Option<SequenceEffectParamValue> {
    Some(match ty {
        Type::Int => SequenceEffectParamValue::Int { value: 0.0 },
        Type::Float => SequenceEffectParamValue::Float { value: 0.0 },
        Type::Bool => SequenceEffectParamValue::Bool { value: false },
        Type::Color => SequenceEffectParamValue::Color {
            value: "#ffffff".to_string(),
        },
        Type::Enum(options) => SequenceEffectParamValue::Enum {
            value: options
                .first()
                .map(|option| option.as_str().to_string())
                .unwrap_or_default(),
        },
        Type::Marks => SequenceEffectParamValue::Marks { key: String::new() },
        Type::Curve(inner) => match inner.as_ref() {
            Type::Color => SequenceEffectParamValue::ColorCurve { points: Vec::new() },
            _ => SequenceEffectParamValue::FloatCurve { points: Vec::new() },
        },
        Type::Array(inner) => match param_kind(inner) {
            Some(SequenceEffectParamKind::Int) => {
                SequenceEffectParamValue::IntArray { values: Vec::new() }
            }
            Some(SequenceEffectParamKind::Float) => {
                SequenceEffectParamValue::FloatArray { values: Vec::new() }
            }
            Some(SequenceEffectParamKind::Bool) => {
                SequenceEffectParamValue::BoolArray { values: Vec::new() }
            }
            Some(SequenceEffectParamKind::Color) => {
                SequenceEffectParamValue::ColorArray { values: Vec::new() }
            }
            Some(SequenceEffectParamKind::ColorCurve) => {
                SequenceEffectParamValue::ColorCurveArray { values: Vec::new() }
            }
            _ => SequenceEffectParamValue::FloatCurveArray { values: Vec::new() },
        },
        Type::Void | Type::Timeline | Type::Target | Type::TargetItems | Type::TargetItem => {
            return None;
        }
    })
}

fn curve_source(
    session: &ProjectSession,
    value: &EffectParamValue,
) -> Option<SequenceEffectParamCurveSource> {
    match value {
        EffectParamValue::Curve(CurveSource::Inline(_)) => {
            Some(SequenceEffectParamCurveSource::Inline)
        }
        EffectParamValue::Curve(CurveSource::Reference(id)) => {
            let location =
                source_location(&session.source.source_map, SourceObjectKind::Curve, &id.0);
            Some(SequenceEffectParamCurveSource::Library {
                reference: id.0.clone(),
                path: location
                    .as_ref()
                    .map(|location| location.document.to_string()),
                object_key: location
                    .as_ref()
                    .map(|location| location.object_key.clone()),
                display_name: Some(
                    location
                        .as_ref()
                        .map(|location| location.object_key.clone())
                        .unwrap_or_else(|| id.0.clone()),
                ),
            })
        }
        _ => None,
    }
}

fn curve_points(
    points: &[dawn_language::values::CurvePoint],
) -> Option<SequenceCurveLibraryPoints> {
    let first = points.first()?;
    match first.value {
        CurveValue::Float(_) => Some(SequenceCurveLibraryPoints::Float {
            points: points
                .iter()
                .filter_map(|point| match point.value {
                    CurveValue::Float(value) => Some(FloatCurvePoint {
                        time: point.position,
                        value,
                    }),
                    CurveValue::Color(_) => None,
                })
                .collect(),
        }),
        CurveValue::Color(_) => Some(SequenceCurveLibraryPoints::Color {
            points: points
                .iter()
                .filter_map(|point| match point.value {
                    CurveValue::Color(value) => Some(ColorCurvePoint {
                        time: point.position,
                        value: color_hex(value),
                    }),
                    CurveValue::Float(_) => None,
                })
                .collect(),
        }),
    }
}

fn curve_points_param_value(points: SequenceCurveLibraryPoints) -> SequenceEffectParamValue {
    match points {
        SequenceCurveLibraryPoints::Float { points } => {
            SequenceEffectParamValue::FloatCurve { points }
        }
        SequenceCurveLibraryPoints::Color { points } => {
            SequenceEffectParamValue::ColorCurve { points }
        }
    }
}

fn array_param_value(
    session: &ProjectSession,
    values: &[EffectParamValue],
) -> SequenceEffectParamValue {
    let converted = values
        .iter()
        .map(|value| effect_param_value(session, value))
        .collect::<Vec<_>>();
    array_param_from_sequence_values(&converted)
}

fn array_param_from_sequence_values(
    values: &[SequenceEffectParamValue],
) -> SequenceEffectParamValue {
    match values.first() {
        Some(SequenceEffectParamValue::Int { .. }) => SequenceEffectParamValue::IntArray {
            values: values
                .iter()
                .filter_map(|value| match value {
                    SequenceEffectParamValue::Int { value } => Some(*value),
                    _ => None,
                })
                .collect(),
        },
        Some(SequenceEffectParamValue::Bool { .. }) => SequenceEffectParamValue::BoolArray {
            values: values
                .iter()
                .filter_map(|value| match value {
                    SequenceEffectParamValue::Bool { value } => Some(*value),
                    _ => None,
                })
                .collect(),
        },
        Some(SequenceEffectParamValue::Color { .. }) => SequenceEffectParamValue::ColorArray {
            values: values
                .iter()
                .filter_map(|value| match value {
                    SequenceEffectParamValue::Color { value } => Some(value.clone()),
                    _ => None,
                })
                .collect(),
        },
        Some(SequenceEffectParamValue::ColorCurve { .. }) => {
            SequenceEffectParamValue::ColorCurveArray {
                values: values
                    .iter()
                    .filter_map(|value| match value {
                        SequenceEffectParamValue::ColorCurve { points } => Some(points.clone()),
                        _ => None,
                    })
                    .collect(),
            }
        }
        Some(SequenceEffectParamValue::FloatCurve { .. }) => {
            SequenceEffectParamValue::FloatCurveArray {
                values: values
                    .iter()
                    .filter_map(|value| match value {
                        SequenceEffectParamValue::FloatCurve { points } => Some(points.clone()),
                        _ => None,
                    })
                    .collect(),
            }
        }
        _ => SequenceEffectParamValue::FloatArray {
            values: values
                .iter()
                .filter_map(|value| match value {
                    SequenceEffectParamValue::Float { value } => Some(*value),
                    _ => None,
                })
                .collect(),
        },
    }
}

fn geometry(geometry: &DomainGeometry) -> Geometry {
    match geometry {
        DomainGeometry::Points { points } => Geometry::Points {
            points: points.iter().map(|point| point3_meters(*point)).collect(),
        },
        DomainGeometry::Lines { points, pixels } => Geometry::Lines {
            points: points.iter().map(|point| point3_meters(*point)).collect(),
            pixels: *pixels,
        },
        DomainGeometry::Arc {
            center,
            radius,
            start_degrees,
            end_degrees,
            pixels,
        } => Geometry::Arc {
            center: point3_meters(*center),
            radius_meters: distance_span_meters(*radius),
            start_degrees: *start_degrees,
            end_degrees: *end_degrees,
            pixels: *pixels,
        },
    }
}

fn render_plan(geometry: &DomainGeometry, bulb_radius: DistanceSpan) -> GeometryRenderPlan {
    let emitters = emitters(geometry);
    let guides = guides(geometry);
    let bounds = bounds_for_points(emitters.iter().cloned());
    GeometryRenderPlan {
        emitters,
        guides,
        bounds,
        bulb_radius_meters: distance_span_meters(bulb_radius),
    }
}

fn emitters(geometry: &DomainGeometry) -> Vec<GeometryRenderPoint> {
    match geometry {
        DomainGeometry::Points { points } => {
            points.iter().map(|point| render_point(*point)).collect()
        }
        DomainGeometry::Lines { points, pixels } => line_emitters(points, *pixels),
        DomainGeometry::Arc {
            center,
            radius,
            start_degrees,
            end_degrees,
            pixels,
        } => arc_emitters(*center, *radius, *start_degrees, *end_degrees, *pixels),
    }
}

fn guides(geometry: &DomainGeometry) -> Vec<GeometryRenderGuide> {
    match geometry {
        DomainGeometry::Lines { points, .. } => points
            .windows(2)
            .filter_map(|window| {
                let [from, to] = window else {
                    return None;
                };
                Some(GeometryRenderGuide::Line {
                    from: render_point(*from),
                    to: render_point(*to),
                })
            })
            .collect(),
        DomainGeometry::Arc {
            center,
            radius,
            start_degrees,
            end_degrees,
            ..
        } => {
            let radius_meters = distance_span_meters(*radius);
            let start = arc_point(*center, radius_meters, *start_degrees);
            let end = arc_point(*center, radius_meters, *end_degrees);
            vec![GeometryRenderGuide::Arc {
                start,
                end,
                radius_x_meters: radius_meters,
                radius_y_meters: radius_meters,
                rotation: 0.0,
                large_arc: (*end_degrees - *start_degrees).abs() > 180.0,
                sweep_positive: end_degrees >= start_degrees,
            }]
        }
        DomainGeometry::Points { .. } => Vec::new(),
    }
}

fn line_emitters(points: &[Point3], pixels: u32) -> Vec<GeometryRenderPoint> {
    if points.is_empty() || pixels == 0 {
        return Vec::new();
    }
    if points.len() == 1 || pixels == 1 {
        return vec![render_point(points[0])];
    }
    let last = points[points.len() - 1];
    let first = points[0];
    (0..pixels)
        .map(|index| {
            let t = f64::from(index) / f64::from(pixels.saturating_sub(1));
            GeometryRenderPoint {
                x_meters: lerp(distance_meters(first.x), distance_meters(last.x), t),
                y_meters: lerp(distance_meters(first.y), distance_meters(last.y), t),
                z_meters: lerp(distance_meters(first.z), distance_meters(last.z), t),
            }
        })
        .collect()
}

fn arc_emitters(
    center: Point3,
    radius: DistanceSpan,
    start_degrees: f64,
    end_degrees: f64,
    pixels: u32,
) -> Vec<GeometryRenderPoint> {
    if pixels == 0 {
        return Vec::new();
    }
    let radius_meters = distance_span_meters(radius);
    (0..pixels)
        .map(|index| {
            let t = if pixels == 1 {
                0.0
            } else {
                f64::from(index) / f64::from(pixels.saturating_sub(1))
            };
            arc_point(center, radius_meters, lerp(start_degrees, end_degrees, t))
        })
        .collect()
}

fn arc_point(center: Point3, radius_meters: f64, degrees: f64) -> GeometryRenderPoint {
    let radians = degrees.to_radians();
    GeometryRenderPoint {
        x_meters: distance_meters(center.x) + radius_meters * radians.cos(),
        y_meters: distance_meters(center.y) + radius_meters * radians.sin(),
        z_meters: distance_meters(center.z),
    }
}

fn render_point(point: Point3) -> GeometryRenderPoint {
    GeometryRenderPoint {
        x_meters: distance_meters(point.x),
        y_meters: distance_meters(point.y),
        z_meters: distance_meters(point.z),
    }
}

fn point3_meters(point: Point3) -> Point3Meters {
    Point3Meters {
        x_meters: distance_meters(point.x),
        y_meters: distance_meters(point.y),
        z_meters: distance_meters(point.z),
    }
}

fn distance_meters(distance: Distance) -> f64 {
    distance.micrometers as f64 / 1_000_000.0
}

fn distance_span_meters(distance: DistanceSpan) -> f64 {
    distance.micrometers as f64 / 1_000_000.0
}

fn lerp(start: f64, end: f64, t: f64) -> f64 {
    start + (end - start) * t
}

fn bounds_for_points(points: impl Iterator<Item = GeometryRenderPoint>) -> GeometryRenderBounds {
    let mut min_x = 0.0_f64;
    let mut min_y = 0.0_f64;
    let mut max_x = 1.0_f64;
    let mut max_y = 1.0_f64;
    let mut saw = false;
    for point in points {
        if !saw {
            min_x = point.x_meters;
            min_y = point.y_meters;
            max_x = point.x_meters;
            max_y = point.y_meters;
            saw = true;
        } else {
            min_x = min_x.min(point.x_meters);
            min_y = min_y.min(point.y_meters);
            max_x = max_x.max(point.x_meters);
            max_y = max_y.max(point.y_meters);
        }
    }
    if (max_x - min_x).abs() < 1.0 {
        max_x = min_x + 1.0;
    }
    if (max_y - min_y).abs() < 1.0 {
        max_y = min_y + 1.0;
    }
    GeometryRenderBounds {
        min_x_meters: min_x,
        min_y_meters: min_y,
        max_x_meters: max_x,
        max_y_meters: max_y,
    }
}

fn layout_bounds(fixtures: &[LayoutFixturePlacement]) -> GeometryRenderBounds {
    bounds_for_points(fixtures.iter().map(|fixture| GeometryRenderPoint {
        x_meters: fixture.transform.position.x_meters,
        y_meters: fixture.transform.position.y_meters,
        z_meters: fixture.transform.position.z_meters,
    }))
}

fn geometry_summary(geometry: &DomainGeometry) -> String {
    match geometry {
        DomainGeometry::Points { points } => format!("{} points", points.len()),
        DomainGeometry::Lines { pixels, .. } => format!("{pixels} line pixels"),
        DomainGeometry::Arc { pixels, .. } => format!("{pixels} arc pixels"),
    }
}

fn empty_resolved_fixture() -> ResolvedLayoutFixture {
    ResolvedLayoutFixture {
        name: "Missing fixture".to_string(),
        color_model: "rgb".to_string(),
        bulb_diameter_meters: 0.05,
        geometry_summary: "Missing".to_string(),
        render_plan: GeometryRenderPlan {
            emitters: Vec::new(),
            guides: Vec::new(),
            bounds: GeometryRenderBounds {
                min_x_meters: 0.0,
                min_y_meters: 0.0,
                max_x_meters: 1.0,
                max_y_meters: 1.0,
            },
            bulb_radius_meters: 0.025,
        },
        source_path: String::new(),
        object_key: None,
    }
}

fn color_hex(color: Color) -> String {
    format!("#{:02x}{:02x}{:02x}", color.red, color.green, color.blue)
}

fn gui_diagnostic(path: &str, code: &str, message: &str) -> ProjectDiagnostic {
    ProjectDiagnostic {
        path: path.to_string(),
        range: None,
        severity: DiagnosticSeverity::Error,
        code: code.to_string(),
        message: message.to_string(),
    }
}

fn edit_layout(
    session: &mut ProjectSession,
    location: &SourceObjectLocation,
    edit: LayoutGuiEdit,
) -> Result<(), GuiMutationError> {
    let object = source_object_mut(session, location)?;
    match edit {
        LayoutGuiEdit::UpdatePlacementTransform { id, transform } => {
            let fixtures = sequence_field_mut(object, "fixtures")?;
            let fixture = fixtures
                .iter_mut()
                .find(|value| u32_field(value, "id") == Some(id))
                .ok_or_else(|| {
                    GuiMutationError::Invalid("Fixture placement was not found.".to_string())
                })?;
            mapping_mut(fixture)?.insert(string_value("transform"), transform_value(transform)?);
            Ok(())
        }
    }
}

fn edit_fixture(
    session: &mut ProjectSession,
    location: &SourceObjectLocation,
    edit: FixtureGuiEdit,
) -> Result<(), GuiMutationError> {
    match edit {
        FixtureGuiEdit::UpdateBulbDiameter {
            object_key,
            bulb_diameter_meters,
        } => {
            let object = object_by_key_mut(session, &location.document, &object_key)?;
            mapping_mut(object)?.insert(
                string_value("bulb_diameter"),
                yaml_serde::to_value(bulb_diameter_meters)
                    .map_err(|error| GuiMutationError::Invalid(error.to_string()))?,
            );
            Ok(())
        }
        FixtureGuiEdit::MovePoint {
            object_key,
            point_index,
            point,
        } => {
            let object = object_by_key_mut(session, &location.document, &object_key)?;
            let geometry = mapping_field_mut(object, "geometry")?;
            let points = sequence_field_mut(geometry, "points")?;
            let point_value = points.get_mut(point_index as usize).ok_or_else(|| {
                GuiMutationError::Invalid("Fixture point was not found.".to_string())
            })?;
            *point_value = point_value_yaml(point)?;
            Ok(())
        }
    }
}

fn edit_sequence(
    session: &mut ProjectSession,
    resolved: &ResolvedGuiObject,
    edit: SequenceGuiEdit,
) -> Result<(), GuiMutationError> {
    let add_effect_mark_params = match &edit {
        SequenceGuiEdit::AddEffect {
            script,
            mark_collection_key: Some(_),
            ..
        } => mark_param_names(session, &script.effect_name)?,
        _ => Vec::new(),
    };
    let unlink_curve_value = match &edit {
        SequenceGuiEdit::UnlinkEffectCurveParam { id, name } => {
            Some(current_curve_param_value(session, resolved, *id, name)?)
        }
        _ => None,
    };
    let object = source_object_mut(session, &resolved.location)?;
    match edit {
        SequenceGuiEdit::SetAudio { import_path } => {
            mapping_mut(object)?.insert(
                string_value("audio"),
                import_path.map_or(Value::Null, Value::String),
            );
        }
        SequenceGuiEdit::MoveEffect {
            id,
            start_seconds,
            target,
        } => {
            let effect = effect_mut(object, id)?;
            set_seconds_field(effect, "start", start_seconds)?;
            if let Some(target) = target {
                mapping_mut(effect)?.insert(string_value("target"), layout_target_value(target)?);
            }
        }
        SequenceGuiEdit::ResizeEffect {
            id,
            start_seconds,
            duration_seconds,
        } => {
            let effect = effect_mut(object, id)?;
            set_seconds_field(effect, "start", start_seconds)?;
            set_seconds_field(effect, "duration", duration_seconds)?;
        }
        SequenceGuiEdit::SetEffectScope { id, scope } => {
            let effect = effect_mut(object, id)?;
            mapping_mut(effect)?.insert(
                string_value("scope"),
                Value::String(match scope {
                    SequenceEffectScope::PerFixture => "per_fixture".to_string(),
                    SequenceEffectScope::WholeTarget => "whole_target".to_string(),
                }),
            );
        }
        SequenceGuiEdit::RetargetEffect { id, target } => {
            let effect = effect_mut(object, id)?;
            mapping_mut(effect)?.insert(string_value("target"), layout_target_value(target)?);
        }
        SequenceGuiEdit::DeleteEffect { id } => {
            sequence_field_mut(object, "effects")?
                .retain(|value| u32_field(value, "id") != Some(id));
        }
        SequenceGuiEdit::MoveMark {
            collection_key,
            index,
            time_seconds,
        } => {
            let collection = mark_collection_mut(object, &collection_key)?;
            let marks = sequence_field_mut(collection, "marks")?;
            let mark = marks
                .get_mut(index as usize)
                .ok_or_else(|| GuiMutationError::Invalid("Mark was not found.".to_string()))?;
            *mark = seconds_value(time_seconds);
            sort_duration_strings(marks);
        }
        SequenceGuiEdit::AddMark {
            collection_key,
            time_seconds,
        } => {
            let collection = mark_collection_mut(object, &collection_key)?;
            let marks = sequence_field_mut(collection, "marks")?;
            marks.push(seconds_value(time_seconds));
            sort_duration_strings(marks);
        }
        SequenceGuiEdit::DeleteMark {
            collection_key,
            index,
        } => {
            let collection = mark_collection_mut(object, &collection_key)?;
            let marks = sequence_field_mut(collection, "marks")?;
            if (index as usize) < marks.len() {
                marks.remove(index as usize);
            }
        }
        SequenceGuiEdit::CreateMarkCollection { key, name, color } => {
            let collections = ensure_sequence_field_mut(object, "mark_collections")?;
            let mut collection = Mapping::new();
            collection.insert(string_value("key"), Value::String(key));
            collection.insert(string_value("name"), Value::String(name));
            collection.insert(string_value("color"), Value::String(color));
            collection.insert(string_value("marks"), Value::Sequence(Vec::new()));
            collections.push(Value::Mapping(collection));
        }
        SequenceGuiEdit::RenameMarkCollection { key, name } => {
            let collection = mark_collection_mut(object, &key)?;
            mapping_mut(collection)?.insert(string_value("name"), Value::String(name));
        }
        SequenceGuiEdit::DeleteMarkCollection { key } => {
            sequence_field_mut(object, "mark_collections")?
                .retain(|value| string_field(value, "key").as_deref() != Some(key.as_str()));
        }
        SequenceGuiEdit::SetMarkCollectionColor { key, color } => {
            let collection = mark_collection_mut(object, &key)?;
            mapping_mut(collection)?.insert(string_value("color"), Value::String(color));
        }
        SequenceGuiEdit::UpdateEffectParam { id, name, value } => {
            let effect = effect_mut(object, id)?;
            upsert_param(effect, &name, value)?;
        }
        SequenceGuiEdit::AddEffect {
            script,
            target,
            scope,
            start_seconds,
            mark_collection_key,
        } => {
            let effects = ensure_sequence_field_mut(object, "effects")?;
            let next_id = effects
                .iter()
                .filter_map(|value| u32_field(value, "id"))
                .max()
                .unwrap_or(0)
                + 1;
            let mut effect = Mapping::new();
            effect.insert(
                string_value("id"),
                yaml_serde::to_value(next_id)
                    .map_err(|error| GuiMutationError::Invalid(error.to_string()))?,
            );
            effect.insert(string_value("start"), seconds_value(start_seconds));
            effect.insert(string_value("duration"), seconds_value(1.0));
            effect.insert(string_value("target"), layout_target_value(target)?);
            effect.insert(
                string_value("scope"),
                Value::String(match scope {
                    SequenceEffectScope::PerFixture => "per_fixture".to_string(),
                    SequenceEffectScope::WholeTarget => "whole_target".to_string(),
                }),
            );
            effect.insert(string_value("script"), Value::String(script.effect_name));
            if let Some(key) = mark_collection_key {
                if !add_effect_mark_params.is_empty() {
                    let mut params = Mapping::new();
                    for name in add_effect_mark_params {
                        params.insert(
                            string_value(&name),
                            param_value(SequenceEffectParamValue::Marks { key: key.clone() })?,
                        );
                    }
                    effect.insert(string_value("params"), Value::Mapping(params));
                }
            }
            effects.push(Value::Mapping(effect));
        }
        SequenceGuiEdit::ChangeEffectScript { id, script } => {
            let effect = effect_mut(object, id)?;
            mapping_mut(effect)?.insert(string_value("script"), Value::String(script.effect_name));
        }
        SequenceGuiEdit::LinkEffectCurveParam {
            id,
            name,
            curve_path: _,
            object_key,
        } => {
            let effect = effect_mut(object, id)?;
            upsert_param_raw(effect, &name, curve_reference_param_value(object_key))?;
        }
        SequenceGuiEdit::UnlinkEffectCurveParam { id, name } => {
            let effect = effect_mut(object, id)?;
            let value = unlink_curve_value.ok_or_else(|| {
                GuiMutationError::Invalid("Curve param could not be resolved.".to_string())
            })?;
            upsert_param(effect, &name, value)?;
        }
    }
    Ok(())
}

fn current_curve_param_value(
    session: &ProjectSession,
    resolved: &ResolvedGuiObject,
    effect_id: u32,
    name: &str,
) -> Result<SequenceEffectParamValue, GuiMutationError> {
    let sequence_id = SequenceId(resolved.source_id.id.clone());
    let sequence = session
        .project
        .sequences
        .get(&sequence_id)
        .ok_or_else(|| GuiMutationError::Invalid("Sequence was not found.".to_string()))?;
    let effect = sequence
        .effects
        .iter()
        .find(|effect| effect.id.0 == effect_id)
        .ok_or_else(|| GuiMutationError::Invalid("Effect was not found.".to_string()))?;

    if let Some(value) = effect
        .param_overrides
        .iter()
        .find_map(|(key, value)| (key.as_str() == name).then_some(value))
    {
        return match value {
            EffectParamValue::Curve(_) => Ok(effect_param_value(session, value)),
            _ => Err(GuiMutationError::Invalid(
                "Param is not a curve param.".to_string(),
            )),
        };
    }

    let definition = session
        .project
        .definitions
        .effects
        .get(&effect.definition)
        .ok_or_else(|| GuiMutationError::Invalid("Effect definition was not found.".to_string()))?;
    let param = definition
        .compiled
        .params()
        .iter()
        .find(|param| param.name.as_str() == name)
        .ok_or_else(|| GuiMutationError::Invalid("Effect param was not found.".to_string()))?;
    match &param.ty {
        Type::Curve(inner) => Ok(param
            .default
            .as_ref()
            .and_then(default_param_value)
            .unwrap_or_else(|| match inner.as_ref() {
                Type::Color => SequenceEffectParamValue::ColorCurve { points: Vec::new() },
                _ => SequenceEffectParamValue::FloatCurve { points: Vec::new() },
            })),
        _ => Err(GuiMutationError::Invalid(
            "Param is not a curve param.".to_string(),
        )),
    }
}

fn copy_sequence_selection(
    session: &ProjectSession,
    location: &SourceObjectLocation,
    sequence_id: &SequenceId,
    selection: &SequenceSelection,
) -> Result<(Option<SequenceClipboard>, u32, u32), GuiMutationError> {
    match selection {
        SequenceSelection::Effects { ids } => {
            let object = source_object(session, location)?;
            let effects = sequence_field(object, "effects")?;
            let sequence =
                session.project.sequences.get(sequence_id).ok_or_else(|| {
                    GuiMutationError::Invalid("Sequence was not found.".to_string())
                })?;
            let mut copied = Vec::new();
            let mut skipped = 0u32;
            for id in ids {
                let Some(effect) = sequence.effects.iter().find(|effect| effect.id.0 == *id) else {
                    skipped = skipped.saturating_add(1);
                    continue;
                };
                let Some(value) = effects
                    .iter()
                    .find(|value| u32_field(value, "id") == Some(*id))
                else {
                    skipped = skipped.saturating_add(1);
                    continue;
                };
                copied.push(ClipboardEffect {
                    value: value.clone(),
                    start_seconds: effect.start.as_seconds_f64(),
                    lane_index: effect_lane_index(session, &effect.target),
                });
            }
            let copied_count = copied.len() as u32;
            Ok((
                (!copied.is_empty()).then_some(SequenceClipboard::Effects(copied)),
                copied_count,
                skipped,
            ))
        }
        SequenceSelection::Marks { marks } => {
            let object = source_object(session, location)?;
            let mut copied = Vec::new();
            let mut skipped = 0u32;
            for mark in marks {
                let Some(time_seconds) = mark_time_seconds(object, mark)? else {
                    skipped = skipped.saturating_add(1);
                    continue;
                };
                copied.push(ClipboardMark {
                    collection_key: mark.collection_key.clone(),
                    time_seconds,
                });
            }
            let copied_count = copied.len() as u32;
            Ok((
                (!copied.is_empty()).then_some(SequenceClipboard::Marks(copied)),
                copied_count,
                skipped,
            ))
        }
    }
}

fn delete_sequence_selection(
    session: &mut ProjectSession,
    location: &SourceObjectLocation,
    selection: &SequenceSelection,
) -> Result<(), GuiMutationError> {
    let object = source_object_mut(session, location)?;
    match selection {
        SequenceSelection::Effects { ids } => {
            sequence_field_mut(object, "effects")?
                .retain(|value| !ids.iter().any(|id| u32_field(value, "id") == Some(*id)));
        }
        SequenceSelection::Marks { marks } => {
            for (collection_key, indexes) in mark_indexes_by_collection(marks) {
                let collection = mark_collection_mut(object, &collection_key)?;
                let values = sequence_field_mut(collection, "marks")?;
                for index in indexes.into_iter().rev() {
                    if index < values.len() {
                        values.remove(index);
                    }
                }
            }
        }
    }
    Ok(())
}

fn paste_sequence_clipboard(
    session: &mut ProjectSession,
    location: &SourceObjectLocation,
    anchor: SequencePasteAnchor,
    clipboard: Option<&SequenceClipboard>,
) -> Result<SequenceSelectionMutation, GuiMutationError> {
    let Some(clipboard) = clipboard else {
        return Ok(SequenceSelectionMutation {
            selection: None,
            copied_count: 0,
            skipped_count: 0,
        });
    };
    let lane_count = sequence_lane_count(session);
    let lane_targets = (0..lane_count)
        .map(|lane| target_for_lane(session, lane))
        .collect::<Vec<_>>();
    let object = source_object_mut(session, location)?;
    match clipboard {
        SequenceClipboard::Effects(effects) => {
            let min_start = effects
                .iter()
                .map(|effect| effect.start_seconds)
                .fold(f64::INFINITY, f64::min);
            let min_lane = effects
                .iter()
                .map(|effect| effect.lane_index)
                .min()
                .unwrap_or_default();
            let mut next_id = sequence_field_mut(object, "effects")?
                .iter()
                .filter_map(|value| u32_field(value, "id"))
                .max()
                .unwrap_or(0)
                .saturating_add(1);
            let mut pasted_ids = Vec::with_capacity(effects.len());
            for effect in effects {
                let mut value = effect.value.clone();
                let target_lane = anchored_lane(
                    anchor.lane_index as usize,
                    effect.lane_index,
                    min_lane,
                    lane_count,
                );
                mapping_mut(&mut value)?.insert(
                    string_value("id"),
                    yaml_serde::to_value(next_id)
                        .map_err(|error| GuiMutationError::Invalid(error.to_string()))?,
                );
                set_seconds_field(
                    &mut value,
                    "start",
                    (anchor.time_seconds + effect.start_seconds - min_start).max(0.0),
                )?;
                if let Some(Some(target)) = lane_targets.get(target_lane) {
                    mapping_mut(&mut value)?
                        .insert(string_value("target"), effect_target_value(target.clone())?);
                }
                sequence_field_mut(object, "effects")?.push(value);
                pasted_ids.push(next_id);
                next_id = next_id.saturating_add(1);
            }
            Ok(SequenceSelectionMutation {
                selection: Some(SequenceSelection::Effects { ids: pasted_ids }),
                copied_count: effects.len() as u32,
                skipped_count: 0,
            })
        }
        SequenceClipboard::Marks(marks) => {
            let min_time = marks
                .iter()
                .map(|mark| mark.time_seconds)
                .fold(f64::INFINITY, f64::min);
            let mut pasted = Vec::new();
            let mut skipped = 0u32;
            for mark in marks {
                let collection = match mark_collection_mut(object, &mark.collection_key) {
                    Ok(collection) => collection,
                    Err(_) => {
                        skipped = skipped.saturating_add(1);
                        continue;
                    }
                };
                let values = sequence_field_mut(collection, "marks")?;
                let time_seconds = (anchor.time_seconds + mark.time_seconds - min_time).max(0.0);
                values.push(seconds_value(time_seconds));
                sort_duration_strings(values);
                let index = values
                    .iter()
                    .position(|value| (duration_seconds(value) - time_seconds).abs() < f64::EPSILON)
                    .unwrap_or_else(|| values.len().saturating_sub(1));
                pasted.push(SequenceMarkRef {
                    collection_key: mark.collection_key.clone(),
                    index: index as u32,
                });
            }
            Ok(SequenceSelectionMutation {
                selection: Some(SequenceSelection::Marks { marks: pasted }),
                copied_count: marks.len() as u32,
                skipped_count: skipped,
            })
        }
    }
}

fn move_effect_selection(
    session: &mut ProjectSession,
    location: &SourceObjectLocation,
    sequence_id: &SequenceId,
    ids: &[u32],
    time_delta_seconds: f64,
    lane_delta: i32,
) -> Result<Vec<u32>, GuiMutationError> {
    let effect_updates = effect_selection_updates(session, sequence_id, ids, |session, effect| {
        let lane = shifted_lane(
            effect_lane_index(session, &effect.target),
            lane_delta,
            sequence_lane_count(session),
        );
        (
            effect.start.as_seconds_f64() + time_delta_seconds,
            effect.duration.as_seconds_f64(),
            lane,
        )
    })?;
    apply_effect_updates(session, location, effect_updates)
}

fn resize_effect_selection(
    session: &mut ProjectSession,
    location: &SourceObjectLocation,
    sequence_id: &SequenceId,
    ids: &[u32],
    edge: SequenceResizeEdge,
    time_delta_seconds: f64,
) -> Result<(), GuiMutationError> {
    let effect_updates = effect_selection_updates(session, sequence_id, ids, |session, effect| {
        let start_seconds = effect.start.as_seconds_f64();
        let duration_seconds = effect.duration.as_seconds_f64();
        let lane = effect_lane_index(session, &effect.target);
        match edge {
            SequenceResizeEdge::Left => (
                start_seconds + time_delta_seconds,
                duration_seconds - time_delta_seconds,
                lane,
            ),
            SequenceResizeEdge::Right => {
                (start_seconds, duration_seconds + time_delta_seconds, lane)
            }
        }
    })?;
    apply_effect_updates(session, location, effect_updates)?;
    Ok(())
}

fn move_mark_selection(
    session: &mut ProjectSession,
    location: &SourceObjectLocation,
    marks: &[SequenceMarkRef],
    time_delta_seconds: f64,
) -> Result<Vec<SequenceMarkRef>, GuiMutationError> {
    let object = source_object_mut(session, location)?;
    let mut moved = Vec::new();
    for (collection_key, indexes) in mark_indexes_by_collection(marks) {
        let collection = mark_collection_mut(object, &collection_key)?;
        let values = sequence_field_mut(collection, "marks")?;
        let mut moved_times = Vec::new();
        for index in indexes {
            if let Some(value) = values.get_mut(index) {
                let time_seconds = (duration_seconds(value) + time_delta_seconds).max(0.0);
                *value = seconds_value(time_seconds);
                moved_times.push(time_seconds);
            }
        }
        sort_duration_strings(values);
        for time_seconds in moved_times {
            if let Some(index) = values
                .iter()
                .position(|value| (duration_seconds(value) - time_seconds).abs() < f64::EPSILON)
            {
                moved.push(SequenceMarkRef {
                    collection_key: collection_key.clone(),
                    index: index as u32,
                });
            }
        }
    }
    Ok(moved)
}

struct EffectUpdate {
    id: u32,
    start_seconds: f64,
    duration_seconds: f64,
    lane_index: usize,
}

fn effect_selection_updates(
    session: &ProjectSession,
    sequence_id: &SequenceId,
    ids: &[u32],
    update: impl Fn(&ProjectSession, &dawn_language::effect::EffectInst) -> (f64, f64, usize),
) -> Result<Vec<EffectUpdate>, GuiMutationError> {
    let sequence = session
        .project
        .sequences
        .get(sequence_id)
        .ok_or_else(|| GuiMutationError::Invalid("Sequence was not found.".to_string()))?;
    Ok(sequence
        .effects
        .iter()
        .filter(|effect| ids.contains(&effect.id.0))
        .map(|effect| {
            let (start_seconds, duration_seconds, lane_index) = update(session, effect);
            EffectUpdate {
                id: effect.id.0,
                start_seconds,
                duration_seconds,
                lane_index,
            }
        })
        .collect())
}

fn apply_effect_updates(
    session: &mut ProjectSession,
    location: &SourceObjectLocation,
    updates: Vec<EffectUpdate>,
) -> Result<Vec<u32>, GuiMutationError> {
    let targets = updates
        .iter()
        .map(|update| (update.id, target_for_lane(session, update.lane_index)))
        .collect::<Vec<_>>();
    let object = source_object_mut(session, location)?;
    let mut moved = Vec::new();
    for update in updates {
        let effect = effect_mut(object, update.id)?;
        set_seconds_field(effect, "start", update.start_seconds.max(0.0))?;
        set_seconds_field(effect, "duration", update.duration_seconds.max(0.000000001))?;
        if let Some((_, Some(target))) = targets.iter().find(|(id, _)| *id == update.id) {
            mapping_mut(effect)?
                .insert(string_value("target"), effect_target_value(target.clone())?);
        }
        moved.push(update.id);
    }
    Ok(moved)
}

fn source_object<'a>(
    session: &'a ProjectSession,
    location: &SourceObjectLocation,
) -> Result<&'a Value, GuiMutationError> {
    let document = session
        .source
        .documents
        .get(&location.document)
        .ok_or_else(|| GuiMutationError::Invalid("Source document was not found.".to_string()))?;
    let SourceDocumentKind::Dawn { value, .. } = &document.kind else {
        return Err(GuiMutationError::Invalid(
            "GUI edits can only read Dawn YAML documents.".to_string(),
        ));
    };
    mapping(value)?
        .get(string_value(&location.object_key))
        .ok_or_else(|| GuiMutationError::Invalid("Source object was not found.".to_string()))
}

fn mapping(value: &Value) -> Result<&Mapping, GuiMutationError> {
    match value {
        Value::Mapping(mapping) => Ok(mapping),
        _ => Err(GuiMutationError::Invalid(
            "Expected a YAML mapping.".to_string(),
        )),
    }
}

fn sequence_field<'a>(value: &'a Value, field: &str) -> Result<&'a Vec<Value>, GuiMutationError> {
    match mapping(value)?.get(string_value(field)) {
        Some(Value::Sequence(sequence)) => Ok(sequence),
        _ => Err(GuiMutationError::Invalid(format!(
            "`{field}` must be a sequence."
        ))),
    }
}

fn mark_time_seconds(
    sequence_object: &Value,
    mark: &SequenceMarkRef,
) -> Result<Option<f64>, GuiMutationError> {
    let collections = sequence_field(sequence_object, "mark_collections")?;
    let Some(collection) = collections.iter().find(|collection| {
        string_field(collection, "key").as_deref() == Some(&mark.collection_key)
    }) else {
        return Ok(None);
    };
    Ok(sequence_field(collection, "marks")?
        .get(mark.index as usize)
        .map(duration_seconds))
}

fn mark_indexes_by_collection(marks: &[SequenceMarkRef]) -> BTreeMap<String, Vec<usize>> {
    let mut grouped = BTreeMap::<String, Vec<usize>>::new();
    for mark in marks {
        grouped
            .entry(mark.collection_key.clone())
            .or_default()
            .push(mark.index as usize);
    }
    for indexes in grouped.values_mut() {
        indexes.sort_unstable();
        indexes.dedup();
    }
    grouped
}

fn effect_lane_index(session: &ProjectSession, target: &EffectTarget) -> usize {
    let Some(layout_id) = active_layout_id(session) else {
        return 0;
    };
    let Some(layout) = session.project.layouts.get(&layout_id) else {
        return 0;
    };
    layout
        .target_order
        .iter()
        .position(|candidate| effect_target_matches_layout(target, candidate))
        .unwrap_or_default()
}

fn effect_target_matches_layout(target: &EffectTarget, candidate: &DomainLayoutTarget) -> bool {
    matches!(
        (target, candidate),
        (EffectTarget::Fixture(left), DomainLayoutTarget::Fixture(right)) if left == right
    ) || matches!(
        (target, candidate),
        (EffectTarget::Group(left), DomainLayoutTarget::Group(right)) if left == right
    )
}

fn sequence_lane_count(session: &ProjectSession) -> usize {
    active_layout_id(session)
        .and_then(|layout_id| session.project.layouts.get(&layout_id))
        .map(|layout| layout.target_order.len())
        .unwrap_or_default()
}

fn target_for_lane(session: &ProjectSession, lane_index: usize) -> Option<EffectTarget> {
    let layout_id = active_layout_id(session)?;
    let layout = session.project.layouts.get(&layout_id)?;
    layout
        .target_order
        .get(lane_index)
        .map(effect_target_from_layout)
}

fn effect_target_from_layout(target: &DomainLayoutTarget) -> EffectTarget {
    match target {
        DomainLayoutTarget::Fixture(id) => EffectTarget::Fixture(FixtureInstanceId(id.0)),
        DomainLayoutTarget::Group(id) => EffectTarget::Group(FixtureGroupId(id.0)),
    }
}

fn effect_target_value(target: EffectTarget) -> Result<Value, GuiMutationError> {
    match target {
        EffectTarget::Fixture(id) => layout_target_value(LayoutTarget {
            kind: LayoutTargetKind::Fixture,
            name: id.0.to_string(),
        }),
        EffectTarget::Group(id) => layout_target_value(LayoutTarget {
            kind: LayoutTargetKind::Group,
            name: id.0.to_string(),
        }),
    }
}

fn shifted_lane(lane_index: usize, lane_delta: i32, lane_count: usize) -> usize {
    if lane_count == 0 {
        return 0;
    }
    (lane_index as i32 + lane_delta).clamp(0, lane_count.saturating_sub(1) as i32) as usize
}

fn anchored_lane(
    anchor_lane: usize,
    lane_index: usize,
    min_lane: usize,
    lane_count: usize,
) -> usize {
    if lane_count == 0 {
        return lane_index;
    }
    (anchor_lane + lane_index.saturating_sub(min_lane)).min(lane_count.saturating_sub(1))
}

fn mark_param_names(
    session: &ProjectSession,
    effect_name: &str,
) -> Result<Vec<String>, GuiMutationError> {
    let id = EffectDefinitionId(effect_name.to_string());
    let definition = session
        .project
        .definitions
        .effects
        .get(&id)
        .ok_or_else(|| GuiMutationError::Invalid("Effect script was not found.".to_string()))?;
    Ok(definition
        .compiled
        .params()
        .iter()
        .filter(|param| matches!(param.ty, Type::Marks))
        .map(|param| param.name.as_str().to_string())
        .collect())
}

fn source_object_mut<'a>(
    session: &'a mut ProjectSession,
    location: &SourceObjectLocation,
) -> Result<&'a mut Value, GuiMutationError> {
    object_by_key_mut(session, &location.document, &location.object_key)
}

fn object_by_key_mut<'a>(
    session: &'a mut ProjectSession,
    document_path: &Utf8Path,
    object_key: &str,
) -> Result<&'a mut Value, GuiMutationError> {
    let document = session
        .source
        .documents
        .get_mut(document_path)
        .ok_or_else(|| GuiMutationError::Invalid("Source document was not found.".to_string()))?;
    let SourceDocumentKind::Dawn { value, .. } = &mut document.kind else {
        return Err(GuiMutationError::Invalid(
            "GUI edits can only modify Dawn YAML documents.".to_string(),
        ));
    };
    mapping_mut(value)?
        .get_mut(string_value(object_key))
        .ok_or_else(|| GuiMutationError::Invalid("Source object was not found.".to_string()))
}

fn mapping_mut(value: &mut Value) -> Result<&mut Mapping, GuiMutationError> {
    match value {
        Value::Mapping(mapping) => Ok(mapping),
        _ => Err(GuiMutationError::Invalid(
            "Expected a YAML mapping.".to_string(),
        )),
    }
}

fn mapping_field_mut<'a>(
    value: &'a mut Value,
    field: &str,
) -> Result<&'a mut Value, GuiMutationError> {
    mapping_mut(value)?
        .get_mut(string_value(field))
        .ok_or_else(|| GuiMutationError::Invalid(format!("Missing `{field}` field.")))
}

fn sequence_field_mut<'a>(
    value: &'a mut Value,
    field: &str,
) -> Result<&'a mut Vec<Value>, GuiMutationError> {
    match mapping_field_mut(value, field)? {
        Value::Sequence(sequence) => Ok(sequence),
        _ => Err(GuiMutationError::Invalid(format!(
            "`{field}` must be a sequence."
        ))),
    }
}

fn ensure_sequence_field_mut<'a>(
    value: &'a mut Value,
    field: &str,
) -> Result<&'a mut Vec<Value>, GuiMutationError> {
    let mapping = mapping_mut(value)?;
    if !mapping.contains_key(string_value(field)) {
        mapping.insert(string_value(field), Value::Sequence(Vec::new()));
    }
    match mapping.get_mut(string_value(field)) {
        Some(Value::Sequence(sequence)) => Ok(sequence),
        _ => Err(GuiMutationError::Invalid(format!(
            "`{field}` must be a sequence."
        ))),
    }
}

fn ensure_mapping_field_mut<'a>(
    value: &'a mut Value,
    field: &str,
) -> Result<&'a mut Mapping, GuiMutationError> {
    let mapping = mapping_mut(value)?;
    if !mapping.contains_key(string_value(field)) {
        mapping.insert(string_value(field), Value::Mapping(Mapping::new()));
    }
    match mapping.get_mut(string_value(field)) {
        Some(Value::Mapping(mapping)) => Ok(mapping),
        _ => Err(GuiMutationError::Invalid(format!(
            "`{field}` must be a mapping."
        ))),
    }
}

fn effect_mut(value: &mut Value, id: u32) -> Result<&mut Value, GuiMutationError> {
    sequence_field_mut(value, "effects")?
        .iter_mut()
        .find(|effect| u32_field(effect, "id") == Some(id))
        .ok_or_else(|| GuiMutationError::Invalid("Effect was not found.".to_string()))
}

fn mark_collection_mut<'a>(
    value: &'a mut Value,
    key: &str,
) -> Result<&'a mut Value, GuiMutationError> {
    sequence_field_mut(value, "mark_collections")?
        .iter_mut()
        .find(|collection| string_field(collection, "key").as_deref() == Some(key))
        .ok_or_else(|| GuiMutationError::Invalid("Mark collection was not found.".to_string()))
}

fn set_seconds_field(value: &mut Value, field: &str, seconds: f64) -> Result<(), GuiMutationError> {
    mapping_mut(value)?.insert(string_value(field), seconds_value(seconds));
    Ok(())
}

fn upsert_param(
    effect: &mut Value,
    name: &str,
    value: SequenceEffectParamValue,
) -> Result<(), GuiMutationError> {
    upsert_param_raw(effect, name, param_value(value)?)
}

fn upsert_param_raw(effect: &mut Value, name: &str, value: Value) -> Result<(), GuiMutationError> {
    ensure_mapping_field_mut(effect, "params")?.insert(string_value(name), value);
    Ok(())
}

fn param_value(value: SequenceEffectParamValue) -> Result<Value, GuiMutationError> {
    let mut mapping = Mapping::new();
    match value {
        SequenceEffectParamValue::Int { value } => {
            mapping.insert(string_value("type"), Value::String("integer".to_string()));
            mapping.insert(
                string_value("value"),
                yaml_serde::to_value(value as i64)
                    .map_err(|error| GuiMutationError::Invalid(error.to_string()))?,
            );
        }
        SequenceEffectParamValue::Float { value } => {
            mapping.insert(string_value("type"), Value::String("float".to_string()));
            mapping.insert(
                string_value("value"),
                yaml_serde::to_value(value)
                    .map_err(|error| GuiMutationError::Invalid(error.to_string()))?,
            );
        }
        SequenceEffectParamValue::Bool { value } => {
            mapping.insert(string_value("type"), Value::String("bool".to_string()));
            mapping.insert(
                string_value("value"),
                yaml_serde::to_value(value)
                    .map_err(|error| GuiMutationError::Invalid(error.to_string()))?,
            );
        }
        SequenceEffectParamValue::Color { value } => {
            mapping.insert(string_value("type"), Value::String("color".to_string()));
            mapping.insert(string_value("value"), Value::String(value));
        }
        SequenceEffectParamValue::Enum { value } => {
            mapping.insert(string_value("type"), Value::String("enum".to_string()));
            mapping.insert(string_value("value"), Value::String(value));
        }
        SequenceEffectParamValue::Marks { key } => {
            mapping.insert(string_value("type"), Value::String("marks".to_string()));
            mapping.insert(string_value("key"), Value::String(key));
        }
        SequenceEffectParamValue::FloatCurve { points } => {
            mapping.insert(string_value("type"), Value::String("curve".to_string()));
            let points = points
                .into_iter()
                .map(|point| {
                    yaml_serde::to_value(point.value)
                        .map(|value| (point.time, value))
                        .map_err(|error| GuiMutationError::Invalid(error.to_string()))
                })
                .collect::<Result<Vec<_>, _>>()?;
            mapping.insert(string_value("curve"), curve_value("float", points));
        }
        SequenceEffectParamValue::ColorCurve { points } => {
            mapping.insert(string_value("type"), Value::String("curve".to_string()));
            mapping.insert(
                string_value("curve"),
                curve_value(
                    "color",
                    points
                        .into_iter()
                        .map(|point| Ok((point.time, Value::String(point.value))))
                        .collect::<Result<Vec<_>, GuiMutationError>>()?,
                ),
            );
        }
        SequenceEffectParamValue::IntArray { values } => {
            mapping.insert(string_value("type"), Value::String("array".to_string()));
            mapping.insert(
                string_value("element_type"),
                Value::String("integer".to_string()),
            );
            mapping.insert(
                string_value("values"),
                Value::Sequence(
                    values
                        .into_iter()
                        .map(|value| param_value(SequenceEffectParamValue::Int { value }))
                        .collect::<Result<Vec<_>, _>>()?,
                ),
            );
        }
        SequenceEffectParamValue::FloatArray { values } => {
            mapping.insert(string_value("type"), Value::String("array".to_string()));
            mapping.insert(
                string_value("element_type"),
                Value::String("float".to_string()),
            );
            mapping.insert(
                string_value("values"),
                Value::Sequence(
                    values
                        .into_iter()
                        .map(|value| param_value(SequenceEffectParamValue::Float { value }))
                        .collect::<Result<Vec<_>, _>>()?,
                ),
            );
        }
        SequenceEffectParamValue::BoolArray { values } => {
            mapping.insert(string_value("type"), Value::String("array".to_string()));
            mapping.insert(
                string_value("element_type"),
                Value::String("bool".to_string()),
            );
            mapping.insert(
                string_value("values"),
                Value::Sequence(
                    values
                        .into_iter()
                        .map(|value| param_value(SequenceEffectParamValue::Bool { value }))
                        .collect::<Result<Vec<_>, _>>()?,
                ),
            );
        }
        SequenceEffectParamValue::ColorArray { values } => {
            mapping.insert(string_value("type"), Value::String("array".to_string()));
            mapping.insert(
                string_value("element_type"),
                Value::String("color".to_string()),
            );
            mapping.insert(
                string_value("values"),
                Value::Sequence(
                    values
                        .into_iter()
                        .map(|value| param_value(SequenceEffectParamValue::Color { value }))
                        .collect::<Result<Vec<_>, _>>()?,
                ),
            );
        }
        SequenceEffectParamValue::FloatCurveArray { values } => {
            mapping.insert(string_value("type"), Value::String("array".to_string()));
            mapping.insert(
                string_value("element_type"),
                Value::String("curve_float".to_string()),
            );
            mapping.insert(
                string_value("values"),
                Value::Sequence(
                    values
                        .into_iter()
                        .map(|points| array_curve_item_value("float", float_curve_points(points)))
                        .collect::<Result<Vec<_>, _>>()?,
                ),
            );
        }
        SequenceEffectParamValue::ColorCurveArray { values } => {
            mapping.insert(string_value("type"), Value::String("array".to_string()));
            mapping.insert(
                string_value("element_type"),
                Value::String("curve_color".to_string()),
            );
            mapping.insert(
                string_value("values"),
                Value::Sequence(
                    values
                        .into_iter()
                        .map(|points| array_curve_item_value("color", color_curve_points(points)))
                        .collect::<Result<Vec<_>, _>>()?,
                ),
            );
        }
    }
    Ok(Value::Mapping(mapping))
}

fn float_curve_points(points: Vec<FloatCurvePoint>) -> Result<Vec<(f64, Value)>, GuiMutationError> {
    points
        .into_iter()
        .map(|point| {
            yaml_serde::to_value(point.value)
                .map(|value| (point.time, value))
                .map_err(|error| GuiMutationError::Invalid(error.to_string()))
        })
        .collect()
}

fn color_curve_points(points: Vec<ColorCurvePoint>) -> Result<Vec<(f64, Value)>, GuiMutationError> {
    points
        .into_iter()
        .map(|point| Ok((point.time, Value::String(point.value))))
        .collect()
}

fn array_curve_item_value(
    value_type: &str,
    points: Result<Vec<(f64, Value)>, GuiMutationError>,
) -> Result<Value, GuiMutationError> {
    let mut mapping = Mapping::new();
    mapping.insert(string_value("curve"), curve_value(value_type, points?));
    Ok(Value::Mapping(mapping))
}

fn curve_reference_param_value(reference: String) -> Value {
    let mut mapping = Mapping::new();
    mapping.insert(string_value("type"), Value::String("curve".to_string()));
    mapping.insert(string_value("curve"), Value::String(reference));
    Value::Mapping(mapping)
}

fn curve_value(value_type: &str, points: Vec<(f64, Value)>) -> Value {
    let mut mapping = Mapping::new();
    mapping.insert(
        string_value("value_type"),
        Value::String(value_type.to_string()),
    );
    mapping.insert(
        string_value("points"),
        Value::Sequence(
            points
                .into_iter()
                .map(|(time, value)| {
                    let mut point = Mapping::new();
                    point.insert(
                        string_value("time"),
                        yaml_serde::to_value(time).unwrap_or(Value::Null),
                    );
                    point.insert(string_value("value"), value);
                    Value::Mapping(point)
                })
                .collect(),
        ),
    );
    Value::Mapping(mapping)
}

fn transform_value(transform: Transform) -> Result<Value, GuiMutationError> {
    let mut mapping = Mapping::new();
    mapping.insert(
        string_value("position"),
        point_value_yaml(transform.position)?,
    );
    let mut rotation = Mapping::new();
    rotation.insert(
        string_value("x"),
        yaml_serde::to_value(transform.rotation.x_degrees)
            .map_err(|error| GuiMutationError::Invalid(error.to_string()))?,
    );
    rotation.insert(
        string_value("y"),
        yaml_serde::to_value(transform.rotation.y_degrees)
            .map_err(|error| GuiMutationError::Invalid(error.to_string()))?,
    );
    rotation.insert(
        string_value("z"),
        yaml_serde::to_value(transform.rotation.z_degrees)
            .map_err(|error| GuiMutationError::Invalid(error.to_string()))?,
    );
    mapping.insert(string_value("rotation"), Value::Mapping(rotation));
    let mut scale = Mapping::new();
    scale.insert(
        string_value("x"),
        yaml_serde::to_value(transform.scale.x)
            .map_err(|error| GuiMutationError::Invalid(error.to_string()))?,
    );
    scale.insert(
        string_value("y"),
        yaml_serde::to_value(transform.scale.y)
            .map_err(|error| GuiMutationError::Invalid(error.to_string()))?,
    );
    scale.insert(
        string_value("z"),
        yaml_serde::to_value(transform.scale.z)
            .map_err(|error| GuiMutationError::Invalid(error.to_string()))?,
    );
    mapping.insert(string_value("scale"), Value::Mapping(scale));
    Ok(Value::Mapping(mapping))
}

fn point_value_yaml(point: Point3Meters) -> Result<Value, GuiMutationError> {
    let mut mapping = Mapping::new();
    mapping.insert(
        string_value("x"),
        yaml_serde::to_value(point.x_meters)
            .map_err(|error| GuiMutationError::Invalid(error.to_string()))?,
    );
    mapping.insert(
        string_value("y"),
        yaml_serde::to_value(point.y_meters)
            .map_err(|error| GuiMutationError::Invalid(error.to_string()))?,
    );
    mapping.insert(
        string_value("z"),
        yaml_serde::to_value(point.z_meters)
            .map_err(|error| GuiMutationError::Invalid(error.to_string()))?,
    );
    Ok(Value::Mapping(mapping))
}

fn layout_target_value(target: LayoutTarget) -> Result<Value, GuiMutationError> {
    let mut mapping = Mapping::new();
    mapping.insert(
        string_value("type"),
        Value::String(match target.kind {
            LayoutTargetKind::Fixture => "fixture".to_string(),
            LayoutTargetKind::Group => "group".to_string(),
        }),
    );
    let id = target
        .name
        .parse::<u32>()
        .map_err(|_| GuiMutationError::Invalid("Layout target id must be numeric.".to_string()))?;
    mapping.insert(
        string_value("id"),
        yaml_serde::to_value(id).map_err(|error| GuiMutationError::Invalid(error.to_string()))?,
    );
    Ok(Value::Mapping(mapping))
}

fn u32_field(value: &Value, field: &str) -> Option<u32> {
    match value {
        Value::Mapping(mapping) => mapping
            .get(string_value(field))?
            .as_u64()
            .map(|value| value as u32),
        _ => None,
    }
}

fn string_field(value: &Value, field: &str) -> Option<String> {
    match value {
        Value::Mapping(mapping) => mapping
            .get(string_value(field))?
            .as_str()
            .map(ToString::to_string),
        _ => None,
    }
}

fn string_value(value: &str) -> Value {
    Value::String(value.to_string())
}

fn seconds_value(seconds: f64) -> Value {
    Value::String(format!("{seconds}s"))
}

fn sort_duration_strings(values: &mut [Value]) {
    values.sort_by(|left, right| {
        duration_seconds(left)
            .partial_cmp(&duration_seconds(right))
            .unwrap_or(std::cmp::Ordering::Equal)
    });
}

fn duration_seconds(value: &Value) -> f64 {
    value
        .as_str()
        .and_then(|value| value.strip_suffix('s'))
        .and_then(|value| value.parse::<f64>().ok())
        .unwrap_or(0.0)
}
