use dawn_language::dsl::{TargetItemValue, TargetPixelValue, TargetValue};
use dawn_language::effect::EffectScope;
use dawn_language::element::ElementSelection;
use dawn_language::element::{ElementCellRange, ElementNodeId};
use dawn_language::model::DawnProject;
use dawn_language::setup::SetupId;
pub(crate) use dawn_runtime::sequence::PreparedPixel as PreparedTargetPixel;
use indexmap::{IndexMap, IndexSet};
use std::collections::HashMap;
use std::sync::Arc;

use crate::RenderError;
use crate::sequence::elements::{PreparedElement, prepare_elements};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RenderedTargetPixelAddress {
    pub element_id: ElementNodeId,
    pub element_cell_index: usize,
}

fn pixel_fraction(index: usize, count: usize) -> f32 {
    if count <= 1 {
        0.0
    } else {
        index as f32 / (count - 1) as f32
    }
}

pub fn resolve_effect_target_pixel_addresses(
    project: &DawnProject,
    setup_id: &SetupId,
    target: &ElementSelection,
    scope: &EffectScope,
) -> Result<Vec<RenderedTargetPixelAddress>, RenderError> {
    let setup = project
        .setups
        .get(setup_id)
        .ok_or_else(|| RenderError::MissingSetup {
            setup_id: setup_id.clone(),
        })?;
    let tree = project
        .element_trees
        .get(&setup.elements)
        .ok_or(RenderError::MissingElementTree)?;
    let (elements, groups) = prepare_elements(project, tree)?;
    let element_ids = elements
        .iter()
        .map(|element| element.id)
        .collect::<IndexSet<_>>();
    let target = prepare_target(target, &element_ids, &groups)?;
    let pixels = prepare_target_pixels(&target, &elements, scope)?;
    Ok(pixels
        .into_iter()
        .map(|pixel| RenderedTargetPixelAddress {
            element_id: elements[pixel.element_index()].id,
            element_cell_index: pixel.element_cell_index(),
        })
        .collect())
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub(crate) struct PreparedTargetSelection {
    pub(crate) elements: Vec<ElementNodeId>,
    pub(crate) cells: Option<ElementCellRange>,
}

#[derive(Default)]
pub(crate) struct PreparedTargetCache {
    prepared_targets: HashMap<PreparedTargetCacheKey, Arc<[PreparedTargetPixel]>>,
    pub(crate) sample_targets: Vec<Arc<[PreparedTargetPixel]>>,
    generated_targets: HashMap<usize, GeneratedTargetCacheEntry>,
    generator_context_targets: HashMap<usize, GeneratorContextTargetCacheEntry>,
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
                element_cell_index as f32 / (element.pixel_count - 1) as f32
            };
            pixels.push(
                PreparedTargetPixel::try_new(
                    element_index,
                    element_cell_index,
                    pixel_index,
                    element.pixel_count,
                    pixel_fraction,
                )
                .ok_or(RenderError::BadTarget)?,
            );
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
            pixels.push(
                PreparedTargetPixel::try_new(
                    element_index,
                    element_cell_index,
                    pixel_index,
                    pixel_count,
                    pixel_fraction(pixel_index, pixel_count),
                )
                .ok_or(RenderError::BadTarget)?,
            );
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
) -> Result<Arc<[PreparedTargetPixel]>, RenderError> {
    let key = PreparedTargetCacheKey {
        target: target.clone(),
        scope: PreparedTargetScopeKey::from(scope),
    };
    if let Some(pixels) = cache.prepared_targets.get(&key) {
        return Ok(Arc::clone(pixels));
    }
    let pixels = Arc::from(prepare_target_pixels(target, elements, scope)?);
    cache.prepared_targets.insert(key, Arc::clone(&pixels));
    Ok(pixels)
}

pub(crate) fn sorted_sample_target(
    target: &Arc<[PreparedTargetPixel]>,
) -> Arc<[PreparedTargetPixel]> {
    if target.is_sorted_by_key(|pixel| (pixel.element_index, pixel.element_cell_index)) {
        return Arc::clone(target);
    }
    let mut sorted = target.to_vec();
    sorted.sort_by_key(|pixel| (pixel.element_index, pixel.element_cell_index));
    Arc::from(sorted)
}

pub(crate) fn generator_expansion_targets(
    target: &Arc<[PreparedTargetPixel]>,
    scope: &EffectScope,
) -> Vec<Arc<[PreparedTargetPixel]>> {
    match scope {
        EffectScope::WholeTarget => vec![Arc::clone(target)],
        EffectScope::PerFixture => {
            let mut targets = Vec::new();
            let mut element_pixels = Vec::new();
            let mut current_element_index = None;

            for pixel in target.iter() {
                if current_element_index.is_some_and(|index| index != pixel.element_index) {
                    targets.push(Arc::from(element_pixels));
                    element_pixels = Vec::new();
                }
                current_element_index = Some(pixel.element_index);
                element_pixels.push(pixel.clone());
            }

            if !element_pixels.is_empty() {
                targets.push(Arc::from(element_pixels));
            }

            targets
        }
    }
}

struct GeneratedTargetCacheEntry {
    source: Arc<TargetItemValue>,
    pixels: Arc<[PreparedTargetPixel]>,
}

struct GeneratorContextTargetCacheEntry {
    source: Arc<[PreparedTargetPixel]>,
    target: Arc<TargetValue>,
}

fn arc_key<T: ?Sized>(value: &Arc<T>) -> usize {
    Arc::as_ptr(value).cast::<()>() as usize
}

fn target_groups_from_pixels(pixels: &[PreparedTargetPixel]) -> Vec<Arc<TargetItemValue>> {
    vec![Arc::new(TargetItemValue {
        pixels: Arc::from(pixels.iter().map(target_pixel_value).collect::<Vec<_>>()),
    })]
}

pub(crate) fn generator_context_target(
    cache: &mut PreparedTargetCache,
    prepared_target: &Arc<[PreparedTargetPixel]>,
) -> Arc<TargetValue> {
    let key = arc_key(prepared_target);
    if let Some(entry) = cache.generator_context_targets.get(&key)
        && Arc::ptr_eq(&entry.source, prepared_target)
    {
        return Arc::clone(&entry.target);
    }
    let target = Arc::new(TargetValue {
        groups: target_groups_from_pixels(prepared_target),
    });
    cache.generator_context_targets.insert(
        key,
        GeneratorContextTargetCacheEntry {
            source: Arc::clone(prepared_target),
            target: Arc::clone(&target),
        },
    );
    target
}

fn target_pixel_value(pixel: &PreparedTargetPixel) -> TargetPixelValue {
    TargetPixelValue {
        element_index: pixel.element_index() as i32,
        element_cell_index: pixel.element_cell_index() as i32,
        pixel_index: pixel.pixel_index() as i32,
        pixel_count: pixel.pixel_count() as i32,
        pixel_fraction: pixel.pixel_fraction,
    }
}

fn prepared_pixels_from_generated_target(
    elements: &[PreparedElement],
    target: Arc<TargetItemValue>,
) -> Result<Vec<PreparedTargetPixel>, RenderError> {
    target
        .pixels
        .iter()
        .copied()
        .map(|pixel| {
            let element_index = usize::try_from(pixel.element_index).map_err(|_| {
                RenderError::GeneratorPrepare {
                    message: "generated target element index cannot be negative".to_string(),
                }
            })?;
            let element_cell_index = usize::try_from(pixel.element_cell_index).map_err(|_| {
                RenderError::GeneratorPrepare {
                    message: "generated target pixel index cannot be negative".to_string(),
                }
            })?;
            let element =
                elements
                    .get(element_index)
                    .ok_or_else(|| RenderError::GeneratorPrepare {
                        message: "generated target element index is out of bounds".to_string(),
                    })?;
            if element_cell_index >= element.pixel_count {
                return Err(RenderError::GeneratorPrepare {
                    message: "generated target pixel index is out of bounds".to_string(),
                });
            }
            let pixel_index =
                usize::try_from(pixel.pixel_index).map_err(|_| RenderError::GeneratorPrepare {
                    message: "generated target pixel context index cannot be negative".to_string(),
                })?;
            let pixel_count =
                usize::try_from(pixel.pixel_count).map_err(|_| RenderError::GeneratorPrepare {
                    message: "generated target pixel context count cannot be negative".to_string(),
                })?;
            PreparedTargetPixel::try_new(
                element_index,
                element_cell_index,
                pixel_index,
                pixel_count,
                pixel.pixel_fraction,
            )
            .ok_or_else(|| RenderError::GeneratorPrepare {
                message: "generated target pixel exceeds the prepared runtime range".to_string(),
            })
        })
        .collect()
}

pub(crate) fn prepared_pixels_from_generated_target_cached(
    cache: &mut PreparedTargetCache,
    elements: &[PreparedElement],
    target: Arc<TargetItemValue>,
) -> Result<Arc<[PreparedTargetPixel]>, RenderError> {
    let key = arc_key(&target);
    if let Some(entry) = cache.generated_targets.get(&key)
        && Arc::ptr_eq(&entry.source, &target)
    {
        return Ok(Arc::clone(&entry.pixels));
    }
    let pixels = Arc::from(prepared_pixels_from_generated_target(
        elements,
        Arc::clone(&target),
    )?);
    cache.generated_targets.insert(
        key,
        GeneratedTargetCacheEntry {
            source: target,
            pixels: Arc::clone(&pixels),
        },
    );
    Ok(pixels)
}

impl PreparedTargetCache {
    pub(crate) fn sample_target(
        &mut self,
        pixels: Arc<[PreparedTargetPixel]>,
    ) -> Result<u32, RenderError> {
        let key = |pixel: &PreparedTargetPixel| {
            (
                pixel.element_index,
                pixel.element_cell_index,
                pixel.pixel_index,
                pixel.pixel_count,
                pixel.pixel_fraction.to_bits(),
            )
        };
        // Intern exact contexts, including local indices and float bits, not just output addresses.
        let index = self
            .sample_targets
            .iter()
            .position(|target| {
                Arc::ptr_eq(target, &pixels) || target.iter().map(key).eq(pixels.iter().map(key))
            })
            .unwrap_or(self.sample_targets.len());
        let id = u32::try_from(index).map_err(|_| RenderError::BadTarget)?;
        if index == self.sample_targets.len() {
            self.sample_targets.push(pixels);
        }
        Ok(id)
    }
}

#[cfg(test)]
mod representation_tests {
    use super::PreparedTargetPixel;

    #[test]
    fn prepared_target_pixel_stays_32_bit_compact() {
        assert_eq!(std::mem::size_of::<PreparedTargetPixel>(), 16);
    }
}
