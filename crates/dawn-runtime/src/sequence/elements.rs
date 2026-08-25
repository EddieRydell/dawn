use dawn_language::element::{ElementNodeId, ElementNodeKind, ElementSelection, ElementTree};
use dawn_language::model::DawnProject;
use indexmap::{IndexMap, IndexSet};

use crate::RenderError;

#[derive(Clone, Debug)]
pub(crate) struct PreparedElement {
    pub(crate) id: ElementNodeId,
    pub(crate) pixel_count: usize,
    pub(crate) color_enabled: bool,
}

pub(crate) type PreparedElements = (
    Vec<PreparedElement>,
    IndexMap<ElementNodeId, Vec<ElementNodeId>>,
);

pub(crate) fn prepare_elements(
    project: &DawnProject,
    tree: &ElementTree,
) -> Result<PreparedElements, RenderError> {
    let mut ordered = Vec::new();
    let mut seen = IndexSet::new();
    for address in tree.flattened_cells().map_err(|_| RenderError::BadTarget)? {
        if seen.insert(address.node) {
            ordered.push(address.node);
        }
    }
    let elements = ordered
        .into_iter()
        .map(|id| {
            let node = tree
                .nodes
                .get(&id)
                .ok_or(RenderError::MissingElement { element_id: id })?;
            let (pixel_count, color_enabled) = match &node.kind {
                ElementNodeKind::Color { cells, .. } => (*cells as usize, true),
                ElementNodeKind::Scalar { cells } | ElementNodeKind::Indexed { cells, .. } => {
                    (*cells as usize, false)
                }
                ElementNodeKind::Fixture { profile } => {
                    let profile = project
                        .definitions
                        .fixture_profiles
                        .definitions
                        .get(profile)
                        .ok_or_else(|| RenderError::BadGraph {
                            message: "fixture element references a missing profile".to_string(),
                        })?;
                    let color_enabled = profile.functions.values().any(|function| {
                        matches!(
                            function.kind,
                            dawn_language::fixture_profile::FixtureFunctionKind::ColorMixing { .. }
                        )
                    });
                    (1, color_enabled)
                }
                ElementNodeKind::Group { .. } => return Err(RenderError::BadTarget),
            };
            Ok(PreparedElement {
                id,
                pixel_count,
                color_enabled,
            })
        })
        .collect::<Result<Vec<_>, _>>()?;
    let mut groups = IndexMap::new();
    for (id, node) in &tree.nodes {
        if matches!(node.kind, ElementNodeKind::Group { .. }) {
            let selection = ElementSelection {
                tree: tree.id.clone(),
                node: *id,
                cells: None,
            };
            let mut members = Vec::new();
            let mut seen = IndexSet::new();
            for address in tree
                .flatten_selection(&selection)
                .map_err(|_| RenderError::BadTarget)?
            {
                if seen.insert(address.node) {
                    members.push(address.node);
                }
            }
            groups.insert(*id, members);
        }
    }
    Ok((elements, groups))
}

pub(crate) fn element_cell_offsets(elements: &[PreparedElement]) -> (Vec<usize>, usize) {
    let mut pixel_count = 0usize;
    let offsets = elements
        .iter()
        .map(|element| {
            let offset = pixel_count;
            pixel_count += element.pixel_count;
            offset
        })
        .collect();
    (offsets, pixel_count)
}
