use dawn_language::effect::EffectScope;
use dawn_language::element::{ElementCellRange, ElementNodeId, ElementSelection};
use indexmap::{IndexMap, IndexSet};
use std::collections::HashMap;
use std::sync::Arc;

use super::RenderError;
use super::elements::PreparedElement;

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub(crate) struct PreparedTargetSelection {
    pub(crate) elements: Vec<ElementNodeId>,
    pub(crate) cells: Option<ElementCellRange>,
}

#[derive(Clone, Debug)]
pub(crate) struct PreparedTargetPixel {
    pub(crate) element_index: usize,
    pub(crate) element_cell_index: usize,
    pub(crate) pixel_index: usize,
    pub(crate) pixel_count: usize,
    pub(crate) pixel_fraction: f64,
}

#[derive(Default)]
pub(crate) struct PreparedTargetCache {
    prepared_targets: HashMap<PreparedTargetCacheKey, Arc<Vec<PreparedTargetPixel>>>,
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
struct PreparedTargetCacheKey {
    target: PreparedTargetSelection,
    scope: PreparedTargetScopeKey,
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
enum PreparedTargetScopeKey {
    PerFixture,
    WholeTarget,
}

impl From<&EffectScope> for PreparedTargetScopeKey {
    fn from(scope: &EffectScope) -> Self {
        match scope {
            EffectScope::PerFixture => Self::PerFixture,
            EffectScope::WholeTarget => Self::WholeTarget,
        }
    }
}

pub(crate) fn full_rig_target_pixels(
    elements: &[PreparedElement],
) -> Result<Vec<PreparedTargetPixel>, RenderError> {
    let mut pixels = Vec::new();
    for (element_index, element) in elements.iter().enumerate() {
        for element_cell_index in 0..element.pixel_count {
            let pixel_index = element_cell_index;
            let pixel_fraction = if element.pixel_count <= 1 {
                0.0
            } else {
                element_cell_index as f64 / (element.pixel_count - 1) as f64
            };
            pixels.push(PreparedTargetPixel {
                element_index,
                element_cell_index,
                pixel_index,
                pixel_count: element.pixel_count,
                pixel_fraction,
            });
        }
    }
    Ok(pixels)
}

pub(crate) fn prepare_target(
    target: &ElementSelection,
    element_ids: &IndexSet<ElementNodeId>,
    groups: &IndexMap<ElementNodeId, Vec<ElementNodeId>>,
) -> Result<PreparedTargetSelection, RenderError> {
    if let Some(members) = groups.get(&target.node) {
        if target.cells.is_some() {
            return Err(RenderError::BadTarget);
        }
        return Ok(PreparedTargetSelection {
            elements: members.clone(),
            cells: None,
        });
    }
    if !element_ids.contains(&target.node) {
        return Err(RenderError::MissingElement {
            element_id: target.node,
        });
    }
    Ok(PreparedTargetSelection {
        elements: vec![target.node],
        cells: target.cells,
    })
}

fn prepare_target_indexes(
    target: &[ElementNodeId],
    elements: &[PreparedElement],
) -> Result<Vec<usize>, RenderError> {
    target
        .iter()
        .map(|id| {
            elements
                .iter()
                .position(|element| &element.id == id)
                .ok_or(RenderError::MissingElement { element_id: *id })
        })
        .collect()
}

pub(crate) fn prepare_target_pixels(
    target: &PreparedTargetSelection,
    elements: &[PreparedElement],
    scope: &EffectScope,
) -> Result<Vec<PreparedTargetPixel>, RenderError> {
    let indexes = prepare_target_indexes(&target.elements, elements)?;
    if indexes.iter().any(|index| !elements[*index].color_enabled) {
        return Err(RenderError::BadTarget);
    }
    let total_target_pixels = indexes
        .iter()
        .map(|index| {
            selected_cell_range(elements[*index].pixel_count, target.cells).map(|range| range.len())
        })
        .collect::<Result<Vec<_>, _>>()?
        .into_iter()
        .sum::<usize>();
    let mut pixels = Vec::with_capacity(total_target_pixels);
    let mut whole_index = 0usize;
    for element_index in indexes {
        let element_cell_count = elements[element_index].pixel_count;
        let cells = selected_cell_range(element_cell_count, target.cells)?;
        for (selected_index, element_cell_index) in cells.enumerate() {
            let (pixel_index, pixel_count) = match scope {
                EffectScope::PerFixture => (
                    selected_index,
                    target
                        .cells
                        .map_or(element_cell_count, |range| range.count as usize),
                ),
                EffectScope::WholeTarget => (whole_index, total_target_pixels),
            };
            pixels.push(PreparedTargetPixel {
                element_index,
                element_cell_index,
                pixel_index,
                pixel_count,
                pixel_fraction: super::pixel_fraction(pixel_index, pixel_count),
            });
            whole_index += 1;
        }
    }
    Ok(pixels)
}

fn selected_cell_range(
    pixel_count: usize,
    range: Option<ElementCellRange>,
) -> Result<std::ops::Range<usize>, RenderError> {
    let Some(range) = range else {
        return Ok(0..pixel_count);
    };
    let start = range.start as usize;
    let end = start
        .checked_add(range.count as usize)
        .ok_or(RenderError::BadTarget)?;
    if range.count == 0 || end > pixel_count {
        return Err(RenderError::BadTarget);
    }
    Ok(start..end)
}

pub(crate) fn prepare_target_pixels_cached(
    cache: &mut PreparedTargetCache,
    target: &PreparedTargetSelection,
    elements: &[PreparedElement],
    scope: &EffectScope,
) -> Result<Arc<Vec<PreparedTargetPixel>>, RenderError> {
    let key = PreparedTargetCacheKey {
        target: target.clone(),
        scope: PreparedTargetScopeKey::from(scope),
    };
    if let Some(pixels) = cache.prepared_targets.get(&key) {
        return Ok(Arc::clone(pixels));
    }
    let pixels = Arc::new(prepare_target_pixels(target, elements, scope)?);
    cache.prepared_targets.insert(key, Arc::clone(&pixels));
    Ok(pixels)
}

pub(crate) fn generator_expansion_targets(
    target: &Arc<Vec<PreparedTargetPixel>>,
    scope: &EffectScope,
) -> Vec<Arc<Vec<PreparedTargetPixel>>> {
    match scope {
        EffectScope::WholeTarget => vec![Arc::clone(target)],
        EffectScope::PerFixture => {
            let mut targets = Vec::new();
            let mut element_pixels = Vec::new();
            let mut current_element_index = None;

            for pixel in target.iter() {
                if current_element_index.is_some_and(|index| index != pixel.element_index) {
                    targets.push(Arc::new(element_pixels));
                    element_pixels = Vec::new();
                }
                current_element_index = Some(pixel.element_index);
                element_pixels.push(pixel.clone());
            }

            if !element_pixels.is_empty() {
                targets.push(Arc::new(element_pixels));
            }

            targets
        }
    }
}
