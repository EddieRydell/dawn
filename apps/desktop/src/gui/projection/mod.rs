mod params;
use params::{
    automation_mapping_to_gui, curve_library, effect_params, fixture_source_ref, gradient_library,
    graph_node_id, graph_operator_definition_to_gui, param_kind, sequence_composition_graph_node,
};
pub(super) use params::{default_param_value, effect_param_value};

pub(super) fn project_sequence(
    session: &ProjectSession,
    resolved: &ResolvedGuiObject,
) -> GuiDocument {
    let id = SequenceId(SourceIdentity::new(
        resolved.identity.document().to_path_buf(),
        resolved.identity.object().to_string(),
    ));
    let Some(sequence) = session.project.sequences.get(&id) else {
        return blocked(
            "Sequence is not available in the checked project model.",
            vec![gui_diagnostic(
                resolved.identity.document().as_ref(),
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
            script: effect.definition.0.object().to_string(),
            script_source: Some(effect_script_ref(&effect.definition.0)),
            params: effect_params(session, sequence, effect),
            kind: SequenceTimelineClipKind::Effect,
        })
        .collect();
    let composition_graph = SequenceCompositionGraph {
        id: 0,
        operator_catalog: BuiltinOperator::ALL
            .iter()
            .map(|builtin| {
                graph_operator_definition_to_gui(
                    OperatorRef::Builtin(builtin.clone()),
                    builtin.definition(),
                )
            })
            .chain(
                session
                    .project
                    .definitions
                    .operators
                    .definitions
                    .iter()
                    .map(|(id, definition)| {
                        graph_operator_definition_to_gui(
                            OperatorRef::Custom(id.clone()),
                            definition,
                        )
                    }),
            )
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
            path: resolved.identity.document().to_string(),
            source_ref: resolved.source_ref(),
            object_key: resolved.identity.object().to_string(),
            duration_seconds: sequence.duration.as_seconds_f64(),
            frame_rate: sequence.frame_rate as f64,
            audio: sequence_audio(session, resolved.identity.document(), &sequence.audio),
            mark_collections: sequence
                .mark_collections
                .iter()
                .map(|collection| SequenceMarkCollection {
                    key: collection.key.name.clone(),
                    name: collection.name.clone(),
                    color: collection.display_color.to_hex(),
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
            gradient_library: gradient_library(session),
            layers: sequence
                .layers
                .iter()
                .map(|layer| SequenceLayer {
                    id: layer.id.0,
                    name: layer.name.clone(),
                    color: layer.color.to_hex(),
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
                .map(|point| SequenceCurvePoint {
                    time: point.position,
                    value: point.value,
                })
                .collect(),
            bindings: clip
                .bindings
                .iter()
                .map(|binding| SequenceAutomationBinding {
                    target: match &binding.target {
                        AutomationTarget::EffectParam { effect_id, param } => {
                            SequenceAutomationTarget::EffectParam {
                                effect_id: effect_id.0,
                                param: param.as_str().to_string(),
                            }
                        }
                        AutomationTarget::CompositionNodeParam { node_id, param } => {
                            SequenceAutomationTarget::CompositionNodeParam {
                                node_id: graph_node_id(node_id),
                                param: param.as_str().to_string(),
                            }
                        }
                    },
                    mapping: automation_mapping_to_gui(&binding.mapping),
                })
                .collect(),
        })
        .collect()
}

pub(super) fn project_layout(
    session: &ProjectSession,
    resolved: &ResolvedGuiObject,
) -> GuiDocument {
    let id = LayoutId(SourceIdentity::new(
        resolved.identity.document().to_path_buf(),
        resolved.identity.object().to_string(),
    ));
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
            let definition_ref = fixture_source_ref(&fixture.definition);
            let resolved_fixture = session
                .project
                .definitions
                .fixtures
                .get(&fixture.definition)
                .map(|definition| ResolvedLayoutFixture {
                    name: fixture.definition.0.object().to_string(),
                    color_model: "rgb".to_string(),
                    bulb_diameter_meters: definition.bulb_radius.as_meters_f64() * 2.0,
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
                    path: resolved.identity.document().to_string(),
                    object_key: resolved.identity.object().to_string(),
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
            path: resolved.identity.document().to_string(),
            source_ref: resolved.source_ref(),
            object_key: resolved.identity.object().to_string(),
            name: resolved.identity.object().to_string(),
            render_bounds,
            fixtures,
        },
    }
}

pub(super) fn project_fixture(
    session: &ProjectSession,
    resolved: &ResolvedGuiObject,
) -> GuiDocument {
    let fixtures = session
        .source
        .documents
        .get(resolved.identity.document())
        .into_iter()
        .flat_map(|document| document.objects())
        .filter(|object| object.kind() == &SourceObjectKind::FixtureDefinition)
        .filter_map(|object| {
            let definition_id = FixtureDefinitionId(SourceIdentity::new(
                resolved.identity.document().to_path_buf(),
                object.id().to_string(),
            ));
            let definition = session.project.definitions.fixtures.get(&definition_id)?;
            let source_ref = GuiObjectRef {
                path: resolved.identity.document().to_string(),
                object_key: object.id().to_string(),
                kind: ObjectKind::Fixture,
                id: object.id().to_string(),
            };
            Some(FixtureDefinition {
                source_ref,
                object_key: object.id().to_string(),
                name: object.id().to_string(),
                color_model: "rgb".to_string(),
                bulb_diameter_meters: definition.bulb_radius.as_meters_f64() * 2.0,
                geometry: geometry(&definition.geometry),
                geometry_summary: geometry_summary(&definition.geometry),
                render_plan: render_plan(&definition.geometry, definition.bulb_radius),
            })
        })
        .collect::<Vec<_>>();
    GuiDocument::Fixture {
        document: FixtureGuiDocument {
            path: resolved.identity.document().to_string(),
            source_ref: Some(resolved.source_ref()),
            selected_object_key: Some(resolved.identity.object().to_string()),
            fixtures,
        },
    }
}

pub(super) fn active_layout_id(session: &ProjectSession) -> Option<LayoutId> {
    session
        .project
        .setups
        .get(&session.project.root.setup)
        .map(|setup| setup.layout.clone())
}

fn sequence_audio(
    session: &ProjectSession,
    document: &Utf8Path,
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
            import_path: relative_path_from_document(document, &asset.relative_path).to_string(),
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

fn effect_script_ref(id: &SourceIdentity) -> EffectScriptReference {
    EffectScriptReference {
        path: id.document().to_string(),
        effect_name: id.object().to_string(),
    }
}

fn effect_scripts(session: &ProjectSession) -> Vec<SequenceEffectScript> {
    session
        .project
        .definitions
        .effects
        .definitions
        .iter()
        .map(|(id, definition)| {
            let source = effect_script_ref(&id.0);
            SequenceEffectScript {
                name: id.0.object().to_string(),
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
            }
        })
        .collect()
}
use camino::Utf8Path;
use dawn_language::dsl::EffectKind;
use dawn_language::effect::{EffectScope, EffectTarget};
use dawn_language::identity::SourceIdentity;
use dawn_language::operator::{BuiltinOperator, OperatorRef};
use dawn_language::sequence::{AutomationTarget, SequenceId};
use dawn_language::setup::{FixtureDefinitionId, LayoutId, LayoutTarget as DomainLayoutTarget};
use dawn_project_io::{ProjectSession, SourceObjectKind, relative_path_from_document};

use super::{ResolvedGuiObject, blocked, gui_diagnostic};
use crate::dto::{
    EffectScriptReference, FixtureDefinition, FixtureGuiDocument, GuiDocument, GuiObjectRef,
    LayoutFixturePlacement, LayoutGuiDocument, LayoutTarget, LayoutTargetKind, ObjectKind,
    ResolvedLayoutFixture, Rotation3Degrees, Scale3, SequenceAudio, SequenceAutomationBinding,
    SequenceAutomationClip, SequenceAutomationTarget, SequenceCompositionGraph, SequenceCurvePoint,
    SequenceEffect, SequenceEffectScope, SequenceEffectScript, SequenceEffectScriptKind,
    SequenceEffectScriptParam, SequenceGraphEdge, SequenceGuiDocument, SequenceLane, SequenceLayer,
    SequenceMarkCollection, SequenceTimelineClipKind, Transform,
};
use crate::gui_geometry::{
    empty_resolved_fixture, geometry, geometry_summary, layout_bounds, point3_meters, render_plan,
};
