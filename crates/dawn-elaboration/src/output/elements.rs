use std::collections::HashMap;

use dawn_language::element::{ElementNodeId, ElementNodeKind, ElementTree};
use dawn_language::fixture_profile::FixtureProfileStore;
use dawn_runtime::element::ElementLayout;

use super::errors::SequenceOutputPrepareError;

pub(crate) struct OutputElements {
    pub(crate) layouts: Vec<(ElementNodeId, ElementLayout)>,
    pub(crate) indexes: HashMap<ElementNodeId, u32>,
}

impl OutputElements {
    pub(crate) fn prepare(
        tree: &ElementTree,
        profiles: &FixtureProfileStore,
    ) -> Result<Self, SequenceOutputPrepareError> {
        let mut layouts = Vec::new();
        let mut indexes = HashMap::new();
        for (id, node) in &tree.nodes {
            let layout = match &node.kind {
                ElementNodeKind::Group { .. } => continue,
                ElementNodeKind::Color { cells, .. } => ElementLayout::Color(*cells),
                ElementNodeKind::Scalar { cells } => ElementLayout::Scalar(*cells),
                ElementNodeKind::Indexed { cells, .. } => ElementLayout::Indexed(*cells),
                ElementNodeKind::Fixture { profile } => ElementLayout::Fixture(
                    u32::try_from(profiles.definitions[profile].functions.len()).map_err(|_| {
                        SequenceOutputPrepareError::InvalidPatch(
                            "fixture function count exceeds u32".to_string(),
                        )
                    })?,
                ),
            };
            let index = u32::try_from(layouts.len()).map_err(|_| {
                SequenceOutputPrepareError::InvalidPatch("element index exceeds u32".to_string())
            })?;
            indexes.insert(*id, index);
            layouts.push((*id, layout));
        }
        Ok(Self { layouts, indexes })
    }
}
