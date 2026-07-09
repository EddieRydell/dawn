use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::time::Duration;

use camino::{Utf8Path, Utf8PathBuf};
use dawn_language::effect::{
    CurveId, CurveSource, EffectDefinitionId, EffectInst, EffectInstId, EffectParamValue,
    EffectScope, EffectTarget,
};
use dawn_language::effect_dsl::{EffectKind, Identifier, Type, Value as EffectValue};
use dawn_language::sequence::{
    AssetId, AutomationBinding, AutomationClip, AutomationClipId, AutomationMapping,
    AutomationTarget, CompositionGraphNode, CompositionGraphNodeId, CompositionGraphNodeKind,
    EffectClip, EffectGraphEdge, GraphNodePosition, GraphOperator, GraphOperatorNode,
    GraphOperatorRef, GraphPortCardinality, GraphPortDefinition, GraphPortId, MarkCollection,
    MarkCollectionKey, SequenceAudio as DomainSequenceAudio, SequenceClip, SequenceClipId,
    SequenceClipKind, SequenceId, SequenceLayerId, validate_graph_interface,
};
use dawn_language::setup::{
    FixtureDefinitionId, FixtureGroupId, FixtureInstanceId, Geometry as DomainGeometry, LayoutId,
    LayoutTarget as DomainLayoutTarget,
};
use dawn_language::values::{
    Color, Curve, CurvePoint, CurveValue, DawnDuration, DawnTime, Distance, DistanceSpan, Point3,
    Rotation3 as DomainRotation3, Scale3 as DomainScale3,
};
use dawn_project_io::{
    ProjectSession, ReferencedAsset, SourceMap, SourceObjectId, SourceObjectKind,
    SourceObjectLocation, ensure_document_can_reference,
};
use indexmap::IndexMap;

use crate::dto::{
    ColorCurvePoint, DiagnosticSeverity, DocumentViewId, EffectScriptReference, FixtureDefinition,
    FixtureGuiDocument, FixtureGuiEdit, FloatCurvePoint, Geometry, GeometryRenderBounds,
    GeometryRenderGuide, GeometryRenderPlan, GeometryRenderPoint, GuiDocument, GuiDocumentRequest,
    GuiEditCommand, GuiObjectRef, LayoutFixturePlacement, LayoutGuiDocument, LayoutGuiEdit,
    LayoutTarget, LayoutTargetKind, ObjectKind, Point3Meters, ProjectDiagnostic,
    ResolvedLayoutFixture, Rotation3Degrees, Scale3, SequenceAudio, SequenceAutomationBinding,
    SequenceAutomationClip, SequenceAutomationMapping, SequenceCompositionGraph,
    SequenceCurveLibraryItem, SequenceCurveLibraryPoints, SequenceCurveValueType, SequenceEffect,
    SequenceEffectParam, SequenceEffectParamCurveSource, SequenceEffectParamKind,
    SequenceEffectParamValue, SequenceEffectScope, SequenceEffectScript, SequenceEffectScriptKind,
    SequenceEffectScriptParam, SequenceGraphEdge, SequenceGraphNode, SequenceGraphNodeKind,
    SequenceGraphOperator, SequenceGraphOperatorDefinition, SequenceGraphPortCardinality,
    SequenceGraphPortDefinition, SequenceGuiDocument, SequenceGuiEdit, SequenceLane, SequenceLayer,
    SequenceMarkCollection, SequenceMarkRef, SequenceParamAutomation, SequencePasteAnchor,
    SequenceResizeEdge, SequenceSelection, SequenceSelectionEdit, SequenceTimelineClipKind,
    Transform,
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
            );
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
            edit_layout(session, &resolved, edit)
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
    effect: EffectInst,
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
                copy_sequence_selection(session, &sequence_id, &selection)?;
            *clipboard = next_clipboard;
            Ok(SequenceSelectionMutation {
                selection: Some(selection),
                copied_count,
                skipped_count,
            })
        }
        SequenceSelectionEdit::Cut { selection } => {
            let (next_clipboard, copied_count, skipped_count) =
                copy_sequence_selection(session, &sequence_id, &selection)?;
            *clipboard = next_clipboard;
            delete_sequence_selection(session, &sequence_id, &selection)?;
            Ok(SequenceSelectionMutation {
                selection: None,
                copied_count,
                skipped_count,
            })
        }
        SequenceSelectionEdit::Delete { selection } => {
            delete_sequence_selection(session, &sequence_id, &selection)?;
            Ok(SequenceSelectionMutation {
                selection: None,
                copied_count: 0,
                skipped_count: 0,
            })
        }
        SequenceSelectionEdit::Paste { anchor } => {
            paste_sequence_clipboard(session, &sequence_id, anchor, clipboard.as_ref())
        }
        SequenceSelectionEdit::MoveEffects {
            ids,
            time_delta_seconds,
            lane_delta,
        } => {
            let moved =
                move_effect_selection(session, &sequence_id, &ids, time_delta_seconds, lane_delta)?;
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
            resize_effect_selection(session, &sequence_id, &ids, edge, time_delta_seconds)?;
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
            let moved = move_mark_selection(session, &sequence_id, &marks, time_delta_seconds)?;
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
            layer_id: effect.layer_id.0,
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
            params: effect_params(session, sequence, effect),
            kind: SequenceTimelineClipKind::Effect,
        })
        .collect();
    let composition_graph = SequenceCompositionGraph {
        id: 0,
        operator_catalog: GraphOperator::ALL
            .iter()
            .map(graph_operator_definition_to_gui)
            .collect(),
        nodes: sequence
            .composition_graph
            .nodes
            .iter()
            .map(|node| sequence_composition_graph_node(session, sequence, node))
            .collect(),
        edges: sequence
            .composition_graph
            .edges
            .iter()
            .map(|edge| SequenceGraphEdge {
                from_node: graph_node_id(&edge.from),
                from_port: edge.from_port.0.clone(),
                to_node: graph_node_id(&edge.to),
                to_port: edge.to_port.0.clone(),
            })
            .collect(),
    };
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
            layers: sequence
                .layers
                .iter()
                .map(|layer| SequenceLayer {
                    id: layer.id.0,
                    name: layer.name.clone(),
                    color: color_hex(layer.color),
                    enabled: layer.enabled,
                    is_default: layer.id.0 == 0,
                })
                .collect(),
            effects,
            composition_graph,
            automation_clips: automation_clips(sequence),
            degraded: false,
        },
    }
}

fn automation_clips(sequence: &dawn_language::sequence::Sequence) -> Vec<SequenceAutomationClip> {
    sequence
        .automation_clips
        .iter()
        .map(|clip| SequenceAutomationClip {
            id: clip.id.0,
            start_seconds: clip.start.as_seconds_f64(),
            duration_seconds: clip.duration.as_seconds_f64(),
            anchor_lane_index: clip.anchor_lane_index,
            lane_index: clip.lane_index,
            curve: clip
                .curve
                .points
                .iter()
                .filter_map(|point| match point.value {
                    CurveValue::Float(value) => Some(FloatCurvePoint {
                        time: point.position,
                        value,
                    }),
                    CurveValue::Color(_) => None,
                })
                .collect(),
            bindings: clip
                .bindings
                .iter()
                .map(|binding| SequenceAutomationBinding {
                    effect_id: binding.effect_id.0,
                    param: binding.param.as_str().to_string(),
                    mapping: automation_mapping_to_gui(&binding.mapping),
                })
                .collect(),
        })
        .collect()
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
    sequence: &dawn_language::sequence::Sequence,
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
                editable: automation_for_param(sequence, effect.id.0, param.name.as_str())
                    .is_none(),
                curve_source: override_value.and_then(|value| curve_source(session, value)),
                automation: automation_for_param(sequence, effect.id.0, param.name.as_str()),
                value,
            })
        })
        .collect()
}

fn sequence_composition_graph_node(
    session: &ProjectSession,
    sequence: &dawn_language::sequence::Sequence,
    node: &CompositionGraphNode,
) -> SequenceGraphNode {
    SequenceGraphNode {
        id: graph_node_id(&node.id),
        x: node.position.x,
        y: node.position.y,
        inputs: graph_node_inputs(&node.kind),
        outputs: graph_node_outputs(&node.kind),
        kind: match &node.kind {
            CompositionGraphNodeKind::Layer { layer_id } => {
                let layer = sequence.layers.iter().find(|layer| layer.id == *layer_id);
                SequenceGraphNodeKind::Layer {
                    layer_id: layer_id.0,
                    layer_name: layer
                        .map(|layer| layer.name.clone())
                        .unwrap_or_else(|| format!("Layer {}", layer_id.0)),
                    layer_color: layer
                        .map(|layer| color_hex(layer.color))
                        .unwrap_or_else(|| "#808080".to_string()),
                    enabled: layer.map(|layer| layer.enabled).unwrap_or(false),
                }
            }
            CompositionGraphNodeKind::Operator(operator) => SequenceGraphNodeKind::Operator {
                operator: graph_operator_to_gui(&operator.operator),
                params: graph_operator_params(session, sequence, &node.id, operator),
            },
            CompositionGraphNodeKind::Output => SequenceGraphNodeKind::Output,
        },
    }
}

fn graph_node_id(node_id: &CompositionGraphNodeId) -> String {
    format!("node:{}", node_id.0)
}

fn graph_operator_params(
    session: &ProjectSession,
    sequence: &dawn_language::sequence::Sequence,
    node_id: &CompositionGraphNodeId,
    operator: &GraphOperatorNode,
) -> Vec<SequenceEffectParam> {
    let GraphOperatorRef::Builtin(builtin) = &operator.operator;
    builtin
        .definition()
        .params
        .iter()
        .filter_map(|declaration| {
            let kind = param_kind(&declaration.ty)?;
            let override_value = operator.params.get(&declaration.name);
            let value = override_value
                .map(|value| effect_param_value(session, value))
                .or_else(|| declaration.default.as_ref().and_then(default_param_value))?;
            Some(SequenceEffectParam {
                name: declaration.name.as_str().to_string(),
                kind,
                options: param_options(&declaration.ty),
                editable: automation_for_composition_param(
                    sequence,
                    node_id,
                    declaration.name.as_str(),
                )
                .is_none(),
                value,
                curve_source: override_value.and_then(|value| curve_source(session, value)),
                automation: automation_for_composition_param(
                    sequence,
                    node_id,
                    declaration.name.as_str(),
                ),
            })
        })
        .collect()
}

fn graph_operator_definition_to_gui(operator: &GraphOperator) -> SequenceGraphOperatorDefinition {
    let definition = operator.definition();
    SequenceGraphOperatorDefinition {
        operator: graph_operator_to_gui(&GraphOperatorRef::Builtin(operator.clone())),
        source_name: definition.source_name.to_string(),
        display_name: definition.display_name.to_string(),
        inputs: definition.inputs.iter().map(graph_port_to_gui).collect(),
        outputs: definition.outputs.iter().map(graph_port_to_gui).collect(),
    }
}

fn graph_port_to_gui(port: &GraphPortDefinition) -> SequenceGraphPortDefinition {
    SequenceGraphPortDefinition {
        source_name: port.source_name.to_string(),
        display_name: port.display_name.to_string(),
        cardinality: match port.cardinality {
            GraphPortCardinality::One => SequenceGraphPortCardinality::One,
            GraphPortCardinality::Many => SequenceGraphPortCardinality::Many,
        },
    }
}

fn graph_node_inputs(kind: &CompositionGraphNodeKind) -> Vec<SequenceGraphPortDefinition> {
    match kind {
        CompositionGraphNodeKind::Layer { .. } => vec![],
        CompositionGraphNodeKind::Operator(operator) => {
            let GraphOperatorRef::Builtin(operator) = &operator.operator;
            operator
                .definition()
                .inputs
                .iter()
                .map(graph_port_to_gui)
                .collect()
        }
        CompositionGraphNodeKind::Output => vec![SequenceGraphPortDefinition {
            source_name: "input".to_string(),
            display_name: "Input".to_string(),
            cardinality: SequenceGraphPortCardinality::Many,
        }],
    }
}

fn graph_node_outputs(kind: &CompositionGraphNodeKind) -> Vec<SequenceGraphPortDefinition> {
    match kind {
        CompositionGraphNodeKind::Layer { .. } | CompositionGraphNodeKind::Operator(_) => {
            vec![SequenceGraphPortDefinition {
                source_name: "output".to_string(),
                display_name: "Output".to_string(),
                cardinality: SequenceGraphPortCardinality::Many,
            }]
        }
        CompositionGraphNodeKind::Output => vec![],
    }
}

fn graph_operator_to_gui(operator: &GraphOperatorRef) -> SequenceGraphOperator {
    let GraphOperatorRef::Builtin(operator) = operator;
    match operator {
        GraphOperator::Max => SequenceGraphOperator::Max,
        GraphOperator::Add => SequenceGraphOperator::Add,
        GraphOperator::Multiply => SequenceGraphOperator::Multiply,
        GraphOperator::IntensityModulate => SequenceGraphOperator::IntensityModulate,
        GraphOperator::Dim => SequenceGraphOperator::Dim,
        GraphOperator::Invert => SequenceGraphOperator::Invert,
        GraphOperator::Colorize => SequenceGraphOperator::Colorize,
        GraphOperator::Delay => SequenceGraphOperator::Delay,
        GraphOperator::Echo => SequenceGraphOperator::Echo,
    }
}

fn automation_for_param(
    sequence: &dawn_language::sequence::Sequence,
    effect_id: u32,
    param: &str,
) -> Option<SequenceParamAutomation> {
    sequence.automation_clips.iter().find_map(|clip| {
        clip.bindings
            .iter()
            .find(|binding| binding.effect_id.0 == effect_id && binding.param.as_str() == param)
            .map(|binding| SequenceParamAutomation {
                clip_id: clip.id.0,
                mapping: automation_mapping_to_gui(&binding.mapping),
            })
    })
}

fn automation_for_composition_param(
    sequence: &dawn_language::sequence::Sequence,
    node_id: &CompositionGraphNodeId,
    param: &str,
) -> Option<SequenceParamAutomation> {
    sequence.automation_clips.iter().find_map(|clip| {
        clip.bindings
            .iter()
            .find(|binding| {
                matches!(
                    &binding.target,
                    AutomationTarget::CompositionNodeParam {
                        node_id: target_node_id,
                        param: target_param,
                    } if target_node_id == node_id && target_param.as_str() == param
                )
            })
            .map(|binding| SequenceParamAutomation {
                clip_id: clip.id.0,
                mapping: automation_mapping_to_gui(&binding.mapping),
            })
    })
}

fn automation_mapping_to_gui(mapping: &AutomationMapping) -> SequenceAutomationMapping {
    match mapping {
        AutomationMapping::Float { min, max } => SequenceAutomationMapping::Float {
            min: *min,
            max: *max,
        },
        AutomationMapping::Int { min, max } => SequenceAutomationMapping::Int {
            min: *min as f64,
            max: *max as f64,
        },
        AutomationMapping::Bool => SequenceAutomationMapping::Bool,
        AutomationMapping::Enum { values } => SequenceAutomationMapping::Enum {
            values: values
                .iter()
                .map(|value| value.as_str().to_string())
                .collect(),
        },
        AutomationMapping::FloatCurve { min, max } => SequenceAutomationMapping::FloatCurve {
            min: *min,
            max: *max,
        },
    }
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

fn default_effect_param_value(ty: &Type) -> Option<EffectParamValue> {
    Some(match ty {
        Type::Int => EffectParamValue::Int(0),
        Type::Float => EffectParamValue::Float(0.0),
        Type::Bool => EffectParamValue::Bool(false),
        Type::Color => EffectParamValue::Color(Color {
            red: 255,
            green: 255,
            blue: 255,
        }),
        Type::Enum(options) => EffectParamValue::Enum(options.first()?.clone()),
        Type::Marks => return None,
        Type::Curve(inner) => EffectParamValue::Curve(CurveSource::Inline(default_curve(inner))),
        Type::Array(inner) => {
            let item = default_effect_param_value(inner)?;
            EffectParamValue::Array(vec![item])
        }
        Type::Void | Type::Timeline | Type::Target | Type::TargetItems | Type::TargetItem => {
            return None;
        }
    })
}

fn default_curve(ty: &Type) -> Curve {
    let value = match ty {
        Type::Color => CurveValue::Color(Color {
            red: 255,
            green: 255,
            blue: 255,
        }),
        _ => CurveValue::Float(1.0),
    };
    Curve {
        points: vec![CurvePoint {
            position: 0.0,
            value,
        }],
    }
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
    resolved: &ResolvedGuiObject,
    edit: LayoutGuiEdit,
) -> Result<(), GuiMutationError> {
    let layout_id = LayoutId(resolved.source_id.id.clone());
    let layout = session
        .project
        .layouts
        .get_mut(&layout_id)
        .ok_or_else(|| GuiMutationError::Invalid("Layout was not found.".to_string()))?;
    match edit {
        LayoutGuiEdit::UpdatePlacementTransform { id, transform } => {
            let fixture = layout
                .fixtures
                .iter_mut()
                .find(|fixture| fixture.id.0 == id)
                .ok_or_else(|| {
                    GuiMutationError::Invalid("Fixture placement was not found.".to_string())
                })?;
            fixture.position = domain_point3_meters(transform.position);
            fixture.rotation = rotation3_degrees(transform.rotation);
            fixture.scale = scale3(transform.scale);
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
            let definition = fixture_definition_mut(session, &location.document, &object_key)?;
            definition.bulb_radius = distance_span(bulb_diameter_meters / 2.0);
            Ok(())
        }
        FixtureGuiEdit::MovePoint {
            object_key,
            point_index,
            point,
        } => {
            let definition = fixture_definition_mut(session, &location.document, &object_key)?;
            let DomainGeometry::Points { points } = &mut definition.geometry else {
                return Err(GuiMutationError::Invalid(
                    "Fixture geometry does not contain movable points.".to_string(),
                ));
            };
            let target = points.get_mut(point_index as usize).ok_or_else(|| {
                GuiMutationError::Invalid("Fixture point was not found.".to_string())
            })?;
            *target = domain_point3_meters(point);
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
    let sequence_id = SequenceId(resolved.source_id.id.clone());
    match edit {
        SequenceGuiEdit::SetDuration { duration_seconds } => {
            if !duration_seconds.is_finite() || duration_seconds <= 0.0 {
                return Err(GuiMutationError::Invalid(
                    "Sequence duration must be greater than zero.".to_string(),
                ));
            }
            sequence_mut(session, &sequence_id)?.duration = dawn_duration(duration_seconds);
        }
        SequenceGuiEdit::SetAudio { import_path } => {
            let audio = match import_path {
                Some(import_path) => {
                    let document = resolved.location.document.clone();
                    let id = register_sequence_audio_asset(session, &document, &import_path)?;
                    DomainSequenceAudio::Asset(id)
                }
                None => DomainSequenceAudio::None,
            };
            sequence_mut(session, &sequence_id)?.audio = audio;
        }
        SequenceGuiEdit::MoveEffect {
            id,
            start_seconds,
            target,
        } => {
            let parsed_target = target.map(layout_target_to_effect_target).transpose()?;
            let sequence = sequence_mut(session, &sequence_id)?;
            clip_mut(sequence, id)?.start = dawn_time(start_seconds.max(0.0));
            if let Some(target) = parsed_target.clone() {
                clip_mut(sequence, id)?.target = target;
            }
            if let Ok(effect) = effect_mut(sequence, id) {
                effect.start = dawn_time(start_seconds.max(0.0));
                if let Some(target) = parsed_target {
                    effect.target = target;
                }
            }
        }
        SequenceGuiEdit::ResizeEffect {
            id,
            start_seconds,
            duration_seconds,
        } => {
            let sequence = sequence_mut(session, &sequence_id)?;
            let start = dawn_time(start_seconds.max(0.0));
            let duration = dawn_duration(duration_seconds.max(0.000000001));
            let clip = clip_mut(sequence, id)?;
            clip.start = start.clone();
            clip.duration = duration.clone();
            if let Ok(effect) = effect_mut(sequence, id) {
                effect.start = start;
                effect.duration = duration;
            }
        }
        SequenceGuiEdit::SetEffectScope { id, scope } => {
            let sequence = sequence_mut(session, &sequence_id)?;
            let scope = effect_scope(scope);
            clip_mut(sequence, id)?.scope = scope.clone();
            if let Ok(effect) = effect_mut(sequence, id) {
                effect.scope = scope;
            }
        }
        SequenceGuiEdit::RetargetEffect { id, target } => {
            let sequence = sequence_mut(session, &sequence_id)?;
            let target = layout_target_to_effect_target(target)?;
            clip_mut(sequence, id)?.target = target.clone();
            if let Ok(effect) = effect_mut(sequence, id) {
                effect.target = target;
            }
        }
        SequenceGuiEdit::DeleteEffect { id } => {
            let sequence = sequence_mut(session, &sequence_id)?;
            sequence.effects.retain(|effect| effect.id.0 != id);
            sequence.clips.retain(|clip| clip.id.0 != id);
            for clip in &mut sequence.automation_clips {
                clip.bindings.retain(|binding| binding.effect_id.0 != id);
            }
            sequence
                .automation_clips
                .retain(|clip| !clip.bindings.is_empty());
        }
        SequenceGuiEdit::MoveMark {
            collection_key,
            index,
            time_seconds,
        } => {
            let collection =
                mark_collection_mut(sequence_mut(session, &sequence_id)?, &collection_key)?;
            let mark = collection
                .marks
                .get_mut(index as usize)
                .ok_or_else(|| GuiMutationError::Invalid("Mark was not found.".to_string()))?;
            *mark = dawn_time(time_seconds.max(0.0));
            collection.marks.sort_by_key(|time| time.0);
        }
        SequenceGuiEdit::ReassignMarkCollection {
            collection_key,
            index,
            target_collection_key,
        } => {
            if collection_key != target_collection_key {
                let sequence = sequence_mut(session, &sequence_id)?;
                let mark = {
                    let collection = mark_collection_mut(sequence, &collection_key)?;
                    if (index as usize) >= collection.marks.len() {
                        return Err(GuiMutationError::Invalid("Mark was not found.".to_string()));
                    }
                    collection.marks.remove(index as usize)
                };
                let target_collection = mark_collection_mut(sequence, &target_collection_key)?;
                target_collection.marks.push(mark);
                target_collection.marks.sort_by_key(|time| time.0);
            }
        }
        SequenceGuiEdit::AddMark {
            collection_key,
            time_seconds,
        } => {
            let collection =
                mark_collection_mut(sequence_mut(session, &sequence_id)?, &collection_key)?;
            collection.marks.push(dawn_time(time_seconds.max(0.0)));
            collection.marks.sort_by_key(|time| time.0);
        }
        SequenceGuiEdit::DeleteMark {
            collection_key,
            index,
        } => {
            let collection =
                mark_collection_mut(sequence_mut(session, &sequence_id)?, &collection_key)?;
            if (index as usize) < collection.marks.len() {
                collection.marks.remove(index as usize);
            }
        }
        SequenceGuiEdit::CreateMarkCollection { key, name, color } => {
            sequence_mut(session, &sequence_id)?
                .mark_collections
                .push(MarkCollection {
                    key: MarkCollectionKey { name: key },
                    name,
                    display_color: parse_color(&color)?,
                    marks: Vec::new(),
                });
        }
        SequenceGuiEdit::RenameMarkCollection { key, name } => {
            mark_collection_mut(sequence_mut(session, &sequence_id)?, &key)?.name = name;
        }
        SequenceGuiEdit::DeleteMarkCollection { key } => {
            sequence_mut(session, &sequence_id)?
                .mark_collections
                .retain(|collection| collection.key.name != key);
        }
        SequenceGuiEdit::SetMarkCollectionColor { key, color } => {
            mark_collection_mut(sequence_mut(session, &sequence_id)?, &key)?.display_color =
                parse_color(&color)?;
        }
        SequenceGuiEdit::UpdateEffectParam { id, name, value } => {
            let value = effect_param_value_from_gui(value)?;
            effect_mut(sequence_mut(session, &sequence_id)?, id)?
                .param_overrides
                .insert(identifier(&name)?, value);
        }
        SequenceGuiEdit::AddEffect {
            script,
            target,
            scope,
            start_seconds,
            mark_collection_key,
        } => {
            let definition = EffectDefinitionId(script.effect_name);
            let Some(effect_definition) = session.project.definitions.effects.get(&definition)
            else {
                return Err(GuiMutationError::Invalid(
                    "Effect script was not found.".to_string(),
                ));
            };
            let params = effect_definition.compiled.params().to_vec();
            ensure_document_can_reference(
                session,
                &resolved.location.document,
                SourceObjectKind::EffectDefinition,
                &definition.0,
            )
            .map_err(|error| GuiMutationError::Blocked(error.to_string()))?;
            let sequence = sequence_mut(session, &sequence_id)?;
            let next_id = sequence
                .effects
                .iter()
                .map(|effect| effect.id.0)
                .max()
                .unwrap_or(0)
                + 1;
            let mut param_overrides = IndexMap::new();
            if let Some(key) = mark_collection_key {
                for name in add_effect_mark_params {
                    param_overrides.insert(
                        identifier(&name)?,
                        EffectParamValue::Marks(MarkCollectionKey { name: key.clone() }),
                    );
                }
            }
            for param in params.iter().filter(|param| param.default.is_none()) {
                if param_overrides.contains_key(&param.name) {
                    continue;
                }
                if let Some(value) = default_effect_param_value(&param.ty) {
                    param_overrides.insert(param.name.clone(), value);
                }
            }
            sequence.effects.push(EffectInst {
                id: EffectInstId(next_id),
                layer_id: dawn_language::sequence::SequenceLayerId(0),
                start: dawn_time(start_seconds.max(0.0)),
                duration: dawn_duration(1.0),
                target: layout_target_to_effect_target(target)?,
                scope: effect_scope(scope),
                definition,
                param_overrides,
            });
        }
        SequenceGuiEdit::CreateLayer { name, color } => {
            create_sequence_layer(session, &sequence_id, name, color, None, true)?;
        }
        SequenceGuiEdit::CreateLayerAt { name, color, x, y } => {
            create_sequence_layer(session, &sequence_id, name, color, Some((x, y)), false)?;
        }
        SequenceGuiEdit::RenameLayer { id, name } => {
            let layer = sequence_mut(session, &sequence_id)?
                .layers
                .iter_mut()
                .find(|layer| layer.id.0 == id)
                .ok_or_else(|| GuiMutationError::Invalid("Layer was not found.".to_string()))?;
            layer.name = name;
        }
        SequenceGuiEdit::SetLayerColor { id, color } => {
            let layer = sequence_mut(session, &sequence_id)?
                .layers
                .iter_mut()
                .find(|layer| layer.id.0 == id)
                .ok_or_else(|| GuiMutationError::Invalid("Layer was not found.".to_string()))?;
            layer.color = parse_color(&color)?;
        }
        SequenceGuiEdit::SetLayerEnabled { id, enabled } => {
            let layer = sequence_mut(session, &sequence_id)?
                .layers
                .iter_mut()
                .find(|layer| layer.id.0 == id)
                .ok_or_else(|| GuiMutationError::Invalid("Layer was not found.".to_string()))?;
            layer.enabled = enabled;
        }
        SequenceGuiEdit::DeleteLayer {
            id,
            migrate_to_layer_id,
        } => {
            if id == 0 {
                return Err(GuiMutationError::Invalid(
                    "Default layer cannot be deleted.".to_string(),
                ));
            }
            let sequence = sequence_mut(session, &sequence_id)?;
            if !sequence.layers.iter().any(|layer| layer.id.0 == id) {
                return Err(GuiMutationError::Invalid(
                    "Layer was not found.".to_string(),
                ));
            }
            if migrate_to_layer_id == id
                || !sequence
                    .layers
                    .iter()
                    .any(|layer| layer.id.0 == migrate_to_layer_id)
            {
                return Err(GuiMutationError::Invalid(
                    "Effect migration layer was not found.".to_string(),
                ));
            }
            for effect in &mut sequence.effects {
                if effect.layer_id.0 == id {
                    effect.layer_id = SequenceLayerId(migrate_to_layer_id);
                }
            }
            sequence.layers.retain(|layer| layer.id.0 != id);
            let layer_node_ids = sequence
                .composition_graph
                .nodes
                .iter()
                .filter_map(|node| match &node.kind {
                    CompositionGraphNodeKind::Layer { layer_id } if layer_id.0 == id => {
                        Some(node.id.clone())
                    }
                    _ => None,
                })
                .collect::<BTreeSet<_>>();
            sequence
                .composition_graph
                .nodes
                .retain(|node| !layer_node_ids.contains(&node.id));
            sequence.composition_graph.edges.retain(|edge| {
                !layer_node_ids.contains(&edge.from) && !layer_node_ids.contains(&edge.to)
            });
        }
        SequenceGuiEdit::SetEffectLayer { id, layer_id } => {
            let sequence = sequence_mut(session, &sequence_id)?;
            if !sequence.layers.iter().any(|layer| layer.id.0 == layer_id) {
                return Err(GuiMutationError::Invalid(
                    "Layer was not found.".to_string(),
                ));
            }
            let effect = sequence
                .effects
                .iter_mut()
                .find(|effect| effect.id.0 == id)
                .ok_or_else(|| GuiMutationError::Invalid("Effect was not found.".to_string()))?;
            effect.layer_id = SequenceLayerId(layer_id);
        }
        SequenceGuiEdit::ChangeEffectScript { id, script } => {
            let definition = EffectDefinitionId(script.effect_name);
            if !session
                .project
                .definitions
                .effects
                .definitions
                .contains_key(&definition)
            {
                return Err(GuiMutationError::Invalid(
                    "Effect script was not found.".to_string(),
                ));
            }
            ensure_document_can_reference(
                session,
                &resolved.location.document,
                SourceObjectKind::EffectDefinition,
                &definition.0,
            )
            .map_err(|error| GuiMutationError::Blocked(error.to_string()))?;
            effect_mut(sequence_mut(session, &sequence_id)?, id)?.definition = definition;
        }
        SequenceGuiEdit::LinkEffectCurveParam {
            id,
            name,
            curve_path: _,
            object_key,
        } => {
            let curve = CurveId(object_key);
            if !session
                .project
                .definitions
                .curves
                .definitions
                .contains_key(&curve)
            {
                return Err(GuiMutationError::Invalid(
                    "Curve was not found.".to_string(),
                ));
            }
            ensure_document_can_reference(
                session,
                &resolved.location.document,
                SourceObjectKind::Curve,
                &curve.0,
            )
            .map_err(|error| GuiMutationError::Blocked(error.to_string()))?;
            effect_mut(sequence_mut(session, &sequence_id)?, id)?
                .param_overrides
                .insert(
                    identifier(&name)?,
                    EffectParamValue::Curve(CurveSource::Reference(curve)),
                );
        }
        SequenceGuiEdit::UnlinkEffectCurveParam { id, name } => {
            let value = unlink_curve_value.ok_or_else(|| {
                GuiMutationError::Invalid("Curve param could not be resolved.".to_string())
            })?;
            effect_mut(sequence_mut(session, &sequence_id)?, id)?
                .param_overrides
                .insert(identifier(&name)?, effect_param_value_from_gui(value)?);
        }
        SequenceGuiEdit::AddGraphOperatorNode { operator, x, y } => {
            let sequence = sequence_mut(session, &sequence_id)?;
            let next_id = next_composition_node_id(sequence);
            sequence.composition_graph.nodes.push(CompositionGraphNode {
                id: CompositionGraphNodeId(next_id),
                position: GraphNodePosition { x, y },
                kind: CompositionGraphNodeKind::Operator(GraphOperatorNode {
                    operator: GraphOperatorRef::Builtin(graph_operator_from_gui(&operator)),
                    params: IndexMap::new(),
                }),
            });
        }
        SequenceGuiEdit::MoveGraphNode { node_id, x, y } => {
            let sequence = sequence_mut(session, &sequence_id)?;
            let node_id = parse_graph_node_id(&node_id)?;
            let node = composition_graph_node_mut(sequence, &node_id)?;
            node.position = GraphNodePosition { x, y };
        }
        SequenceGuiEdit::DeleteGraphNode { node_id } => {
            let sequence = sequence_mut(session, &sequence_id)?;
            let node_id = parse_graph_node_id(&node_id)?;
            let node = sequence
                .composition_graph
                .nodes
                .iter()
                .find(|node| node.id == node_id)
                .ok_or_else(|| {
                    GuiMutationError::Invalid("Graph node was not found.".to_string())
                })?;
            match &node.kind {
                CompositionGraphNodeKind::Layer { .. } => {
                    return Err(GuiMutationError::Invalid(
                        "Delete the layer from the layer list.".to_string(),
                    ));
                }
                CompositionGraphNodeKind::Output => {
                    return Err(GuiMutationError::Invalid(
                        "Output node cannot be deleted.".to_string(),
                    ));
                }
                CompositionGraphNodeKind::Operator(_) => {}
            }
            sequence
                .composition_graph
                .nodes
                .retain(|node| node.id != node_id);
            sequence
                .composition_graph
                .edges
                .retain(|edge| edge.from != node_id && edge.to != node_id);
        }
        SequenceGuiEdit::ConnectGraphNodes {
            from_node,
            from_port,
            to_node,
            to_port,
        } => {
            if from_node == to_node {
                return Err(GuiMutationError::Invalid(
                    "Graph node cannot connect to itself.".to_string(),
                ));
            }
            let sequence = sequence_mut(session, &sequence_id)?;
            let from = parse_graph_node_id(&from_node)?;
            let to = parse_graph_node_id(&to_node)?;
            ensure_graph_node_exists(sequence, &from)?;
            ensure_graph_node_exists(sequence, &to)?;
            if sequence.composition_graph.edges.iter().any(|edge| {
                edge.from == from
                    && edge.from_port.0 == from_port
                    && edge.to == to
                    && edge.to_port.0 == to_port
            }) {
                return Ok(());
            }
            let mut graph = sequence.composition_graph.clone();
            let single_input = graph
                .nodes
                .iter()
                .find(|node| node.id == to)
                .and_then(|node| graph_input_cardinality(&node.kind, &to_port))
                == Some(GraphPortCardinality::One);
            if single_input {
                graph
                    .edges
                    .retain(|edge| edge.to != to || edge.to_port.0 != to_port);
            }
            graph.edges.push(EffectGraphEdge {
                from,
                from_port: GraphPortId(from_port),
                to,
                to_port: GraphPortId(to_port),
            });
            validate_graph_interface(&graph)
                .map_err(|error| GuiMutationError::Invalid(error.message))?;
            sequence.composition_graph = graph;
        }
        SequenceGuiEdit::DisconnectGraphNodes {
            from_node,
            from_port,
            to_node,
            to_port,
        } => {
            let sequence = sequence_mut(session, &sequence_id)?;
            let from = parse_graph_node_id(&from_node)?;
            let to = parse_graph_node_id(&to_node)?;
            sequence.composition_graph.edges.retain(|edge| {
                !(edge.from == from
                    && edge.from_port.0 == from_port
                    && edge.to == to
                    && edge.to_port.0 == to_port)
            });
        }
        SequenceGuiEdit::UpdateGraphOperatorParam {
            node_id,
            name,
            value,
        } => {
            let sequence = sequence_mut(session, &sequence_id)?;
            let node_id = parse_graph_node_id(&node_id)?;
            let mut graph = sequence.composition_graph.clone();
            let node = graph
                .nodes
                .iter_mut()
                .find(|node| node.id == node_id)
                .ok_or_else(|| {
                    GuiMutationError::Invalid("Graph node was not found.".to_string())
                })?;
            let CompositionGraphNodeKind::Operator(operator) = &mut node.kind else {
                return Err(GuiMutationError::Invalid(
                    "Graph node is not an operator.".to_string(),
                ));
            };
            operator
                .params
                .insert(identifier(&name)?, effect_param_value_from_gui(value)?);
            validate_graph_interface(&graph)
                .map_err(|error| GuiMutationError::Invalid(error.message))?;
            sequence.composition_graph = graph;
        }
        SequenceGuiEdit::AddAutomationClip {
            start_seconds,
            duration_seconds,
            anchor_lane_index,
            lane_index,
        } => {
            let sequence = sequence_mut(session, &sequence_id)?;
            let next_id = sequence
                .automation_clips
                .iter()
                .map(|clip| clip.id.0)
                .max()
                .unwrap_or(0)
                + 1;
            sequence.automation_clips.push(AutomationClip {
                id: AutomationClipId(next_id),
                start: dawn_time(start_seconds.max(0.0)),
                duration: dawn_duration(duration_seconds.max(0.000000001)),
                anchor_lane_index,
                lane_index,
                curve: default_automation_curve(),
                bindings: Vec::new(),
            });
        }
        SequenceGuiEdit::CreateAndBindAutomationClip {
            effect_id,
            param,
            mapping,
        } => {
            let param = identifier(&param)?;
            let mapping = automation_mapping_from_gui(mapping)?;
            let (effect_start, effect_duration, anchor_lane_index) = {
                let sequence = session.project.sequences.get(&sequence_id).ok_or_else(|| {
                    GuiMutationError::Invalid("Sequence was not found.".to_string())
                })?;
                let effect = sequence
                    .effects
                    .iter()
                    .find(|effect| effect.id.0 == effect_id)
                    .ok_or_else(|| {
                        GuiMutationError::Invalid("Effect was not found.".to_string())
                    })?;
                let anchor_lane_index = effect_lane_index_resolved(session, &effect.target)
                    .ok_or_else(|| {
                        GuiMutationError::Invalid("Effect lane was not found.".to_string())
                    })?;
                (
                    effect.start.clone(),
                    effect.duration.clone(),
                    anchor_lane_index as u32,
                )
            };
            let sequence = sequence_mut(session, &sequence_id)?;
            for clip in &sequence.automation_clips {
                if clip
                    .bindings
                    .iter()
                    .any(|binding| binding.effect_id.0 == effect_id && binding.param == param)
                {
                    return Err(GuiMutationError::Invalid(
                        "Param is already automated.".to_string(),
                    ));
                }
            }
            let next_id = sequence
                .automation_clips
                .iter()
                .map(|clip| clip.id.0)
                .max()
                .unwrap_or(0)
                + 1;
            sequence.automation_clips.push(AutomationClip {
                id: AutomationClipId(next_id),
                start: effect_start,
                duration: effect_duration,
                anchor_lane_index,
                lane_index: 0,
                curve: default_automation_curve(),
                bindings: vec![AutomationBinding {
                    target: AutomationTarget::EffectParam {
                        effect_id: EffectInstId(effect_id),
                        param: param.clone(),
                    },
                    effect_id: EffectInstId(effect_id),
                    param,
                    mapping,
                }],
            });
        }
        SequenceGuiEdit::MoveAutomationClip {
            id,
            start_seconds,
            anchor_lane_index,
            lane_index,
        } => {
            let clip = automation_clip_mut(sequence_mut(session, &sequence_id)?, id)?;
            clip.start = dawn_time(start_seconds.max(0.0));
            clip.anchor_lane_index = anchor_lane_index;
            clip.lane_index = lane_index;
        }
        SequenceGuiEdit::ResizeAutomationClip {
            id,
            start_seconds,
            duration_seconds,
        } => {
            let clip = automation_clip_mut(sequence_mut(session, &sequence_id)?, id)?;
            clip.start = dawn_time(start_seconds.max(0.0));
            clip.duration = dawn_duration(duration_seconds.max(0.000000001));
        }
        SequenceGuiEdit::UpdateAutomationCurve { id, curve } => {
            automation_clip_mut(sequence_mut(session, &sequence_id)?, id)?.curve =
                float_curve(curve);
        }
        SequenceGuiEdit::DeleteAutomationClip { id } => {
            sequence_mut(session, &sequence_id)?
                .automation_clips
                .retain(|clip| clip.id.0 != id);
        }
        SequenceGuiEdit::BindAutomationParam {
            clip_id,
            effect_id,
            param,
            mapping,
        } => {
            let param = identifier(&param)?;
            let sequence = sequence_mut(session, &sequence_id)?;
            for clip in &sequence.automation_clips {
                if clip
                    .bindings
                    .iter()
                    .any(|binding| binding.effect_id.0 == effect_id && binding.param == param)
                {
                    return Err(GuiMutationError::Invalid(
                        "Param is already automated.".to_string(),
                    ));
                }
            }
            automation_clip_mut(sequence, clip_id)?
                .bindings
                .push(AutomationBinding {
                    target: AutomationTarget::EffectParam {
                        effect_id: EffectInstId(effect_id),
                        param: param.clone(),
                    },
                    effect_id: EffectInstId(effect_id),
                    param,
                    mapping: automation_mapping_from_gui(mapping)?,
                });
        }
        SequenceGuiEdit::UnbindAutomationParam {
            clip_id,
            effect_id,
            param,
        } => {
            let param_id = identifier(&param)?;
            let sequence = sequence_mut(session, &sequence_id)?;
            let clip = sequence
                .automation_clips
                .iter()
                .find(|clip| clip.id.0 == clip_id)
                .cloned()
                .ok_or_else(|| {
                    GuiMutationError::Invalid("Automation clip was not found.".to_string())
                })?;
            let Some(binding) = clip
                .bindings
                .iter()
                .find(|binding| binding.effect_id.0 == effect_id && binding.param == param_id)
                .cloned()
            else {
                return Err(GuiMutationError::Invalid(
                    "Automation binding was not found.".to_string(),
                ));
            };
            let effect_start = sequence
                .effects
                .iter()
                .find(|effect| effect.id.0 == effect_id)
                .map(|effect| effect.start.as_seconds_f64())
                .ok_or_else(|| GuiMutationError::Invalid("Effect was not found.".to_string()))?;
            let value = automation_binding_value_at(&clip, &binding, effect_start)?;
            effect_mut(sequence, effect_id)?
                .param_overrides
                .insert(param_id.clone(), value);
            automation_clip_mut(sequence, clip_id)?
                .bindings
                .retain(|binding| !(binding.effect_id.0 == effect_id && binding.param == param_id));
        }
    }
    sync_sequence_effect_clips(sequence_mut(session, &sequence_id)?);
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
    sequence_id: &SequenceId,
    selection: &SequenceSelection,
) -> Result<(Option<SequenceClipboard>, u32, u32), GuiMutationError> {
    match selection {
        SequenceSelection::Effects { ids } => {
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
                copied.push(ClipboardEffect {
                    effect: effect.clone(),
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
            let sequence =
                session.project.sequences.get(sequence_id).ok_or_else(|| {
                    GuiMutationError::Invalid("Sequence was not found.".to_string())
                })?;
            let mut copied = Vec::new();
            let mut skipped = 0u32;
            for mark in marks {
                let Some(time_seconds) = mark_time_seconds(sequence, mark) else {
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
    sequence_id: &SequenceId,
    selection: &SequenceSelection,
) -> Result<(), GuiMutationError> {
    let sequence = sequence_mut(session, sequence_id)?;
    match selection {
        SequenceSelection::Effects { ids } => {
            sequence
                .effects
                .retain(|effect| !ids.contains(&effect.id.0));
            sync_sequence_effect_clips(sequence);
        }
        SequenceSelection::Marks { marks } => {
            for (collection_key, indexes) in mark_indexes_by_collection(marks) {
                for index in indexes.into_iter().rev() {
                    let collection = mark_collection_mut(sequence, &collection_key)?;
                    if index < collection.marks.len() {
                        collection.marks.remove(index);
                    }
                }
            }
        }
    }
    Ok(())
}

fn paste_sequence_clipboard(
    session: &mut ProjectSession,
    sequence_id: &SequenceId,
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
            let sequence = sequence_mut(session, sequence_id)?;
            let mut next_id = sequence
                .effects
                .iter()
                .map(|effect| effect.id.0)
                .max()
                .unwrap_or(0)
                .saturating_add(1);
            let mut pasted_ids = Vec::with_capacity(effects.len());
            for effect in effects {
                let mut value = effect.effect.clone();
                let target_lane = anchored_lane(
                    anchor.lane_index as usize,
                    effect.lane_index,
                    min_lane,
                    lane_count,
                );
                value.id = EffectInstId(next_id);
                value.start =
                    dawn_time((anchor.time_seconds + effect.start_seconds - min_start).max(0.0));
                if let Some(Some(target)) = lane_targets.get(target_lane) {
                    value.target = target.clone();
                }
                sequence.effects.push(value);
                pasted_ids.push(next_id);
                next_id = next_id.saturating_add(1);
            }
            sync_sequence_effect_clips(sequence);
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
            let sequence = sequence_mut(session, sequence_id)?;
            for mark in marks {
                let collection = match mark_collection_mut(sequence, &mark.collection_key) {
                    Ok(collection) => collection,
                    Err(_) => {
                        skipped = skipped.saturating_add(1);
                        continue;
                    }
                };
                let time_seconds = (anchor.time_seconds + mark.time_seconds - min_time).max(0.0);
                collection.marks.push(dawn_time(time_seconds));
                collection.marks.sort_by_key(|time| time.0);
                let index = collection
                    .marks
                    .iter()
                    .position(|value| (value.as_seconds_f64() - time_seconds).abs() < f64::EPSILON)
                    .unwrap_or_else(|| collection.marks.len().saturating_sub(1));
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
    apply_effect_updates(session, sequence_id, effect_updates)
}

fn resize_effect_selection(
    session: &mut ProjectSession,
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
    apply_effect_updates(session, sequence_id, effect_updates)?;
    Ok(())
}

fn move_mark_selection(
    session: &mut ProjectSession,
    sequence_id: &SequenceId,
    marks: &[SequenceMarkRef],
    time_delta_seconds: f64,
) -> Result<Vec<SequenceMarkRef>, GuiMutationError> {
    let sequence = sequence_mut(session, sequence_id)?;
    let mut moved = Vec::new();
    for (collection_key, indexes) in mark_indexes_by_collection(marks) {
        let mut moved_times = Vec::new();
        for index in indexes {
            let collection = mark_collection_mut(sequence, &collection_key)?;
            if let Some(value) = collection.marks.get_mut(index) {
                let time_seconds = (value.as_seconds_f64() + time_delta_seconds).max(0.0);
                *value = dawn_time(time_seconds);
                moved_times.push(time_seconds);
            }
        }
        let collection = mark_collection_mut(sequence, &collection_key)?;
        collection.marks.sort_by_key(|time| time.0);
        for time_seconds in moved_times {
            if let Some(index) = collection
                .marks
                .iter()
                .position(|value| (value.as_seconds_f64() - time_seconds).abs() < f64::EPSILON)
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
    sequence_id: &SequenceId,
    updates: Vec<EffectUpdate>,
) -> Result<Vec<u32>, GuiMutationError> {
    let targets = updates
        .iter()
        .map(|update| (update.id, target_for_lane(session, update.lane_index)))
        .collect::<Vec<_>>();
    let sequence = sequence_mut(session, sequence_id)?;
    let mut moved = Vec::new();
    for update in updates {
        let effect = effect_mut(sequence, update.id)?;
        effect.start = dawn_time(update.start_seconds.max(0.0));
        effect.duration = dawn_duration(update.duration_seconds.max(0.000000001));
        if let Some((_, Some(target))) = targets.iter().find(|(id, _)| *id == update.id) {
            effect.target = target.clone();
        }
        moved.push(update.id);
    }
    sync_sequence_effect_clips(sequence);
    Ok(moved)
}

fn mark_time_seconds(
    sequence: &dawn_language::sequence::Sequence,
    mark: &SequenceMarkRef,
) -> Option<f64> {
    sequence
        .mark_collections
        .iter()
        .find(|collection| collection.key.name == mark.collection_key)?
        .marks
        .get(mark.index as usize)
        .map(DawnTime::as_seconds_f64)
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
    effect_lane_index_resolved(session, target).unwrap_or_default()
}

fn effect_lane_index_resolved(session: &ProjectSession, target: &EffectTarget) -> Option<usize> {
    let layout_id = active_layout_id(session)?;
    let layout = session.project.layouts.get(&layout_id)?;
    layout
        .target_order
        .iter()
        .position(|candidate| effect_target_matches_layout(target, candidate))
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

fn sequence_mut<'a>(
    session: &'a mut ProjectSession,
    id: &SequenceId,
) -> Result<&'a mut dawn_language::sequence::Sequence, GuiMutationError> {
    session
        .project
        .sequences
        .get_mut(id)
        .ok_or_else(|| GuiMutationError::Invalid("Sequence was not found.".to_string()))
}

fn register_sequence_audio_asset(
    session: &mut ProjectSession,
    document: &Utf8Path,
    import_path: &str,
) -> Result<AssetId, GuiMutationError> {
    if let Some(asset) = session
        .source
        .referenced_assets
        .iter()
        .find(|asset| asset.relative_path.as_str() == import_path)
    {
        return Ok(asset.id.clone());
    }

    let document_path = session.source.source_root.join(document);
    let document_dir = document_path
        .parent()
        .unwrap_or(&session.source.source_root)
        .to_path_buf();
    let selected_path = document_dir.join(import_path);
    let absolute_path = fs::canonicalize(&selected_path)
        .map_err(|error| GuiMutationError::Invalid(format!("Audio file was not found: {error}")))?;
    let absolute_path = Utf8PathBuf::from_path_buf(absolute_path).map_err(|path| {
        GuiMutationError::Invalid(format!("Audio path is not valid UTF-8: {}", path.display()))
    })?;
    if !absolute_path.is_file() {
        return Err(GuiMutationError::Invalid(
            "Selected audio path is not a file.".to_string(),
        ));
    }

    if let Some(asset) = session
        .source
        .referenced_assets
        .iter()
        .find(|asset| asset.absolute_path == absolute_path)
    {
        return Ok(asset.id.clone());
    }

    let relative_path = absolute_path
        .strip_prefix(&session.source.source_root)
        .map(Utf8Path::to_path_buf)
        .unwrap_or_else(|_| Utf8PathBuf::from(import_path));
    let next_id = session
        .source
        .referenced_assets
        .iter()
        .map(|asset| asset.id.0)
        .max()
        .unwrap_or(0)
        .saturating_add(1);
    let id = AssetId(next_id);
    session.source.referenced_assets.push(ReferencedAsset {
        id: id.clone(),
        relative_path,
        absolute_path,
    });
    Ok(id)
}

fn fixture_definition_mut<'a>(
    session: &'a mut ProjectSession,
    document_path: &Utf8Path,
    object_key: &str,
) -> Result<&'a mut dawn_language::setup::FixtureDefinition, GuiMutationError> {
    let id = session
        .source
        .source_map
        .objects
        .iter()
        .find_map(|(id, location)| {
            (id.kind == SourceObjectKind::FixtureDefinition
                && location.document == document_path
                && location.object_key == object_key)
                .then(|| FixtureDefinitionId(id.id.clone()))
        })
        .ok_or_else(|| {
            GuiMutationError::Invalid("Fixture definition was not found.".to_string())
        })?;
    session
        .project
        .definitions
        .fixtures
        .definitions
        .get_mut(&id)
        .ok_or_else(|| GuiMutationError::Invalid("Fixture definition was not loaded.".to_string()))
}

fn effect_mut(
    sequence: &mut dawn_language::sequence::Sequence,
    id: u32,
) -> Result<&mut EffectInst, GuiMutationError> {
    sequence
        .effects
        .iter_mut()
        .find(|effect| effect.id.0 == id)
        .ok_or_else(|| GuiMutationError::Invalid("Effect was not found.".to_string()))
}

fn clip_mut(
    sequence: &mut dawn_language::sequence::Sequence,
    id: u32,
) -> Result<&mut SequenceClip, GuiMutationError> {
    sequence
        .clips
        .iter_mut()
        .find(|clip| clip.id.0 == id)
        .ok_or_else(|| GuiMutationError::Invalid("Clip was not found.".to_string()))
}

fn composition_graph_node_mut<'a>(
    sequence: &'a mut dawn_language::sequence::Sequence,
    id: &CompositionGraphNodeId,
) -> Result<&'a mut CompositionGraphNode, GuiMutationError> {
    sequence
        .composition_graph
        .nodes
        .iter_mut()
        .find(|node| node.id == *id)
        .ok_or_else(|| GuiMutationError::Invalid("Graph node was not found.".to_string()))
}

fn parse_graph_node_id(value: &str) -> Result<CompositionGraphNodeId, GuiMutationError> {
    if let Some(id) = value.strip_prefix("node:") {
        return id
            .parse::<u32>()
            .map(CompositionGraphNodeId)
            .map_err(|_| GuiMutationError::Invalid("Invalid graph node id.".to_string()));
    }
    Err(GuiMutationError::Invalid(
        "Invalid graph node id.".to_string(),
    ))
}

fn ensure_graph_node_exists(
    sequence: &dawn_language::sequence::Sequence,
    node_id: &CompositionGraphNodeId,
) -> Result<(), GuiMutationError> {
    if sequence
        .composition_graph
        .nodes
        .iter()
        .any(|node| node.id == *node_id)
    {
        Ok(())
    } else {
        Err(GuiMutationError::Invalid(
            "Graph node was not found.".to_string(),
        ))
    }
}

fn graph_input_cardinality(
    kind: &CompositionGraphNodeKind,
    source_name: &str,
) -> Option<GraphPortCardinality> {
    match kind {
        CompositionGraphNodeKind::Layer { .. } => None,
        CompositionGraphNodeKind::Operator(operator) => {
            let GraphOperatorRef::Builtin(operator) = &operator.operator;
            operator
                .definition()
                .inputs
                .iter()
                .find(|port| port.source_name == source_name)
                .map(|port| port.cardinality.clone())
        }
        CompositionGraphNodeKind::Output => {
            (source_name == "input").then_some(GraphPortCardinality::Many)
        }
    }
}

fn next_composition_node_id(sequence: &dawn_language::sequence::Sequence) -> u32 {
    sequence
        .composition_graph
        .nodes
        .iter()
        .map(|node| node.id.0)
        .max()
        .unwrap_or(0)
        + 1
}

fn create_sequence_layer(
    session: &mut ProjectSession,
    sequence_id: &SequenceId,
    name: String,
    color: String,
    position: Option<(f64, f64)>,
    connect_to_output: bool,
) -> Result<(), GuiMutationError> {
    let sequence = sequence_mut(session, sequence_id)?;
    let next_layer_id = sequence
        .layers
        .iter()
        .map(|layer| layer.id.0)
        .max()
        .unwrap_or(0)
        + 1;
    let output_node_id = sequence
        .composition_graph
        .nodes
        .iter()
        .find(|node| matches!(node.kind, CompositionGraphNodeKind::Output))
        .map(|node| node.id.clone())
        .ok_or_else(|| {
            GuiMutationError::Invalid("Composition graph output was not found.".to_string())
        })?;
    let layer_node_id = CompositionGraphNodeId(next_composition_node_id(sequence));
    sequence
        .layers
        .push(dawn_language::sequence::SequenceLayer {
            id: SequenceLayerId(next_layer_id),
            name,
            color: parse_color(&color)?,
            enabled: true,
        });
    let (x, y) = position.unwrap_or((80.0, 120.0 + f64::from(next_layer_id) * 80.0));
    sequence.composition_graph.nodes.push(CompositionGraphNode {
        id: layer_node_id.clone(),
        position: GraphNodePosition { x, y },
        kind: CompositionGraphNodeKind::Layer {
            layer_id: SequenceLayerId(next_layer_id),
        },
    });
    if connect_to_output {
        sequence.composition_graph.edges.push(EffectGraphEdge {
            from: layer_node_id,
            from_port: GraphPortId("output".to_string()),
            to: output_node_id,
            to_port: GraphPortId("input".to_string()),
        });
    }
    Ok(())
}

fn graph_operator_from_gui(operator: &SequenceGraphOperator) -> GraphOperator {
    match operator {
        SequenceGraphOperator::Max => GraphOperator::Max,
        SequenceGraphOperator::Add => GraphOperator::Add,
        SequenceGraphOperator::Multiply => GraphOperator::Multiply,
        SequenceGraphOperator::IntensityModulate => GraphOperator::IntensityModulate,
        SequenceGraphOperator::Dim => GraphOperator::Dim,
        SequenceGraphOperator::Invert => GraphOperator::Invert,
        SequenceGraphOperator::Colorize => GraphOperator::Colorize,
        SequenceGraphOperator::Delay => GraphOperator::Delay,
        SequenceGraphOperator::Echo => GraphOperator::Echo,
    }
}

fn sync_sequence_effect_clips(sequence: &mut dawn_language::sequence::Sequence) {
    let mut clips = sequence
        .effects
        .iter()
        .map(|effect| SequenceClip {
            id: SequenceClipId(effect.id.0),
            start: effect.start.clone(),
            duration: effect.duration.clone(),
            target: effect.target.clone(),
            scope: effect.scope.clone(),
            kind: SequenceClipKind::Effect(EffectClip {
                definition: effect.definition.clone(),
                param_overrides: effect.param_overrides.clone(),
            }),
        })
        .collect::<Vec<_>>();
    clips.sort_by_key(|clip| clip.id.0);
    sequence.clips = clips;
    for automation_clip in &mut sequence.automation_clips {
        for binding in &mut automation_clip.bindings {
            if matches!(binding.target, AutomationTarget::EffectParam { .. }) {
                binding.target = AutomationTarget::EffectParam {
                    effect_id: EffectInstId(binding.effect_id.0),
                    param: binding.param.clone(),
                };
            }
        }
    }
}

fn mark_collection_mut<'a>(
    sequence: &'a mut dawn_language::sequence::Sequence,
    key: &str,
) -> Result<&'a mut MarkCollection, GuiMutationError> {
    sequence
        .mark_collections
        .iter_mut()
        .find(|collection| collection.key.name == key)
        .ok_or_else(|| GuiMutationError::Invalid("Mark collection was not found.".to_string()))
}

fn automation_clip_mut(
    sequence: &mut dawn_language::sequence::Sequence,
    id: u32,
) -> Result<&mut AutomationClip, GuiMutationError> {
    sequence
        .automation_clips
        .iter_mut()
        .find(|clip| clip.id.0 == id)
        .ok_or_else(|| GuiMutationError::Invalid("Automation clip was not found.".to_string()))
}

fn identifier(value: &str) -> Result<Identifier, GuiMutationError> {
    Identifier::new(value.to_string())
        .map_err(|_| GuiMutationError::Invalid(format!("Invalid identifier `{value}`.")))
}

fn effect_scope(scope: SequenceEffectScope) -> EffectScope {
    match scope {
        SequenceEffectScope::PerFixture => EffectScope::PerFixture,
        SequenceEffectScope::WholeTarget => EffectScope::WholeTarget,
    }
}

fn layout_target_to_effect_target(target: LayoutTarget) -> Result<EffectTarget, GuiMutationError> {
    let id = target
        .name
        .parse::<u32>()
        .map_err(|_| GuiMutationError::Invalid("Layout target id must be numeric.".to_string()))?;
    Ok(match target.kind {
        LayoutTargetKind::Fixture => EffectTarget::Fixture(FixtureInstanceId(id)),
        LayoutTargetKind::Group => EffectTarget::Group(FixtureGroupId(id)),
    })
}

fn effect_param_value_from_gui(
    value: SequenceEffectParamValue,
) -> Result<EffectParamValue, GuiMutationError> {
    Ok(match value {
        SequenceEffectParamValue::Int { value } => EffectParamValue::Int(value as i64),
        SequenceEffectParamValue::Float { value } => EffectParamValue::Float(value),
        SequenceEffectParamValue::Bool { value } => EffectParamValue::Bool(value),
        SequenceEffectParamValue::Color { value } => EffectParamValue::Color(parse_color(&value)?),
        SequenceEffectParamValue::Enum { value } => EffectParamValue::Enum(identifier(&value)?),
        SequenceEffectParamValue::Marks { key } => {
            EffectParamValue::Marks(MarkCollectionKey { name: key })
        }
        SequenceEffectParamValue::FloatCurve { points } => {
            EffectParamValue::Curve(CurveSource::Inline(float_curve(points)))
        }
        SequenceEffectParamValue::ColorCurve { points } => {
            EffectParamValue::Curve(CurveSource::Inline(color_curve(points)?))
        }
        SequenceEffectParamValue::IntArray { values } => EffectParamValue::Array(
            values
                .into_iter()
                .map(|value| EffectParamValue::Int(value as i64))
                .collect(),
        ),
        SequenceEffectParamValue::FloatArray { values } => {
            EffectParamValue::Array(values.into_iter().map(EffectParamValue::Float).collect())
        }
        SequenceEffectParamValue::BoolArray { values } => {
            EffectParamValue::Array(values.into_iter().map(EffectParamValue::Bool).collect())
        }
        SequenceEffectParamValue::ColorArray { values } => EffectParamValue::Array(
            values
                .into_iter()
                .map(|value| parse_color(&value).map(EffectParamValue::Color))
                .collect::<Result<Vec<_>, _>>()?,
        ),
        SequenceEffectParamValue::FloatCurveArray { values } => EffectParamValue::Array(
            values
                .into_iter()
                .map(|points| EffectParamValue::Curve(CurveSource::Inline(float_curve(points))))
                .collect(),
        ),
        SequenceEffectParamValue::ColorCurveArray { values } => EffectParamValue::Array(
            values
                .into_iter()
                .map(|points| color_curve(points).map(CurveSource::Inline))
                .map(|source| source.map(EffectParamValue::Curve))
                .collect::<Result<Vec<_>, _>>()?,
        ),
    })
}

fn automation_mapping_from_gui(
    mapping: SequenceAutomationMapping,
) -> Result<AutomationMapping, GuiMutationError> {
    Ok(match mapping {
        SequenceAutomationMapping::Float { min, max } => AutomationMapping::Float { min, max },
        SequenceAutomationMapping::Int { min, max } => AutomationMapping::Int {
            min: min.round() as i64,
            max: max.round() as i64,
        },
        SequenceAutomationMapping::Bool => AutomationMapping::Bool,
        SequenceAutomationMapping::Enum { values } => AutomationMapping::Enum {
            values: values
                .into_iter()
                .map(|value| identifier(&value))
                .collect::<Result<Vec<_>, _>>()?,
        },
        SequenceAutomationMapping::FloatCurve { min, max } => {
            AutomationMapping::FloatCurve { min, max }
        }
    })
}

fn automation_binding_value_at(
    clip: &AutomationClip,
    binding: &AutomationBinding,
    seconds: f64,
) -> Result<EffectParamValue, GuiMutationError> {
    let normalized = sample_gui_automation_clip(clip, seconds);
    Ok(match &binding.mapping {
        AutomationMapping::Float { min, max } => {
            EffectParamValue::Float(lerp(*min, *max, normalized))
        }
        AutomationMapping::Int { min, max } => {
            EffectParamValue::Int(lerp(*min as f64, *max as f64, normalized).round() as i64)
        }
        AutomationMapping::Bool => EffectParamValue::Bool(normalized >= 0.5),
        AutomationMapping::Enum { values } => {
            if values.is_empty() {
                return Err(GuiMutationError::Invalid(
                    "Enum automation mapping has no values.".to_string(),
                ));
            }
            let index = ((normalized.clamp(0.0, 1.0) * values.len() as f64).floor() as usize)
                .min(values.len().saturating_sub(1));
            EffectParamValue::Enum(values[index].clone())
        }
        AutomationMapping::FloatCurve { min, max } => {
            EffectParamValue::Curve(CurveSource::Inline(Curve {
                points: vec![CurvePoint {
                    position: 0.0,
                    value: CurveValue::Float(lerp(*min, *max, normalized)),
                }],
            }))
        }
    })
}

fn sample_gui_automation_clip(clip: &AutomationClip, seconds: f64) -> f64 {
    let start_seconds = clip.start.as_seconds_f64();
    let duration_seconds = clip.duration.as_seconds_f64();
    let position = if duration_seconds <= 0.0 {
        0.0
    } else {
        ((seconds - start_seconds) / duration_seconds).clamp(0.0, 1.0)
    };
    sample_gui_float_curve(&clip.curve, position).clamp(0.0, 1.0)
}

fn sample_gui_float_curve(curve: &Curve, position: f64) -> f64 {
    let mut points = curve.points.iter().collect::<Vec<_>>();
    points.sort_by(|left, right| left.position.total_cmp(&right.position));
    let Some(first) = points.first() else {
        return 0.0;
    };
    if position <= first.position {
        return gui_curve_point_float(first);
    }
    for pair in points.windows(2) {
        let left = pair[0];
        let right = pair[1];
        if position <= right.position {
            let span = right.position - left.position;
            let amount = if span <= 0.0 {
                0.0
            } else {
                (position - left.position) / span
            };
            return lerp(
                gui_curve_point_float(left),
                gui_curve_point_float(right),
                amount,
            );
        }
    }
    points
        .last()
        .map(|point| gui_curve_point_float(point))
        .unwrap_or(0.0)
}

fn gui_curve_point_float(point: &CurvePoint) -> f64 {
    match point.value {
        CurveValue::Float(value) => value,
        CurveValue::Color(_) => 0.0,
    }
}

fn default_automation_curve() -> Curve {
    Curve {
        points: vec![
            CurvePoint {
                position: 0.0,
                value: CurveValue::Float(0.0),
            },
            CurvePoint {
                position: 1.0,
                value: CurveValue::Float(1.0),
            },
        ],
    }
}

fn float_curve(points: Vec<FloatCurvePoint>) -> Curve {
    Curve {
        points: points
            .into_iter()
            .map(|point| CurvePoint {
                position: point.time,
                value: CurveValue::Float(point.value),
            })
            .collect(),
    }
}

fn color_curve(points: Vec<ColorCurvePoint>) -> Result<Curve, GuiMutationError> {
    Ok(Curve {
        points: points
            .into_iter()
            .map(|point| {
                Ok(CurvePoint {
                    position: point.time,
                    value: CurveValue::Color(parse_color(&point.value)?),
                })
            })
            .collect::<Result<Vec<_>, GuiMutationError>>()?,
    })
}

fn parse_color(value: &str) -> Result<Color, GuiMutationError> {
    if value.len() != 7 || !value.starts_with('#') {
        return Err(GuiMutationError::Invalid(format!(
            "Invalid color `{value}`."
        )));
    }
    Ok(Color {
        red: u8::from_str_radix(&value[1..3], 16)
            .map_err(|_| GuiMutationError::Invalid(format!("Invalid color `{value}`.")))?,
        green: u8::from_str_radix(&value[3..5], 16)
            .map_err(|_| GuiMutationError::Invalid(format!("Invalid color `{value}`.")))?,
        blue: u8::from_str_radix(&value[5..7], 16)
            .map_err(|_| GuiMutationError::Invalid(format!("Invalid color `{value}`.")))?,
    })
}

fn domain_point3_meters(point: Point3Meters) -> Point3 {
    Point3 {
        x: distance(point.x_meters),
        y: distance(point.y_meters),
        z: distance(point.z_meters),
    }
}

fn rotation3_degrees(rotation: Rotation3Degrees) -> DomainRotation3 {
    DomainRotation3 {
        x: rotation.x_degrees,
        y: rotation.y_degrees,
        z: rotation.z_degrees,
    }
}

fn scale3(scale: Scale3) -> DomainScale3 {
    DomainScale3 {
        x: scale.x,
        y: scale.y,
        z: scale.z,
    }
}

fn dawn_time(seconds: f64) -> DawnTime {
    DawnTime(Duration::from_secs_f64(seconds))
}

fn dawn_duration(seconds: f64) -> DawnDuration {
    DawnDuration(Duration::from_secs_f64(seconds))
}

fn distance(value: f64) -> Distance {
    Distance {
        micrometers: (value * 1_000_000.0).round() as i64,
    }
}

fn distance_span(value: f64) -> DistanceSpan {
    DistanceSpan {
        micrometers: (value * 1_000_000.0).round() as u64,
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use camino::Utf8PathBuf;
    use dawn_language::model::{DawnProject, ProjectDefinitionStores, ProjectId, ProjectRoot};
    use dawn_language::sequence::{
        CompositionGraphNode, CompositionGraphNodeId, CompositionGraphNodeKind, EffectGraphEdge,
        GraphNodePosition, GraphOperator, GraphOperatorNode, GraphOperatorRef, GraphPortId,
        Sequence, SequenceAudio, SequenceCompositionGraph, SequenceId, SequenceLayer,
        SequenceLayerId,
    };
    use dawn_language::setup::{LayoutId, PatchId, Setup, SetupId};
    use dawn_language::values::{Color, DawnDuration};
    use dawn_project_io::{
        ProjectSession, SourceMap, SourceObjectId, SourceObjectKind, SourceObjectLocation,
        SourceProject,
    };
    use indexmap::IndexMap;

    use super::apply_edit;
    use crate::dto::{DocumentViewId, GuiDocumentRequest, GuiEditCommand, SequenceGuiEdit};

    #[test]
    fn create_layer_adds_layer_to_output_edge() {
        let mut session = test_session(test_sequence_with_graph(true));

        apply_sequence_edit(
            &mut session,
            SequenceGuiEdit::CreateLayer {
                name: "Front".to_string(),
                color: "#123456".to_string(),
            },
        )
        .unwrap();

        let sequence = test_sequence(&session);
        assert!(
            sequence
                .layers
                .iter()
                .any(|layer| layer.id == SequenceLayerId(1) && layer.name == "Front")
        );
        assert!(sequence.composition_graph.edges.iter().any(|edge| {
            edge.from == CompositionGraphNodeId(3)
                && edge.from_port.0 == "output"
                && edge.to == CompositionGraphNodeId(2)
                && edge.to_port.0 == "input"
        }));
    }

    #[test]
    fn create_layer_errors_when_output_node_is_missing() {
        let mut session = test_session(test_sequence_with_graph(false));

        let error = apply_sequence_edit(
            &mut session,
            SequenceGuiEdit::CreateLayer {
                name: "Front".to_string(),
                color: "#123456".to_string(),
            },
        )
        .unwrap_err();

        assert_eq!(error.message(), "Composition graph output was not found.");
        assert_eq!(test_sequence(&session).layers.len(), 1);
        assert!(test_sequence(&session).composition_graph.edges.is_empty());
    }

    #[test]
    fn disconnect_graph_nodes_removes_only_matching_edge() {
        let mut sequence = test_sequence_with_graph(true);
        sequence.composition_graph.nodes.push(operator_node(3));
        sequence.composition_graph.edges.push(graph_edge(
            CompositionGraphNodeId(1),
            "output",
            CompositionGraphNodeId(3),
            "input",
        ));
        sequence.composition_graph.edges.push(graph_edge(
            CompositionGraphNodeId(3),
            "output",
            CompositionGraphNodeId(2),
            "input",
        ));
        let mut session = test_session(sequence);

        apply_sequence_edit(
            &mut session,
            SequenceGuiEdit::DisconnectGraphNodes {
                from_node: "node:1".to_string(),
                from_port: "output".to_string(),
                to_node: "node:2".to_string(),
                to_port: "input".to_string(),
            },
        )
        .unwrap();

        let edges = &test_sequence(&session).composition_graph.edges;
        assert_eq!(edges.len(), 2);
        assert!(!edges.iter().any(|edge| {
            edge.from == CompositionGraphNodeId(1) && edge.to == CompositionGraphNodeId(2)
        }));
        assert!(edges.iter().any(|edge| {
            edge.from == CompositionGraphNodeId(1) && edge.to == CompositionGraphNodeId(3)
        }));
        assert!(edges.iter().any(|edge| {
            edge.from == CompositionGraphNodeId(3) && edge.to == CompositionGraphNodeId(2)
        }));
    }

    #[test]
    fn delete_graph_node_removes_operator_edges_but_rejects_layer_and_output_nodes() {
        let mut sequence = test_sequence_with_graph(true);
        sequence.composition_graph.nodes.push(operator_node(3));
        sequence.composition_graph.edges.push(graph_edge(
            CompositionGraphNodeId(1),
            "output",
            CompositionGraphNodeId(3),
            "input",
        ));
        sequence.composition_graph.edges.push(graph_edge(
            CompositionGraphNodeId(3),
            "output",
            CompositionGraphNodeId(2),
            "input",
        ));
        let mut session = test_session(sequence);

        apply_sequence_edit(
            &mut session,
            SequenceGuiEdit::DeleteGraphNode {
                node_id: "node:1".to_string(),
            },
        )
        .unwrap_err();
        apply_sequence_edit(
            &mut session,
            SequenceGuiEdit::DeleteGraphNode {
                node_id: "node:2".to_string(),
            },
        )
        .unwrap_err();
        apply_sequence_edit(
            &mut session,
            SequenceGuiEdit::DeleteGraphNode {
                node_id: "node:3".to_string(),
            },
        )
        .unwrap();

        let sequence = test_sequence(&session);
        assert!(
            sequence
                .composition_graph
                .nodes
                .iter()
                .all(|node| node.id != CompositionGraphNodeId(3))
        );
        assert!(sequence.composition_graph.nodes.iter().any(|node| {
            node.id == CompositionGraphNodeId(2)
                && matches!(node.kind, CompositionGraphNodeKind::Output)
        }));
        assert!(sequence.composition_graph.edges.iter().all(|edge| {
            edge.from != CompositionGraphNodeId(3) && edge.to != CompositionGraphNodeId(3)
        }));
    }

    fn apply_sequence_edit(
        session: &mut ProjectSession,
        edit: SequenceGuiEdit,
    ) -> Result<(), super::GuiMutationError> {
        apply_edit(
            session,
            &GuiDocumentRequest {
                path: "sequences.dawn".to_string(),
                view: DocumentViewId::Sequence,
                object_key: Some("seq".to_string()),
            },
            GuiEditCommand::Sequence { edit },
        )
    }

    fn test_session(sequence: Sequence) -> ProjectSession {
        ProjectSession {
            project: DawnProject {
                root: ProjectRoot {
                    id: ProjectId("project".to_string()),
                    setup: SetupId("setup".to_string()),
                    sequences: vec![SequenceId("seq".to_string())],
                },
                setups: IndexMap::from([(
                    SetupId("setup".to_string()),
                    Setup {
                        id: SetupId("setup".to_string()),
                        layout: LayoutId("layout".to_string()),
                        patch: PatchId("patch".to_string()),
                        controllers: Vec::new(),
                    },
                )]),
                layouts: IndexMap::new(),
                patches: IndexMap::new(),
                controllers: IndexMap::new(),
                sequences: IndexMap::from([(SequenceId("seq".to_string()), sequence)]),
                definitions: ProjectDefinitionStores::default(),
            },
            source: SourceProject {
                source_root: Utf8PathBuf::from("."),
                entrypoint: Utf8PathBuf::from("project.dawn"),
                documents: IndexMap::new(),
                import_graph: IndexMap::new(),
                source_map: SourceMap {
                    objects: IndexMap::from([(
                        SourceObjectId {
                            kind: SourceObjectKind::Sequence,
                            id: "seq".to_string(),
                        },
                        SourceObjectLocation {
                            document: Utf8PathBuf::from("sequences.dawn"),
                            object_key: "seq".to_string(),
                        },
                    )]),
                },
                effect_source_text: IndexMap::new(),
                referenced_assets: Vec::new(),
            },
        }
    }

    fn test_sequence(session: &ProjectSession) -> &Sequence {
        session
            .project
            .sequences
            .get(&SequenceId("seq".to_string()))
            .unwrap()
    }

    fn test_sequence_with_graph(include_output: bool) -> Sequence {
        Sequence {
            id: SequenceId("seq".to_string()),
            duration: DawnDuration(Duration::from_secs(1)),
            frame_rate: 30,
            audio: SequenceAudio::None,
            mark_collections: Vec::new(),
            clips: Vec::new(),
            layers: vec![SequenceLayer {
                id: SequenceLayerId(0),
                name: "Default".to_string(),
                color: Color {
                    red: 80,
                    green: 160,
                    blue: 255,
                },
                enabled: true,
            }],
            effects: Vec::new(),
            composition_graph: SequenceCompositionGraph {
                nodes: if include_output {
                    vec![
                        layer_node(1, 0),
                        CompositionGraphNode {
                            id: CompositionGraphNodeId(2),
                            position: GraphNodePosition { x: 240.0, y: 0.0 },
                            kind: CompositionGraphNodeKind::Output,
                        },
                    ]
                } else {
                    vec![layer_node(1, 0)]
                },
                edges: if include_output {
                    vec![graph_edge(
                        CompositionGraphNodeId(1),
                        "output",
                        CompositionGraphNodeId(2),
                        "input",
                    )]
                } else {
                    Vec::new()
                },
            },
            automation_clips: Vec::new(),
        }
    }

    fn operator_node(id: u32) -> CompositionGraphNode {
        CompositionGraphNode {
            id: CompositionGraphNodeId(id),
            position: GraphNodePosition { x: 120.0, y: 0.0 },
            kind: CompositionGraphNodeKind::Operator(GraphOperatorNode {
                operator: GraphOperatorRef::Builtin(GraphOperator::Dim),
                params: IndexMap::new(),
            }),
        }
    }

    fn layer_node(id: u32, layer_id: u32) -> CompositionGraphNode {
        CompositionGraphNode {
            id: CompositionGraphNodeId(id),
            position: GraphNodePosition { x: 0.0, y: 0.0 },
            kind: CompositionGraphNodeKind::Layer {
                layer_id: SequenceLayerId(layer_id),
            },
        }
    }

    fn graph_edge(
        from: CompositionGraphNodeId,
        from_port: &str,
        to: CompositionGraphNodeId,
        to_port: &str,
    ) -> EffectGraphEdge {
        EffectGraphEdge {
            from,
            from_port: GraphPortId(from_port.to_string()),
            to,
            to_port: GraphPortId(to_port.to_string()),
        }
    }
}
