use std::collections::HashMap;

use dawn_language::controller::{ControllerId, ControllerPortAddress, ControllerProtocol};
use dawn_language::element::{ElementCellAddress, ElementNodeId, ElementNodeKind};
use dawn_language::patch::{PatchEdge, PatchNode, PatchNodeId, PatchPortId};
use dawn_language::preview::PropInstanceId;
use dawn_language::validation::validate_project;
use dawn_project_io::ProjectSession;

use super::model::source_identity_from_gui;
use super::{GuiMutationError, ResolvedGuiObject, blocked};
use crate::dto::{
    GuiDocument, GuiObjectRef, ObjectKind, SetupController, SetupControllerPort, SetupElementCell,
    SetupElementKind, SetupElementNode, SetupFixtureProfile, SetupGuiDocument, SetupGuiEdit,
    SetupPatchEdge, SetupPatchNode, SetupPatchNodeKind, SetupPreviewLink,
};

pub(super) fn project_setup(session: &ProjectSession, resolved: &ResolvedGuiObject) -> GuiDocument {
    let Some(setup) = session.project.setups.get(&session.project.root.setup) else {
        return blocked("Active setup is missing.", Vec::new());
    };
    let Some(tree) = session.project.element_trees.get(&setup.elements) else {
        return blocked("Active element tree is missing.", Vec::new());
    };
    let parents = tree
        .nodes
        .iter()
        .flat_map(|(parent, node)| match &node.kind {
            ElementNodeKind::Group { children } => children
                .iter()
                .map(|child| (*child, *parent))
                .collect::<Vec<_>>(),
            _ => Vec::new(),
        })
        .collect::<HashMap<_, _>>();
    let elements = tree
        .nodes
        .iter()
        .map(|(id, node)| {
            let (kind, children, capability, profile) = match &node.kind {
                ElementNodeKind::Group { children } => (
                    SetupElementKind::Group,
                    children.iter().map(|id| id.0).collect(),
                    None,
                    None,
                ),
                ElementNodeKind::Color { capability, .. } => (
                    SetupElementKind::Color,
                    Vec::new(),
                    Some(format!("{capability:?}")),
                    None,
                ),
                ElementNodeKind::Scalar { .. } => {
                    (SetupElementKind::Scalar, Vec::new(), None, None)
                }
                ElementNodeKind::Indexed { .. } => {
                    (SetupElementKind::Indexed, Vec::new(), None, None)
                }
                ElementNodeKind::Fixture { profile } => (
                    SetupElementKind::Fixture,
                    Vec::new(),
                    None,
                    Some(source_key(&profile.0)),
                ),
            };
            SetupElementNode {
                id: id.0,
                name: node.name.clone(),
                kind,
                parent: parents.get(id).map(|id| id.0),
                children,
                cell_count: node.kind.cell_count(),
                capability,
                profile,
            }
        })
        .collect();
    let fixture_profiles = session
        .project
        .definitions
        .fixture_profiles
        .definitions
        .iter()
        .map(|(id, profile)| SetupFixtureProfile {
            id: source_key(&id.0),
            name: id.0.object().to_string(),
            function_count: profile.functions.len() as u32,
            channel_count: profile.channels.len() as u32,
            behavior_rule_count: profile.behavior_rules.len() as u32,
        })
        .collect();
    let preview_links = session
        .project
        .preview_layouts
        .get(&setup.preview)
        .map(|preview| {
            preview
                .props
                .iter()
                .map(|prop| SetupPreviewLink {
                    prop_id: prop.id.0,
                    name: prop.name.clone(),
                    point_count: session
                        .project
                        .definitions
                        .props
                        .definitions
                        .get(&prop.definition)
                        .map_or(0, |definition| definition.geometry.point_count() as u32),
                    bindings: prop
                        .bindings
                        .iter()
                        .map(|binding| SetupElementCell {
                            node: binding.node.0,
                            cell: binding.cell,
                        })
                        .collect(),
                })
                .collect()
        })
        .unwrap_or_default();
    let (patch_nodes, patch_edges) = session
        .project
        .patches
        .get(&setup.patch)
        .map(|patch| {
            let nodes = patch
                .nodes
                .iter()
                .map(|(id, node)| {
                    let (kind, label, width) = match node {
                        PatchNode::Source(source) => (
                            SetupPatchNodeKind::Source,
                            format!("Element {}", source.selection.node.0),
                            source.output.width(),
                        ),
                        PatchNode::Filter(filter) => (
                            SetupPatchNodeKind::Filter,
                            format!("{filter:?}"),
                            filter_width(filter),
                        ),
                        PatchNode::Sink(sink) => (
                            SetupPatchNodeKind::Sink,
                            format!(
                                "{} / port {} / slots {}-{}",
                                sink.controller.0.object(),
                                sink.port.0,
                                sink.start_slot,
                                u32::from(sink.start_slot) + u32::from(sink.slot_count)
                            ),
                            usize::from(sink.slot_count),
                        ),
                    };
                    SetupPatchNode {
                        id: id.0,
                        kind,
                        label,
                        width: width as u32,
                    }
                })
                .collect();
            let edges = patch
                .edges
                .iter()
                .map(|edge| SetupPatchEdge {
                    from_node: edge.from.0,
                    from_port: edge.from_port.0,
                    to_node: edge.to.0,
                    to_port: edge.to_port.0,
                })
                .collect();
            (nodes, edges)
        })
        .unwrap_or_default();
    let controllers = setup
        .controllers
        .iter()
        .filter_map(|id| {
            let controller = session.project.controllers.get(id)?;
            let (protocol, bind_address, destination, mode, priority, source_name) =
                match &controller.protocol {
                    ControllerProtocol::E131(config) => (
                        "E1.31".to_string(),
                        config.bind_address.to_string(),
                        match &config.mode {
                            dawn_language::controller::E131Mode::Multicast => None,
                            dawn_language::controller::E131Mode::Unicast { destination } => {
                                Some(destination.to_string())
                            }
                        },
                        match config.mode {
                            dawn_language::controller::E131Mode::Multicast => "multicast",
                            dawn_language::controller::E131Mode::Unicast { .. } => "unicast",
                        }
                        .to_string(),
                        Some(config.priority),
                        Some(config.source_name.clone()),
                    ),
                    ControllerProtocol::ArtNet(config) => (
                        "Art-Net".to_string(),
                        config.bind_address.to_string(),
                        Some(config.destination.to_string()),
                        format!("{:?}", config.mode).to_lowercase(),
                        None,
                        None,
                    ),
                };
            Some(SetupController {
                label: source_key(&id.0),
                source_ref: GuiObjectRef {
                    module_id: id.0.module_id().to_string(),
                    path: id.0.document().to_string(),
                    object_key: id.0.object().to_string(),
                    kind: ObjectKind::Controller,
                    id: id.0.object().to_string(),
                },
                read_only: !session.source.is_project_owned(id.0.document_id()),
                protocol,
                bind_address,
                destination,
                mode,
                priority,
                source_name,
                ports: controller
                    .ports
                    .iter()
                    .map(|port| SetupControllerPort {
                        id: port.id.0,
                        address: match port.address {
                            ControllerPortAddress::E131Universe(address)
                            | ControllerPortAddress::ArtNetPort(address) => address,
                        },
                        slot_count: port.slot_count,
                    })
                    .collect(),
            })
        })
        .collect();
    GuiDocument::Setup {
        document: SetupGuiDocument {
            path: resolved.identity.document().to_string(),
            source_ref: resolved.source_ref(),
            object_key: resolved.identity.object().to_string(),
            elements,
            fixture_profiles,
            preview_links,
            patch_nodes,
            patch_edges,
            controllers,
        },
    }
}

pub(super) fn edit_setup(
    session: &mut ProjectSession,
    edit: SetupGuiEdit,
) -> Result<(), GuiMutationError> {
    let setup = session
        .project
        .setups
        .get(&session.project.root.setup)
        .cloned()
        .ok_or_else(|| GuiMutationError::Invalid("Active setup is missing.".to_string()))?;
    match edit {
        SetupGuiEdit::RenameElement { id, name } => {
            if name.trim().is_empty() {
                return Err(GuiMutationError::Invalid(
                    "Element name cannot be empty.".to_string(),
                ));
            }
            tree_mut(session, &setup.elements)?
                .nodes
                .get_mut(&ElementNodeId(id))
                .ok_or_else(|| GuiMutationError::Invalid("Element was not found.".to_string()))?
                .name = name;
        }
        SetupGuiEdit::SetElementCellCount { id, cells } => {
            if cells == 0 {
                return Err(GuiMutationError::Invalid(
                    "Element cell count must be positive.".to_string(),
                ));
            }
            let node = tree_mut(session, &setup.elements)?
                .nodes
                .get_mut(&ElementNodeId(id))
                .ok_or_else(|| GuiMutationError::Invalid("Element was not found.".to_string()))?;
            match &mut node.kind {
                ElementNodeKind::Color { cells: value, .. }
                | ElementNodeKind::Scalar { cells: value }
                | ElementNodeKind::Indexed { cells: value, .. } => *value = cells,
                _ => {
                    return Err(GuiMutationError::Invalid(
                        "This element does not have an editable cell count.".to_string(),
                    ));
                }
            }
        }
        SetupGuiEdit::ReorderElements {
            parent,
            ordered_ids,
        } => {
            let ordered = ordered_ids
                .into_iter()
                .map(ElementNodeId)
                .collect::<Vec<_>>();
            let tree = tree_mut(session, &setup.elements)?;
            if ordered.iter().any(|id| !tree.nodes.contains_key(id)) {
                return Err(GuiMutationError::Invalid(
                    "Element order references a missing node.".to_string(),
                ));
            }
            if let Some(parent) = parent {
                let node = tree.nodes.get_mut(&ElementNodeId(parent)).ok_or_else(|| {
                    GuiMutationError::Invalid("Parent element was not found.".to_string())
                })?;
                let ElementNodeKind::Group { children } = &mut node.kind else {
                    return Err(GuiMutationError::Invalid(
                        "Parent element is not a group.".to_string(),
                    ));
                };
                if children.len() != ordered.len()
                    || !children.iter().all(|child| ordered.contains(child))
                {
                    return Err(GuiMutationError::Invalid(
                        "Reorder must contain exactly the group's current children.".to_string(),
                    ));
                }
                *children = ordered;
            } else {
                if tree.roots.len() != ordered.len()
                    || !tree.roots.iter().all(|root| ordered.contains(root))
                {
                    return Err(GuiMutationError::Invalid(
                        "Reorder must contain exactly the current roots.".to_string(),
                    ));
                }
                tree.roots = ordered;
            }
        }
        SetupGuiEdit::SetPreviewBindings { prop_id, bindings } => {
            let prop = preview_prop_mut(session, &setup.preview, prop_id)?;
            prop.bindings = bindings
                .into_iter()
                .map(|binding| ElementCellAddress {
                    node: ElementNodeId(binding.node),
                    cell: binding.cell,
                })
                .collect();
        }
        SetupGuiEdit::AutoLinkPreview {
            prop_id,
            node,
            start_cell,
        } => {
            let point_count = {
                let preview = session
                    .project
                    .preview_layouts
                    .get(&setup.preview)
                    .ok_or_else(|| {
                        GuiMutationError::Invalid("Preview layout is missing.".to_string())
                    })?;
                let prop = preview
                    .props
                    .iter()
                    .find(|prop| prop.id == PropInstanceId(prop_id))
                    .ok_or_else(|| {
                        GuiMutationError::Invalid("Preview prop was not found.".to_string())
                    })?;
                session
                    .project
                    .definitions
                    .props
                    .definitions
                    .get(&prop.definition)
                    .map(|definition| definition.geometry.point_count() as u32)
                    .ok_or_else(|| {
                        GuiMutationError::Invalid("Preview prop definition is missing.".to_string())
                    })?
            };
            let tree = session
                .project
                .element_trees
                .get(&setup.elements)
                .ok_or_else(|| GuiMutationError::Invalid("Element tree is missing.".to_string()))?;
            let available = tree
                .nodes
                .get(&ElementNodeId(node))
                .and_then(|node| node.kind.cell_count())
                .ok_or_else(|| {
                    GuiMutationError::Invalid(
                        "Auto-link target must be a leaf element.".to_string(),
                    )
                })?;
            if start_cell
                .checked_add(point_count)
                .is_none_or(|end| end > available)
            {
                return Err(GuiMutationError::Invalid(
                    "Preview and requested element span counts do not match.".to_string(),
                ));
            }
            preview_prop_mut(session, &setup.preview, prop_id)?.bindings = (start_cell
                ..start_cell + point_count)
                .map(|cell| ElementCellAddress {
                    node: ElementNodeId(node),
                    cell,
                })
                .collect();
        }
        SetupGuiEdit::ConnectPatch {
            from_node,
            from_port,
            to_node,
            to_port,
        } => {
            let patch = session
                .project
                .patches
                .get_mut(&setup.patch)
                .ok_or_else(|| GuiMutationError::Invalid("Patch graph is missing.".to_string()))?;
            let edge = PatchEdge {
                from: PatchNodeId(from_node),
                from_port: PatchPortId(from_port),
                to: PatchNodeId(to_node),
                to_port: PatchPortId(to_port),
            };
            if !patch.edges.contains(&edge) {
                patch.edges.push(edge);
            }
        }
        SetupGuiEdit::DisconnectPatch {
            from_node,
            from_port,
            to_node,
            to_port,
        } => {
            let patch = session
                .project
                .patches
                .get_mut(&setup.patch)
                .ok_or_else(|| GuiMutationError::Invalid("Patch graph is missing.".to_string()))?;
            patch.edges.retain(|edge| {
                !(edge.from == PatchNodeId(from_node)
                    && edge.from_port == PatchPortId(from_port)
                    && edge.to == PatchNodeId(to_node)
                    && edge.to_port == PatchPortId(to_port))
            });
        }
        SetupGuiEdit::SetControllerPort {
            controller,
            port,
            address,
            slot_count,
        } => {
            if !matches!(&controller.kind, ObjectKind::Controller) {
                return Err(GuiMutationError::Invalid(
                    "Controller source kind is invalid.".to_string(),
                ));
            }
            let identity = source_identity_from_gui(
                &controller.module_id,
                &controller.path,
                &controller.object_key,
            )?;
            if !session.source.is_project_owned(identity.document_id()) {
                return Err(GuiMutationError::Blocked(
                    "Dependency controller definitions are read-only. Fork the package before editing this controller."
                        .to_string(),
                ));
            }
            let definition = session
                .project
                .controllers
                .get_mut(&ControllerId(identity))
                .ok_or_else(|| {
                    GuiMutationError::Invalid("Controller was not found.".to_string())
                })?;
            let port = definition
                .ports
                .iter_mut()
                .find(|candidate| candidate.id.0 == port)
                .ok_or_else(|| {
                    GuiMutationError::Invalid("Controller port was not found.".to_string())
                })?;
            port.address = match definition.protocol {
                ControllerProtocol::E131(_) => ControllerPortAddress::E131Universe(address),
                ControllerProtocol::ArtNet(_) => ControllerPortAddress::ArtNetPort(address),
            };
            port.slot_count = slot_count;
        }
    }
    validate_project(&session.project)
        .map_err(|error| GuiMutationError::Invalid(format!("{error:?}")))
}

fn tree_mut<'a>(
    session: &'a mut ProjectSession,
    id: &dawn_language::element::ElementTreeId,
) -> Result<&'a mut dawn_language::element::ElementTree, GuiMutationError> {
    session
        .project
        .element_trees
        .get_mut(id)
        .ok_or_else(|| GuiMutationError::Invalid("Element tree is missing.".to_string()))
}

fn preview_prop_mut<'a>(
    session: &'a mut ProjectSession,
    id: &dawn_language::preview::PreviewLayoutId,
    prop: u32,
) -> Result<&'a mut dawn_language::preview::PropInstance, GuiMutationError> {
    session
        .project
        .preview_layouts
        .get_mut(id)
        .and_then(|preview| {
            preview
                .props
                .iter_mut()
                .find(|candidate| candidate.id == PropInstanceId(prop))
        })
        .ok_or_else(|| GuiMutationError::Invalid("Preview prop was not found.".to_string()))
}

fn source_key(id: &dawn_language::identity::SourceIdentity) -> String {
    format!("{}#{}", id.document(), id.object())
}

fn filter_width(filter: &dawn_language::patch::FilterDefinition) -> usize {
    use dawn_language::patch::FilterDefinition;
    match filter {
        FilterDefinition::ColorBreakdown { cell_count, .. } => *cell_count,
        FilterDefinition::DimmingCurve { width, .. }
        | FilterDefinition::ScaleInvert { width, .. }
        | FilterDefinition::FanOut { width, .. }
        | FilterDefinition::IndexedValueMapping { width, .. }
        | FilterDefinition::Quantize8 { width }
        | FilterDefinition::Quantize16 { width, .. } => *width,
        FilterDefinition::ComponentReorder {
            components_per_cell,
            cell_count,
            ..
        } => usize::from(*components_per_cell) * *cell_count,
        FilterDefinition::FixtureProfileEncoding { slot_count, .. } => *slot_count,
    }
}
