use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::hash::{Hash, Hasher};
use std::time::Instant;

use crate::document::geometry_render_plan;
use crate::document::{
    EffectScriptReferenceDocument, GeometryRenderBounds, GeometryRenderPoint, LayoutTargetDocument,
    SequenceEditorDocument, SequenceEffectDocument, SequenceEffectParamDocument,
    SequenceMarkCollectionDocument,
};

use dawn_project::{
    resolve_import_path, CurveUse, DawnProject, EffectDefinitionKey, PathStringExt,
    RuntimeArrayValue, Utf8PathBuf,
};
use dawn_project::{
    BytecodeStats, CompiledEffect, EffectSampleScratch, EffectScriptKind, FixtureContext,
    GeneratedChildEffectRef, GeneratedChildTopology, GeneratorTarget, GeneratorTargetPixel,
    PixelContext, PreparedEffectParams, RuntimeError, RuntimeMarks, RuntimeValue,
};
use dawn_project::{
    Color, Curve, CurveValue, CurveValueType, Distance, DistanceSpan, EffectParam, FixtureId,
    Layout, LayoutTargetKind, Resolved, ResolvedInlineOrRef, SequenceEffectScope, Time, TimeSpan,
    Transform,
};

const MAX_FLATTENED_GENERATED_CHILDREN: usize = 65_536;
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct OutputGeometryIdentity {
    pub bounds: OutputGeometryBoundsIdentity,
    pub fixtures: Vec<OutputFixtureTopologyIdentity>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct OutputGeometryBoundsIdentity {
    pub min_x_micrometers: i64,
    pub min_y_micrometers: i64,
    pub max_x_micrometers: i64,
    pub max_y_micrometers: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct OutputFixtureTopologyIdentity {
    pub id: FixtureId,
    pub name: String,
    pub pixel_count: usize,
}

impl OutputGeometryIdentity {
    pub fn from_parts(bounds: GeometryRenderBounds, fixtures: &[OutputFixtureFrame]) -> Self {
        Self {
            bounds: OutputGeometryBoundsIdentity {
                min_x_micrometers: bounds.min_x.as_micrometers(),
                min_y_micrometers: bounds.min_y.as_micrometers(),
                max_x_micrometers: bounds.max_x.as_micrometers(),
                max_y_micrometers: bounds.max_y.as_micrometers(),
            },
            fixtures: fixtures
                .iter()
                .map(|fixture| OutputFixtureTopologyIdentity {
                    id: fixture.id,
                    name: fixture.name.clone(),
                    pixel_count: fixture.pixels.len(),
                })
                .collect(),
        }
    }

    pub fn stable_key(&self) -> String {
        let mut key = format!(
            "{}|{}|{}|{}|{}",
            self.bounds.min_x_micrometers,
            self.bounds.min_y_micrometers,
            self.bounds.max_x_micrometers,
            self.bounds.max_y_micrometers,
            self.fixtures.len()
        );
        for fixture in &self.fixtures {
            key.push_str(&format!(
                "|{}:{}:{}",
                fixture.id.0, fixture.name, fixture.pixel_count
            ));
        }
        key
    }
}

fn output_fixture_templates(project: &DawnProject) -> Result<Vec<OutputFixtureFrame>, String> {
    let layout = active_layout(project)?;
    layout
        .fixtures
        .iter()
        .map(|fixture| {
            let fixture_definition = match &fixture.fixture {
                ResolvedInlineOrRef::Inline(fixture) => fixture,
                ResolvedInlineOrRef::Ref(reference) => project
                    .stores
                    .fixture_definitions
                    .get(&reference.key)
                    .map(|fixture| &fixture.value)
                    .ok_or_else(|| {
                        format!("fixture `{}` was not found", reference.key.display_key())
                    })?,
            };
            let render_plan = geometry_render_plan(fixture_definition);
            Ok(OutputFixtureFrame {
                id: fixture.id,
                name: fixture
                    .name
                    .clone()
                    .unwrap_or_else(|| fixture.id.to_string()),
                bulb_radius: render_plan.bulb_radius,
                pixels: render_plan
                    .emitters
                    .iter()
                    .map(|position| {
                        Ok(OutputPixelFrame {
                            position: transform_render_point(*position, fixture.transform)?,
                            color: Color::new(0, 0, 0),
                        })
                    })
                    .collect::<Result<Vec<_>, String>>()?,
            })
        })
        .collect::<Result<Vec<_>, String>>()
}

fn render_pixels_for_fixture(
    fixture_index: usize,
    fixtures: &[OutputFixtureFrame],
) -> Vec<SequenceRenderPixel> {
    let Some(fixture) = fixtures.get(fixture_index) else {
        return Vec::new();
    };
    let pixel_count = fixture.pixels.len();
    (0..pixel_count)
        .map(|pixel_index| SequenceRenderPixel {
            fixture_index,
            pixel_index,
            pixel_count,
        })
        .collect()
}

fn transform_render_point(
    point: GeometryRenderPoint,
    transform: Transform,
) -> Result<GeometryRenderPoint, String> {
    let mut x = point.x.as_meters_f64() * transform.scale.x;
    let mut y = point.y.as_meters_f64() * transform.scale.y;
    let mut z = point.z.as_meters_f64() * transform.scale.z;

    let rotation_x = transform.rotation.x.to_radians();
    let (sin_x, cos_x) = rotation_x.sin_cos();
    let rotated_y = y * cos_x - z * sin_x;
    let rotated_z = y * sin_x + z * cos_x;
    y = rotated_y;
    z = rotated_z;

    let rotation_y = transform.rotation.y.to_radians();
    let (sin_y, cos_y) = rotation_y.sin_cos();
    let rotated_x = x * cos_y + z * sin_y;
    let rotated_z = -x * sin_y + z * cos_y;
    x = rotated_x;
    z = rotated_z;

    let rotation_z = transform.rotation.z.to_radians();
    let (sin_z, cos_z) = rotation_z.sin_cos();
    let rotated_x = x * cos_z - y * sin_z;
    let rotated_y = x * sin_z + y * cos_z;
    x = rotated_x + transform.position.x.as_meters_f64();
    y = rotated_y + transform.position.y.as_meters_f64();
    z += transform.position.z.as_meters_f64();

    Ok(GeometryRenderPoint {
        x: Distance::try_from_meters_f64_truncated(x)
            .map_err(|error| format!("layout transform produced invalid x coordinate: {error}"))?,
        y: Distance::try_from_meters_f64_truncated(y)
            .map_err(|error| format!("layout transform produced invalid y coordinate: {error}"))?,
        z: Distance::try_from_meters_f64_truncated(z)
            .map_err(|error| format!("layout transform produced invalid z coordinate: {error}"))?,
    })
}

fn output_bounds(fixtures: &[OutputFixtureFrame]) -> GeometryRenderBounds {
    let mut points = fixtures
        .iter()
        .flat_map(|fixture| fixture.pixels.iter().map(|pixel| pixel.position));
    let Some(first) = points.next() else {
        return GeometryRenderBounds {
            min_x: Distance::ZERO,
            min_y: Distance::ZERO,
            max_x: Distance::ZERO,
            max_y: Distance::ZERO,
        };
    };
    let mut min_x = first.x;
    let mut min_y = first.y;
    let mut max_x = first.x;
    let mut max_y = first.y;
    for point in points {
        min_x = min_x.min(point.x);
        min_y = min_y.min(point.y);
        max_x = max_x.max(point.x);
        max_y = max_y.max(point.y);
    }
    GeometryRenderBounds {
        min_x,
        min_y,
        max_x,
        max_y,
    }
}

fn active_layout(project: &DawnProject) -> Result<&Layout<Resolved>, String> {
    let display = match &project.display {
        ResolvedInlineOrRef::Inline(display) => display,
        ResolvedInlineOrRef::Ref(reference) => project
            .stores
            .displays
            .get(&reference.key)
            .map(|display| &display.value)
            .ok_or_else(|| format!("display `{}` was not found", reference.key.display_key()))?,
    };
    match &display.layout {
        ResolvedInlineOrRef::Inline(layout) => Ok(layout),
        ResolvedInlineOrRef::Ref(reference) => project
            .stores
            .layouts
            .get(&reference.key)
            .map(|layout| &layout.value)
            .ok_or_else(|| format!("layout `{}` was not found", reference.key.display_key())),
    }
}

#[derive(Debug, Clone)]
pub struct OutputSourceMetadata {
    pub label: String,
    pub kind: OutputSourceKind,
    pub duration_seconds: f64,
    pub fps: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum OutputSourceKind {
    Sequence,
    Empty,
}

#[derive(Debug, Clone)]
pub enum OutputFrameStatus {
    Live,
    Idle(String),
    Error(String),
}

#[derive(Debug, Clone)]
pub struct OutputFixtureFrame {
    pub id: FixtureId,
    pub name: String,
    pub bulb_radius: DistanceSpan,
    pub pixels: Vec<OutputPixelFrame>,
}

#[derive(Debug, Clone)]
pub struct OutputPixelFrame {
    pub position: GeometryRenderPoint,
    pub color: Color,
}

#[derive(Debug, Clone)]
pub struct OutputGeometryModel {
    pub geometry_id: String,
    pub bounds: GeometryRenderBounds,
    pub fixtures: Vec<OutputFixtureFrame>,
    target_pixels_by_target: HashMap<LayoutTargetDocument, Vec<SequenceRenderPixel>>,
}

impl OutputGeometryModel {
    pub fn from_project(project: &DawnProject) -> Result<Self, String> {
        let layout = active_layout(project)?;
        let fixtures = output_fixture_templates(project)?;
        let bounds = output_bounds(&fixtures);
        let mut target_pixels_by_target = HashMap::new();
        for (fixture_index, fixture) in layout.fixtures.iter().enumerate() {
            let pixels = render_pixels_for_fixture(fixture_index, &fixtures);
            target_pixels_by_target.insert(
                LayoutTargetDocument {
                    kind: LayoutTargetKind::Fixture,
                    name: fixture.id.to_string(),
                },
                pixels.clone(),
            );
            if let Some(name) = fixture.name.as_ref() {
                target_pixels_by_target.insert(
                    LayoutTargetDocument {
                        kind: LayoutTargetKind::Fixture,
                        name: name.clone(),
                    },
                    pixels,
                );
            }
        }
        for group in &layout.groups {
            let pixels = group
                .members
                .iter()
                .flat_map(|member_id| {
                    layout
                        .fixtures
                        .iter()
                        .position(|fixture| fixture.id == *member_id)
                        .map(|fixture_index| render_pixels_for_fixture(fixture_index, &fixtures))
                        .unwrap_or_default()
                })
                .collect::<Vec<_>>();
            target_pixels_by_target.insert(
                LayoutTargetDocument {
                    kind: LayoutTargetKind::Group,
                    name: group.id.to_string(),
                },
                pixels.clone(),
            );
            if let Some(name) = group.name.as_ref() {
                target_pixels_by_target.insert(
                    LayoutTargetDocument {
                        kind: LayoutTargetKind::Group,
                        name: name.clone(),
                    },
                    pixels,
                );
            }
        }
        let geometry_id = OutputGeometryIdentity::from_parts(bounds, &fixtures).stable_key();
        Ok(Self {
            geometry_id,
            bounds,
            fixtures,
            target_pixels_by_target,
        })
    }

    fn target_pixels(
        &self,
        target: &LayoutTargetDocument,
    ) -> Result<Vec<SequenceRenderPixel>, String> {
        self.target_pixels_by_target
            .iter()
            .find(|(candidate, _)| candidate.kind == target.kind && candidate.name == target.name)
            .map(|(_, pixels)| pixels.clone())
            .ok_or_else(|| format!("target `{:?} {}` was not found", target.kind, target.name))
    }

    fn all_pixels(&self) -> Vec<SequenceRenderPixel> {
        self.fixtures
            .iter()
            .enumerate()
            .flat_map(|(fixture_index, _)| render_pixels_for_fixture(fixture_index, &self.fixtures))
            .collect()
    }
}

#[derive(Debug, Clone)]
pub struct SequenceRenderEffect {
    pub script: EffectDefinitionKey,
    pub script_source: String,
    pub params: Vec<SequenceEffectParamDocument>,
    pub target_pixels: Vec<SequenceRenderPixel>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct SequenceRenderPixel {
    pub fixture_index: usize,
    pub pixel_index: usize,
    pub pixel_count: usize,
}

#[derive(Debug, Clone)]
pub struct RenderedOutputFrame {
    pub geometry_id: String,
    pub generation: u64,
    pub time_seconds: f64,
    pub status: OutputFrameStatus,
    pub rgb: Vec<u8>,
}

pub trait OutputSink {
    fn write_frame(&self, frame: RenderedOutputFrame);
}

#[derive(Debug, Clone, Copy, Default)]
pub struct SequenceFrameRenderTiming {
    pub total_ms: f64,
    pub render_buffer_clone_ms: f64,
    pub effect_loop_ms: f64,
    pub rgb_buffer_ms: f64,
    pub active_effects: u32,
    pub active_authored_effects: u32,
    pub active_prepared_effects: u32,
    pub visited_prepared_effects: u32,
    pub sampled_pixels: u32,
    pub vm_sample_evaluations: u32,
    pub sample_reuse_saved_evaluations: u32,
    pub sample_reuse_group_hits: u32,
}

#[derive(Debug, Clone, Default)]
pub struct SequenceRenderPlanBuildTiming {
    pub total_ms: f64,
    pub layout_template_ms: f64,
    pub authored_sample_ms: f64,
    pub generator_expansion_ms: f64,
    pub timeline_index_ms: f64,
    pub prepared_effect_count: usize,
    pub generator_parent_count: usize,
    pub generated_child_count: usize,
    pub generator_parents: Vec<GeneratorParentPreparationTiming>,
}

#[derive(Debug, Clone)]
pub struct GeneratorParentPreparationTiming {
    pub parent_effect_id: u32,
    pub script_id: EffectDefinitionKey,
    pub target_pixels: usize,
    pub emitted_children: usize,
    pub prepared_children: usize,
    pub prepared_cache_hit: bool,
    pub topology_cache_hit: bool,
    pub total_prepare_ms: f64,
}

#[derive(Debug, Clone, Default)]
pub struct SequencePreparationCache {
    entries: HashMap<u32, PreparedEffectCacheEntry>,
    generator_topology_entries: HashMap<u32, GeneratorTopologyCacheEntry>,
}

impl SequencePreparationCache {
    fn prepared_effects(
        &self,
        effect_id: u32,
        key: &PreparedEffectCacheKey,
        parent_start_seconds: f64,
    ) -> Option<Vec<PreparedSequenceEffect>> {
        let entry = self.entries.get(&effect_id)?;
        (entry.key == *key)
            .then(|| shift_prepared_effects_to_parent_start(&entry.effects, parent_start_seconds))
    }

    fn store(
        &mut self,
        effect_id: u32,
        key: PreparedEffectCacheKey,
        parent_start_seconds: f64,
        effects: &[PreparedSequenceEffect],
    ) {
        self.entries.insert(
            effect_id,
            PreparedEffectCacheEntry {
                key,
                effects: localize_prepared_effects(effects, parent_start_seconds),
            },
        );
    }

    fn generator_topology(
        &self,
        effect_id: u32,
        key: &PreparedEffectCacheKey,
    ) -> Option<Vec<GeneratedChildTopology>> {
        let entry = self.generator_topology_entries.get(&effect_id)?;
        (entry.key == *key).then(|| entry.children.clone())
    }

    fn store_generator_topology(
        &mut self,
        effect_id: u32,
        key: PreparedEffectCacheKey,
        children: &[GeneratedChildTopology],
    ) {
        self.generator_topology_entries.insert(
            effect_id,
            GeneratorTopologyCacheEntry {
                key,
                children: children.to_vec(),
            },
        );
    }

    fn remove_prepared(&mut self, effect_id: u32) {
        self.entries.remove(&effect_id);
    }

    fn remove_topology(&mut self, effect_id: u32) {
        self.generator_topology_entries.remove(&effect_id);
    }

    fn prune(&mut self, active_effect_ids: &HashSet<u32>) {
        self.entries
            .retain(|effect_id, _| active_effect_ids.contains(effect_id));
        self.generator_topology_entries
            .retain(|effect_id, _| active_effect_ids.contains(effect_id));
    }

    pub fn clear(&mut self) {
        self.entries.clear();
        self.generator_topology_entries.clear();
    }

    pub fn prepared_entry_count(&self) -> usize {
        self.entries.len()
    }

    pub fn topology_entry_count(&self) -> usize {
        self.generator_topology_entries.len()
    }
}

#[derive(Debug, Clone, Default)]
pub struct SequenceRenderPlanCache {
    preparation: SequencePreparationCache,
}

impl SequenceRenderPlanCache {
    pub fn clear(&mut self) {
        self.preparation.clear();
    }

    pub fn apply_change_impact(&mut self, impact: &SequenceChangeImpact) {
        if impact.clear_all {
            self.clear();
            return;
        }
        for effect_id in &impact.invalidated_prepared_effect_ids {
            self.preparation.remove_prepared(*effect_id);
        }
        for effect_id in &impact.invalidated_topology_effect_ids {
            self.preparation.remove_topology(*effect_id);
        }
        self.preparation.prune(&impact.active_effect_ids);
    }

    pub fn build_evaluator(
        &mut self,
        project: &DawnProject,
        document: &SequenceEditorDocument,
    ) -> Result<(SequenceRenderPlan, SequenceRenderPlanBuildTiming), String> {
        SequenceRenderPlan::new_with_preparation_cache(project, document, &mut self.preparation)
    }

    pub fn build_evaluator_cancellable(
        &mut self,
        project: &DawnProject,
        document: &SequenceEditorDocument,
        is_cancelled: impl Fn() -> bool,
    ) -> Result<Option<(SequenceRenderPlan, SequenceRenderPlanBuildTiming)>, String> {
        SequenceRenderPlan::new_with_preparation_cache_cancellable(
            project,
            document,
            &mut self.preparation,
            is_cancelled,
        )
    }

    pub fn prepared_entry_count(&self) -> usize {
        self.preparation.prepared_entry_count()
    }

    pub fn topology_entry_count(&self) -> usize {
        self.preparation.topology_entry_count()
    }
}

#[derive(Debug, Clone)]
pub struct SequenceChangeImpact {
    clear_all: bool,
    active_effect_ids: HashSet<u32>,
    invalidated_prepared_effect_ids: HashSet<u32>,
    invalidated_topology_effect_ids: HashSet<u32>,
}

impl SequenceChangeImpact {
    pub fn between(
        previous: &SequenceEditorDocument,
        refreshed: &SequenceEditorDocument,
        project: &DawnProject,
    ) -> Self {
        let active_effect_ids = refreshed
            .effects
            .iter()
            .map(|effect| effect.id)
            .collect::<HashSet<_>>();
        let mut invalidated_prepared_effect_ids = HashSet::new();
        let mut invalidated_topology_effect_ids = HashSet::new();
        let geometry = OutputGeometryModel::from_project(project).ok();
        let previous_effects = previous
            .effects
            .iter()
            .map(|effect| (effect.id, effect))
            .collect::<HashMap<_, _>>();
        let refreshed_effects = refreshed
            .effects
            .iter()
            .map(|effect| (effect.id, effect))
            .collect::<HashMap<_, _>>();

        for effect_id in previous_effects.keys() {
            if !refreshed_effects.contains_key(effect_id) {
                invalidated_prepared_effect_ids.insert(*effect_id);
                invalidated_topology_effect_ids.insert(*effect_id);
            }
        }

        for effect in &refreshed.effects {
            let Some(previous_effect) = previous_effects.get(&effect.id) else {
                invalidated_prepared_effect_ids.insert(effect.id);
                if geometry
                    .as_ref()
                    .and_then(|geometry| sequence_render_effect(project, geometry, effect).ok())
                    .as_ref()
                    .is_some_and(|render| is_generator_render(project, render))
                {
                    invalidated_topology_effect_ids.insert(effect.id);
                }
                continue;
            };
            let impact = effect_change_impact(
                previous,
                refreshed,
                previous_effect,
                effect,
                project,
                geometry.as_ref(),
            );
            if impact.invalidate_prepared {
                invalidated_prepared_effect_ids.insert(effect.id);
            }
            if impact.invalidate_topology {
                invalidated_topology_effect_ids.insert(effect.id);
            }
        }

        for collection_key in changed_mark_collection_keys(previous, refreshed) {
            for effect in effects_referencing_mark_collection(previous, refreshed, &collection_key)
            {
                invalidated_prepared_effect_ids.insert(effect);
            }
        }

        Self {
            clear_all: sequence_source_requires_full_clear(previous, refreshed),
            active_effect_ids,
            invalidated_prepared_effect_ids,
            invalidated_topology_effect_ids,
        }
    }

    pub fn invalidated_prepared_effect_ids(&self) -> &HashSet<u32> {
        &self.invalidated_prepared_effect_ids
    }

    pub fn invalidated_topology_effect_ids(&self) -> &HashSet<u32> {
        &self.invalidated_topology_effect_ids
    }

    pub fn requires_full_clear(&self) -> bool {
        self.clear_all
    }
}

#[derive(Debug, Clone, Copy, Default)]
struct EffectChangeImpact {
    invalidate_prepared: bool,
    invalidate_topology: bool,
}

fn sequence_source_requires_full_clear(
    previous: &SequenceEditorDocument,
    refreshed: &SequenceEditorDocument,
) -> bool {
    previous.path != refreshed.path
        || previous.object_key != refreshed.object_key
        || previous.frame_rate != refreshed.frame_rate
        || previous.degraded != refreshed.degraded
}

fn effect_change_impact(
    previous_document: &SequenceEditorDocument,
    refreshed_document: &SequenceEditorDocument,
    previous: &SequenceEffectDocument,
    refreshed: &SequenceEffectDocument,
    project: &DawnProject,
    geometry: Option<&OutputGeometryModel>,
) -> EffectChangeImpact {
    let mut impact = EffectChangeImpact::default();
    if previous.index != refreshed.index
        || previous.start_seconds != refreshed.start_seconds
        || previous.duration_seconds != refreshed.duration_seconds
        || previous.target != refreshed.target
        || previous.scope != refreshed.scope
        || previous.script != refreshed.script
        || previous.script_source != refreshed.script_source
    {
        impact.invalidate_prepared = true;
        impact.invalidate_topology = true;
    }
    let previous_render =
        geometry.and_then(|geometry| sequence_render_effect(project, geometry, previous).ok());
    let refreshed_render =
        geometry.and_then(|geometry| sequence_render_effect(project, geometry, refreshed).ok());
    match (previous_render.as_ref(), refreshed_render.as_ref()) {
        (Some(previous_render), Some(refreshed_render)) => {
            let previous_prepared_key = prepared_effect_cache_key(
                previous_document,
                previous.start_seconds,
                previous.duration_seconds,
                previous.scope,
                previous_render,
            );
            let refreshed_prepared_key = prepared_effect_cache_key(
                refreshed_document,
                refreshed.start_seconds,
                refreshed.duration_seconds,
                refreshed.scope,
                refreshed_render,
            );
            if previous_prepared_key != refreshed_prepared_key {
                impact.invalidate_prepared = true;
            }
            if generator_topology_key_changed(
                previous_document,
                refreshed_document,
                previous,
                refreshed,
                previous_render,
                refreshed_render,
                project,
            ) {
                impact.invalidate_topology = true;
            }
        }
        (None, Some(refreshed_render)) => {
            impact.invalidate_prepared = true;
            if is_generator_render(project, refreshed_render) {
                impact.invalidate_topology = true;
            }
        }
        (Some(previous_render), None) => {
            impact.invalidate_prepared = true;
            if is_generator_render(project, previous_render) {
                impact.invalidate_topology = true;
            }
        }
        (None, None) => {}
    }
    impact
}

fn generator_topology_key_changed(
    previous_document: &SequenceEditorDocument,
    refreshed_document: &SequenceEditorDocument,
    previous: &SequenceEffectDocument,
    refreshed: &SequenceEffectDocument,
    previous_render: &SequenceRenderEffect,
    refreshed_render: &SequenceRenderEffect,
    project: &DawnProject,
) -> bool {
    let Some(script) = project
        .stores
        .effect_definitions
        .get(&refreshed_render.script)
        .map(|effect| &effect.value.compiled)
    else {
        return false;
    };
    if script.kind() != EffectScriptKind::Generator {
        return false;
    }
    let topology_param_names = script
        .params()
        .iter()
        .map(|param| param.name.clone())
        .collect::<BTreeSet<_>>();
    let previous_key = prepared_effect_cache_key_for_params(
        previous_document,
        previous.start_seconds,
        previous.duration_seconds,
        previous.scope,
        previous_render,
        Some(&topology_param_names),
    );
    let refreshed_key = prepared_effect_cache_key_for_params(
        refreshed_document,
        refreshed.start_seconds,
        refreshed.duration_seconds,
        refreshed.scope,
        refreshed_render,
        Some(&topology_param_names),
    );
    previous_key != refreshed_key
}

fn is_generator_render(project: &DawnProject, render: &SequenceRenderEffect) -> bool {
    project
        .stores
        .effect_definitions
        .get(&render.script)
        .map(|effect| &effect.value.compiled)
        .is_some_and(|script| script.kind() == EffectScriptKind::Generator)
}

fn changed_mark_collection_keys(
    previous: &SequenceEditorDocument,
    refreshed: &SequenceEditorDocument,
) -> HashSet<String> {
    let previous_collections = previous
        .mark_collections
        .iter()
        .map(|collection| (collection.key.as_str(), collection))
        .collect::<HashMap<_, _>>();
    let refreshed_collections = refreshed
        .mark_collections
        .iter()
        .map(|collection| (collection.key.as_str(), collection))
        .collect::<HashMap<_, _>>();
    let mut changed = HashSet::new();
    for key in previous_collections.keys() {
        match refreshed_collections.get(key) {
            Some(refreshed_collection)
                if previous_collections[key].marks_seconds
                    == refreshed_collection.marks_seconds
                    && previous_collections[key].name == refreshed_collection.name
                    && previous_collections[key].color == refreshed_collection.color => {}
            _ => {
                changed.insert((*key).to_string());
            }
        }
    }
    for key in refreshed_collections.keys() {
        if !previous_collections.contains_key(key) {
            changed.insert((*key).to_string());
        }
    }
    changed
}

fn effects_referencing_mark_collection(
    previous: &SequenceEditorDocument,
    refreshed: &SequenceEditorDocument,
    collection_key: &str,
) -> HashSet<u32> {
    previous
        .effects
        .iter()
        .chain(refreshed.effects.iter())
        .filter(|effect| effect_references_mark_collection(effect, collection_key))
        .map(|effect| effect.id)
        .collect()
}

fn effect_references_mark_collection(
    effect: &SequenceEffectDocument,
    collection_key: &str,
) -> bool {
    effect_params_reference_mark_collection(&effect.params, collection_key)
}

fn effect_params_reference_mark_collection(
    params: &[SequenceEffectParamDocument],
    collection_key: &str,
) -> bool {
    params.iter().any(|param| {
        matches!(
            &param.value,
            EffectParam::<Resolved>::Marks { key } if key == collection_key
        )
    })
}

#[derive(Debug, Clone)]
struct PreparedEffectCacheEntry {
    key: PreparedEffectCacheKey,
    effects: Vec<PreparedSequenceEffect>,
}

#[derive(Debug, Clone)]
struct GeneratorTopologyCacheEntry {
    key: PreparedEffectCacheKey,
    children: Vec<GeneratedChildTopology>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct PreparedEffectCacheKey {
    script_id: EffectDefinitionKey,
    script_source: String,
    scope: SequenceEffectScope,
    duration_seconds: F64CacheKey,
    params: Vec<PreparedEffectParamCacheKey>,
    target_pixels: Vec<PreparedEffectPixelCacheKey>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct PreparedEffectParamCacheKey {
    name: String,
    value: EffectParamCacheValue,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
enum EffectParamCacheValue {
    Integer(u64),
    Float(F64CacheKey),
    Boolean(bool),
    Enum(String),
    Flags(Vec<String>),
    Color(ColorCacheKey),
    Curve(CurveCacheKey),
    Array(Vec<EffectParamArrayValueCacheKey>),
    Marks {
        collection_key: String,
        windowed_seconds: Option<Vec<F64CacheKey>>,
        global_seconds: Option<Vec<F64CacheKey>>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
enum EffectParamArrayValueCacheKey {
    Integer(u64),
    Float(F64CacheKey),
    Boolean(bool),
    Color(ColorCacheKey),
    Curve(CurveCacheKey),
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct CurveCacheKey {
    value_type: CurveValueTypeCacheKey,
    points: Vec<CurvePointCacheKey>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum CurveValueTypeCacheKey {
    Float,
    Color,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct CurvePointCacheKey {
    time: F64CacheKey,
    value: CurveValueCacheKey,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
enum CurveValueCacheKey {
    Float(F64CacheKey),
    Color(ColorCacheKey),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct ColorCacheKey {
    red: u8,
    green: u8,
    blue: u8,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct PreparedEffectPixelCacheKey {
    fixture_index: usize,
    pixel_index: usize,
    pixel_count: usize,
}

#[derive(Debug, Clone, Copy)]
struct F64CacheKey(f64);

impl PartialEq for F64CacheKey {
    fn eq(&self, other: &Self) -> bool {
        normalized_f64_bits(self.0) == normalized_f64_bits(other.0)
    }
}

impl Eq for F64CacheKey {}

impl Hash for F64CacheKey {
    fn hash<H: Hasher>(&self, state: &mut H) {
        normalized_f64_bits(self.0).hash(state);
    }
}

#[derive(Debug, Clone)]
pub struct SequenceRenderPlan {
    source: OutputSourceMetadata,
    geometry: OutputGeometryModel,
    effects: Vec<PreparedSequenceEffect>,
    effect_indices_by_frame: Vec<Vec<usize>>,
}

impl SequenceRenderPlan {
    pub fn new(project: &DawnProject, document: &SequenceEditorDocument) -> Result<Self, String> {
        Self::new_timed(project, document).map(|(evaluator, _timing)| evaluator)
    }

    pub fn new_timed(
        project: &DawnProject,
        document: &SequenceEditorDocument,
    ) -> Result<(Self, SequenceRenderPlanBuildTiming), String> {
        Self::new_timed_with_cache(project, document, None)
    }

    pub(crate) fn new_with_preparation_cache(
        project: &DawnProject,
        document: &SequenceEditorDocument,
        preparation_cache: &mut SequencePreparationCache,
    ) -> Result<(Self, SequenceRenderPlanBuildTiming), String> {
        Self::new_timed_with_cache(project, document, Some(preparation_cache))
    }

    pub(crate) fn new_with_preparation_cache_cancellable(
        project: &DawnProject,
        document: &SequenceEditorDocument,
        preparation_cache: &mut SequencePreparationCache,
        is_cancelled: impl Fn() -> bool,
    ) -> Result<Option<(Self, SequenceRenderPlanBuildTiming)>, String> {
        Self::new_timed_with_cache_cancellable(
            project,
            document,
            Some(preparation_cache),
            &is_cancelled,
        )
    }

    fn new_timed_with_cache(
        project: &DawnProject,
        document: &SequenceEditorDocument,
        preparation_cache: Option<&mut SequencePreparationCache>,
    ) -> Result<(Self, SequenceRenderPlanBuildTiming), String> {
        Self::new_timed_with_cache_cancellable(project, document, preparation_cache, &|| false)
            .and_then(|result| {
                result.ok_or_else(|| "Sequence renderer build was cancelled".to_string())
            })
    }

    fn new_timed_with_cache_cancellable(
        project: &DawnProject,
        document: &SequenceEditorDocument,
        mut preparation_cache: Option<&mut SequencePreparationCache>,
        is_cancelled: &dyn Fn() -> bool,
    ) -> Result<Option<(Self, SequenceRenderPlanBuildTiming)>, String> {
        let total_started = Instant::now();
        if is_cancelled() {
            return Ok(None);
        }
        let layout_started = Instant::now();
        let geometry = OutputGeometryModel::from_project(project)?;
        let fixture_templates = geometry.fixtures.clone();
        let layout_template_ms = elapsed_ms(layout_started);

        let mut effects = Vec::new();
        let mut authored_sample_ms = 0.0;
        let mut generator_expansion_ms = 0.0;
        let mut generator_parent_count = 0;
        let mut generated_child_count = 0;
        let mut generator_parents = Vec::new();
        if let Some(cache) = preparation_cache.as_deref_mut() {
            let active_effect_ids = document
                .effects
                .iter()
                .map(|effect| effect.id)
                .collect::<HashSet<_>>();
            cache.prune(&active_effect_ids);
        }
        for effect in &document.effects {
            if is_cancelled() {
                return Ok(None);
            }
            let render = match sequence_render_effect(project, &geometry, effect) {
                Ok(render) => render,
                Err(error) => {
                    effects.push(diagnostic_prepared_effect(
                        effect,
                        error.message,
                        &error.diagnostic_pixels,
                        effect.scope,
                        &fixture_templates,
                    ));
                    continue;
                }
            };
            let cache_key = prepared_effect_cache_key(
                document,
                effect.start_seconds,
                effect.duration_seconds,
                effect.scope,
                &render,
            );
            let script_id = render.script.clone();
            match project
                .stores
                .effect_definitions
                .get(&script_id)
                .map(|effect| &effect.value.compiled)
            {
                Some(script) if script.kind() == EffectScriptKind::Generator => {
                    generator_parent_count += 1;
                    let generator_started = Instant::now();
                    let mut prepared_cache_hit = false;
                    let mut topology_cache_hit = false;
                    let prepared = match preparation_cache.as_deref().and_then(|cache| {
                        cache.prepared_effects(effect.id, &cache_key, effect.start_seconds)
                    }) {
                        Some(prepared) => {
                            prepared_cache_hit = true;
                            prepared
                        }
                        None => {
                            let topology_param_names = script
                                .params()
                                .iter()
                                .map(|param| param.name.clone())
                                .collect::<BTreeSet<_>>();
                            let topology_cache_key = prepared_effect_cache_key_for_params(
                                document,
                                effect.start_seconds,
                                effect.duration_seconds,
                                effect.scope,
                                &render,
                                Some(&topology_param_names),
                            );
                            let prepared_result =
                                match preparation_cache.as_deref().and_then(|cache| {
                                    cache.generator_topology(effect.id, &topology_cache_key)
                                }) {
                                    Some(children) => {
                                        topology_cache_hit = true;
                                        prepare_generated_effects_from_topology(
                                            GeneratedEffectTopologyInput {
                                                project,
                                                parent_path: parent_path_for_render(&render),
                                                parent_id: effect.id,
                                                parent_start_seconds: effect.start_seconds,
                                                parent_duration_seconds: effect.duration_seconds,
                                                generator_id: script_id.clone(),
                                                generator: script,
                                                render: &render,
                                                mark_collections: &document.mark_collections,
                                                fixture_templates: &fixture_templates,
                                                children,
                                                is_cancelled,
                                            },
                                        )
                                    }
                                    None => {
                                        if is_cancelled() {
                                            return Ok(None);
                                        }
                                        let topology = prepare_generated_topology(
                                            project,
                                            document,
                                            effect.start_seconds,
                                            effect.duration_seconds,
                                            effect.scope,
                                            script,
                                            &render,
                                        );
                                        match topology {
                                            Ok(children) => {
                                                if let Some(cache) =
                                                    preparation_cache.as_deref_mut()
                                                {
                                                    cache.store_generator_topology(
                                                        effect.id,
                                                        topology_cache_key,
                                                        &children,
                                                    );
                                                }
                                                prepare_generated_effects_from_topology(
                                                    GeneratedEffectTopologyInput {
                                                        project,
                                                        parent_path: parent_path_for_render(
                                                            &render,
                                                        ),
                                                        parent_id: effect.id,
                                                        parent_start_seconds: effect.start_seconds,
                                                        parent_duration_seconds: effect
                                                            .duration_seconds,
                                                        generator_id: script_id.clone(),
                                                        generator: script,
                                                        render: &render,
                                                        mark_collections: &document
                                                            .mark_collections,
                                                        fixture_templates: &fixture_templates,
                                                        children,
                                                        is_cancelled,
                                                    },
                                                )
                                            }
                                            Err(error) => Err(error),
                                        }
                                    }
                                };
                            let prepared = match prepared_result {
                                Ok(prepared) => prepared,
                                Err(error) => {
                                    let prepared = vec![diagnostic_prepared_effect(
                                        effect,
                                        error.to_string(),
                                        &render.target_pixels,
                                        effect.scope,
                                        &fixture_templates,
                                    )];
                                    generator_expansion_ms += elapsed_ms(generator_started);
                                    let child_count = prepared_generated_child_count(&prepared);
                                    generated_child_count += child_count;
                                    generator_parents.push(GeneratorParentPreparationTiming {
                                        parent_effect_id: effect.id,
                                        script_id: script_id.clone(),
                                        target_pixels: render.target_pixels.len(),
                                        emitted_children: child_count,
                                        prepared_children: child_count,
                                        prepared_cache_hit,
                                        topology_cache_hit,
                                        total_prepare_ms: elapsed_ms(generator_started),
                                    });
                                    effects.extend(prepared);
                                    continue;
                                }
                            };
                            if let Some(cache) = preparation_cache.as_deref_mut() {
                                cache.store(effect.id, cache_key, effect.start_seconds, &prepared);
                            }
                            prepared
                        }
                    };
                    let child_count = prepared_generated_child_count(&prepared);
                    let total_prepare_ms = elapsed_ms(generator_started);
                    generator_expansion_ms += total_prepare_ms;
                    generated_child_count += child_count;
                    generator_parents.push(GeneratorParentPreparationTiming {
                        parent_effect_id: effect.id,
                        script_id: script_id.clone(),
                        target_pixels: render.target_pixels.len(),
                        emitted_children: child_count,
                        prepared_children: child_count,
                        prepared_cache_hit,
                        topology_cache_hit,
                        total_prepare_ms,
                    });
                    effects.extend(prepared);
                }
                Some(script) => {
                    let sample_started = Instant::now();
                    if let Some(prepared) = preparation_cache.as_deref().and_then(|cache| {
                        cache.prepared_effects(effect.id, &cache_key, effect.start_seconds)
                    }) {
                        authored_sample_ms += elapsed_ms(sample_started);
                        effects.extend(prepared);
                        continue;
                    }
                    let prepared = vec![PreparedSequenceEffect {
                        start_seconds: effect.start_seconds,
                        duration_seconds: effect.duration_seconds,
                        authored: true,
                        render: match prepare_sample_render(SampleRenderPreparationInput {
                            project,
                            script_id: script_id.clone(),
                            script_source: render.script_source.clone(),
                            script,
                            params: &render.params,
                            mark_collections: &document.mark_collections,
                            effect_start_seconds: effect.start_seconds,
                            effect_duration_seconds: effect.duration_seconds,
                            scope: effect.scope,
                            target_pixels: &render.target_pixels,
                            fixture_templates: &fixture_templates,
                        }) {
                            Ok(render) => render,
                            Err(error) => diagnostic_render(
                                error.to_string(),
                                &render.target_pixels,
                                effect.scope,
                                &fixture_templates,
                            ),
                        },
                    }];
                    if let Some(cache) = preparation_cache.as_deref_mut() {
                        cache.store(effect.id, cache_key, effect.start_seconds, &prepared);
                    }
                    authored_sample_ms += elapsed_ms(sample_started);
                    effects.extend(prepared);
                }
                None => {
                    effects.push(diagnostic_prepared_effect(
                        effect,
                        format!(
                            "compiled script `{}` was not found",
                            script_id.display_key()
                        ),
                        &render.target_pixels,
                        effect.scope,
                        &fixture_templates,
                    ));
                }
            }
        }
        if is_cancelled() {
            return Ok(None);
        }

        let source = OutputSourceMetadata {
            label: format!("Sequence {}", document.object_key),
            kind: OutputSourceKind::Sequence,
            duration_seconds: document.duration_seconds,
            fps: document.frame_rate,
        };
        let timeline_started = Instant::now();
        let effect_indices_by_frame =
            build_effect_indices_by_frame(&effects, source.duration_seconds, source.fps);
        let timeline_index_ms = elapsed_ms(timeline_started);
        let timing = SequenceRenderPlanBuildTiming {
            total_ms: elapsed_ms(total_started),
            layout_template_ms,
            authored_sample_ms,
            generator_expansion_ms,
            timeline_index_ms,
            prepared_effect_count: effects.len(),
            generator_parent_count,
            generated_child_count,
            generator_parents,
        };

        Ok(Some((
            Self {
                source,
                geometry,
                effects,
                effect_indices_by_frame,
            },
            timing,
        )))
    }

    pub fn geometry(&self) -> &OutputGeometryModel {
        &self.geometry
    }

    pub fn render_frame(&mut self, time_seconds: f64, generation: u64) -> RenderedOutputFrame {
        self.render_frame_timed(time_seconds, generation).0
    }

    pub fn render_frame_timed(
        &mut self,
        time_seconds: f64,
        generation: u64,
    ) -> (RenderedOutputFrame, SequenceFrameRenderTiming) {
        match self
            .render_frame_timed_cancellable_with_diagnostics(time_seconds, generation, || false)
        {
            Some((frame, timing, _diagnostics)) => (frame, timing),
            None => (
                self.rendered_frame(
                    time_seconds,
                    generation,
                    OutputFrameStatus::Live,
                    self.geometry.fixtures.clone(),
                ),
                SequenceFrameRenderTiming::default(),
            ),
        }
    }

    pub fn render_frame_timed_cancellable(
        &mut self,
        time_seconds: f64,
        generation: u64,
        is_cancelled: impl Fn() -> bool,
    ) -> Option<(RenderedOutputFrame, SequenceFrameRenderTiming)> {
        self.render_timed_cancellable(time_seconds, generation, is_cancelled)
    }

    pub(crate) fn render_frame_timed_with_diagnostics(
        &mut self,
        time_seconds: f64,
        generation: u64,
    ) -> (RenderedOutputFrame, SequenceFrameRenderTiming, Vec<String>) {
        match self
            .render_frame_timed_cancellable_with_diagnostics(time_seconds, generation, || false)
        {
            Some(result) => result,
            None => (
                self.rendered_frame(
                    time_seconds,
                    generation,
                    OutputFrameStatus::Live,
                    self.geometry.fixtures.clone(),
                ),
                SequenceFrameRenderTiming::default(),
                Vec::new(),
            ),
        }
    }

    fn render_timed_cancellable(
        &mut self,
        time_seconds: f64,
        generation: u64,
        is_cancelled: impl Fn() -> bool,
    ) -> Option<(RenderedOutputFrame, SequenceFrameRenderTiming)> {
        self.render_frame_timed_cancellable_with_diagnostics(time_seconds, generation, is_cancelled)
            .map(|(frame, timing, _diagnostics)| (frame, timing))
    }

    fn render_frame_timed_cancellable_with_diagnostics(
        &mut self,
        time_seconds: f64,
        generation: u64,
        is_cancelled: impl Fn() -> bool,
    ) -> Option<(RenderedOutputFrame, SequenceFrameRenderTiming, Vec<String>)> {
        let total_started = Instant::now();
        let clone_started = Instant::now();
        let mut fixtures = self.geometry.fixtures.clone();
        let render_buffer_clone_ms = elapsed_ms(clone_started);
        let status = OutputFrameStatus::Live;
        let mut counters = SequenceEffectEvaluationCounters::default();
        let mut sample_reuse = SequenceFrameSampleReuse::default();
        let mut diagnostics = Vec::new();

        let effect_loop_started = Instant::now();
        if let Some(effect_indices) = self.effect_indices_for_time(time_seconds) {
            for effect_index in effect_indices.clone() {
                if is_cancelled() {
                    return None;
                }
                let mut context = PreparedEffectEvaluationContext {
                    fixtures: &mut fixtures,
                    counters: &mut counters,
                    sample_reuse: &mut sample_reuse,
                    diagnostics: &mut diagnostics,
                    is_cancelled: &is_cancelled,
                };
                evaluate_prepared_effect_at_time(
                    &mut self.effects[effect_index],
                    time_seconds,
                    &mut context,
                );
                if is_cancelled() {
                    return None;
                }
            }
        }
        let effect_loop_ms = elapsed_ms(effect_loop_started);

        let output_started = Instant::now();
        let frame = self.rendered_frame(time_seconds, generation, status, fixtures);
        let rgb_buffer_ms = elapsed_ms(output_started);
        let total_ms = elapsed_ms(total_started);
        Some((
            frame,
            SequenceFrameRenderTiming {
                total_ms,
                render_buffer_clone_ms,
                effect_loop_ms,
                rgb_buffer_ms,
                active_effects: counters.active_prepared_effects,
                active_authored_effects: counters.active_authored_effects,
                active_prepared_effects: counters.active_prepared_effects,
                visited_prepared_effects: counters.visited_prepared_effects,
                sampled_pixels: counters.sampled_pixels,
                vm_sample_evaluations: counters.vm_sample_evaluations,
                sample_reuse_saved_evaluations: counters.sample_reuse_saved_evaluations,
                sample_reuse_group_hits: counters.sample_reuse_group_hits,
            },
            diagnostics,
        ))
    }

    pub fn prepared_effect_count(&self) -> usize {
        self.effects.len()
    }

    fn effect_indices_for_time(&self, time_seconds: f64) -> Option<&Vec<usize>> {
        if !time_seconds.is_finite() || time_seconds < 0.0 {
            return None;
        }
        let frame_index = floor_frame(time_from_seconds_clamped(time_seconds), self.source.fps);
        self.effect_indices_by_frame
            .get(usize::try_from(frame_index).ok()?)
    }

    fn rendered_frame(
        &self,
        time_seconds: f64,
        generation: u64,
        status: OutputFrameStatus,
        fixtures: Vec<OutputFixtureFrame>,
    ) -> RenderedOutputFrame {
        RenderedOutputFrame {
            geometry_id: self.geometry.geometry_id.clone(),
            time_seconds,
            generation,
            status,
            rgb: rgb_from_fixtures(&fixtures),
        }
    }
}

fn rgb_from_fixtures(fixtures: &[OutputFixtureFrame]) -> Vec<u8> {
    fixtures
        .iter()
        .flat_map(|fixture| {
            fixture
                .pixels
                .iter()
                .flat_map(|pixel| [pixel.color.red, pixel.color.green, pixel.color.blue])
        })
        .collect()
}

#[derive(Debug, Clone, Copy, Default)]
struct SequenceEffectEvaluationCounters {
    active_authored_effects: u32,
    active_prepared_effects: u32,
    visited_prepared_effects: u32,
    sampled_pixels: u32,
    vm_sample_evaluations: u32,
    sample_reuse_saved_evaluations: u32,
    sample_reuse_group_hits: u32,
}

#[derive(Debug, Default)]
struct SequenceFrameSampleReuse {
    effect_indices: HashMap<SampleReuseEffectKey, usize>,
    colors: HashMap<SampleReuseKey, Color>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct SampleReuseKey {
    effect_index: usize,
    local_seconds: F64CacheKey,
    progress: F64CacheKey,
    pixel_index: usize,
    pixel_count: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct SampleReuseEffectKey {
    script_id: EffectDefinitionKey,
    script_source: String,
    prepared_params: Vec<RuntimeValueCacheKey>,
}

struct PreparedEffectEvaluationContext<'a> {
    fixtures: &'a mut [OutputFixtureFrame],
    counters: &'a mut SequenceEffectEvaluationCounters,
    sample_reuse: &'a mut SequenceFrameSampleReuse,
    diagnostics: &'a mut Vec<String>,
    is_cancelled: &'a dyn Fn() -> bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
enum RuntimeValueCacheKey {
    Float(F64CacheKey),
    Int(i64),
    Bool(bool),
    Color(ColorCacheKey),
    Marks {
        windowed: Vec<F64CacheKey>,
        global: Vec<F64CacheKey>,
    },
    Curve(CurveCacheKey),
    Array(Vec<RuntimeValueCacheKey>),
    Enum(String),
    Flags(Vec<String>),
    Fixture(usize),
    Pixel {
        index: usize,
        count: usize,
    },
}

impl SequenceFrameSampleReuse {
    fn effect_index(&mut self, key: &SampleReuseEffectKey) -> usize {
        let next_index = self.effect_indices.len();
        *self.effect_indices.entry(key.clone()).or_insert(next_index)
    }
}

fn evaluate_prepared_effect_at_time(
    effect: &mut PreparedSequenceEffect,
    time_seconds: f64,
    context: &mut PreparedEffectEvaluationContext<'_>,
) {
    context.counters.visited_prepared_effects =
        context.counters.visited_prepared_effects.saturating_add(1);
    let local_seconds =
        if time_seconds < effect.start_seconds || time_seconds >= effect.end_seconds() {
            return;
        } else {
            time_seconds - effect.start_seconds
        };
    sample_prepared_effect(effect, local_seconds, context);
}

fn sample_prepared_effect(
    effect: &mut PreparedSequenceEffect,
    local_seconds: f64,
    context: &mut PreparedEffectEvaluationContext<'_>,
) {
    let progress = if effect.duration_seconds == 0.0 {
        0.0
    } else {
        (local_seconds / effect.duration_seconds).clamp(0.0, 1.0)
    };

    if effect.authored {
        context.counters.active_authored_effects =
            context.counters.active_authored_effects.saturating_add(1);
    }
    context.counters.active_prepared_effects =
        context.counters.active_prepared_effects.saturating_add(1);

    let PreparedEffectRender::Ready {
        script,
        target_pixels,
        target_pixel_groups,
        prepared_params,
        sample_reuse_effect_key,
        scratch,
        ..
    } = &mut effect.render
    else {
        let PreparedEffectRender::Error { message } = &effect.render else {
            unreachable!("prepared effect render variants are exhaustive")
        };
        context.diagnostics.push(message.clone());
        return;
    };

    context.counters.sampled_pixels = context
        .counters
        .sampled_pixels
        .saturating_add(target_pixels.len().min(u32::MAX as usize) as u32);

    let effect_index = context.sample_reuse.effect_index(sample_reuse_effect_key);
    for group in target_pixel_groups {
        if (context.is_cancelled)() {
            return;
        }
        let cache_key = SampleReuseKey {
            effect_index,
            local_seconds: F64CacheKey(local_seconds),
            progress: F64CacheKey(progress),
            pixel_index: group.pixel_context.index,
            pixel_count: group.pixel_context.count,
        };
        let sampled = match context.sample_reuse.colors.get(&cache_key).copied() {
            Some(color) => {
                context.counters.sample_reuse_saved_evaluations = context
                    .counters
                    .sample_reuse_saved_evaluations
                    .saturating_add(group.output_pixel_indices.len().min(u32::MAX as usize) as u32);
                context.counters.sample_reuse_group_hits =
                    context.counters.sample_reuse_group_hits.saturating_add(1);
                Ok(color)
            }
            None => {
                let pixel = &target_pixels[group.first_pixel_index];
                context.counters.vm_sample_evaluations =
                    context.counters.vm_sample_evaluations.saturating_add(1);
                context.counters.sample_reuse_saved_evaluations = context
                    .counters
                    .sample_reuse_saved_evaluations
                    .saturating_add(
                        group
                            .output_pixel_indices
                            .len()
                            .saturating_sub(1)
                            .min(u32::MAX as usize) as u32,
                    );
                script
                    .sample_prepared_with_scratch(
                        progress,
                        local_seconds,
                        pixel.fixture_context,
                        group.pixel_context,
                        prepared_params,
                        scratch,
                    )
                    .inspect(|color| {
                        context.sample_reuse.colors.insert(cache_key, *color);
                    })
            }
        };
        match sampled {
            Ok(color) => {
                for pixel_index in &group.output_pixel_indices {
                    let pixel = &target_pixels[*pixel_index];
                    add_clamped(
                        &mut context.fixtures[pixel.fixture_index].pixels[pixel.pixel_index].color,
                        color,
                    );
                }
            }
            Err(error) => {
                context.diagnostics.push(error.to_string());
                return;
            }
        }
    }
}

struct SampleRenderPreparationInput<'a> {
    project: &'a DawnProject,
    script_id: EffectDefinitionKey,
    script_source: String,
    script: &'a CompiledEffect,
    params: &'a [SequenceEffectParamDocument],
    mark_collections: &'a [SequenceMarkCollectionDocument],
    effect_start_seconds: f64,
    effect_duration_seconds: f64,
    scope: SequenceEffectScope,
    target_pixels: &'a [SequenceRenderPixel],
    fixture_templates: &'a [OutputFixtureFrame],
}

fn prepare_sample_render(
    input: SampleRenderPreparationInput<'_>,
) -> Result<PreparedEffectRender, RuntimeError> {
    let prepared_params = prepare_params_from_document(
        input.project,
        input.script,
        input.params,
        input.mark_collections,
        input.effect_start_seconds,
        input.effect_duration_seconds,
    )?;
    let sample_reuse_effect_key =
        sample_reuse_effect_key(input.script_id, input.script_source, &prepared_params);
    let target_pixels =
        prepare_effect_pixels(input.scope, input.target_pixels, input.fixture_templates);
    let target_pixel_groups = prepare_effect_pixel_groups(&target_pixels);
    Ok(PreparedEffectRender::Ready {
        script: Box::new(input.script.clone()),
        target_pixels,
        target_pixel_groups,
        prepared_params: Box::new(prepared_params),
        sample_reuse_effect_key: Box::new(sample_reuse_effect_key),
        scratch: Box::new(EffectSampleScratch::new(input.script.bytecode_stats())),
        _bytecode_stats: input.script.bytecode_stats(),
    })
}

fn sample_reuse_effect_key(
    script_id: EffectDefinitionKey,
    script_source: String,
    prepared_params: &PreparedEffectParams,
) -> SampleReuseEffectKey {
    SampleReuseEffectKey {
        script_id,
        script_source,
        prepared_params: prepared_params_cache_key(prepared_params),
    }
}

fn sequence_render_effect(
    project: &DawnProject,
    geometry: &OutputGeometryModel,
    effect: &SequenceEffectDocument,
) -> Result<SequenceRenderEffect, EffectDiagnostic> {
    let target_pixels =
        geometry
            .target_pixels(&effect.target)
            .map_err(|message| EffectDiagnostic {
                message,
                diagnostic_pixels: geometry.all_pixels(),
            })?;
    let script = effect
        .script_source
        .as_ref()
        .ok_or_else(|| EffectDiagnostic {
            message: format!("effect `{}` is missing a script binding", effect.id),
            diagnostic_pixels: target_pixels.clone(),
        })?;
    let script_id = effect_definition_key(project, script);
    let script_source = effect
        .script_text
        .clone()
        .unwrap_or_else(|| script_id.display_key());
    let params =
        resolve_effect_params(project, &effect.params).map_err(|error| EffectDiagnostic {
            message: error.to_string(),
            diagnostic_pixels: target_pixels.clone(),
        })?;
    Ok(SequenceRenderEffect {
        script: script_id,
        script_source,
        params,
        target_pixels,
    })
}

#[derive(Debug, Clone)]
struct EffectDiagnostic {
    message: String,
    diagnostic_pixels: Vec<SequenceRenderPixel>,
}

fn effect_definition_key(
    project: &DawnProject,
    script: &EffectScriptReferenceDocument,
) -> EffectDefinitionKey {
    project
        .stores
        .effect_definitions
        .keys()
        .find(|key| key.path.to_slash_string() == script.path && key.name == script.effect_name)
        .cloned()
        .unwrap_or_else(|| {
            EffectDefinitionKey::new(
                Utf8PathBuf::from(script.path.as_str()),
                script.effect_name.clone(),
            )
        })
}

fn prepared_effect_cache_key(
    document: &SequenceEditorDocument,
    effect_start_seconds: f64,
    duration_seconds: f64,
    scope: SequenceEffectScope,
    render: &SequenceRenderEffect,
) -> PreparedEffectCacheKey {
    prepared_effect_cache_key_for_params(
        document,
        effect_start_seconds,
        duration_seconds,
        scope,
        render,
        None,
    )
}

fn prepared_effect_cache_key_for_params(
    document: &SequenceEditorDocument,
    effect_start_seconds: f64,
    duration_seconds: f64,
    scope: SequenceEffectScope,
    render: &SequenceRenderEffect,
    included_params: Option<&BTreeSet<String>>,
) -> PreparedEffectCacheKey {
    PreparedEffectCacheKey {
        script_id: render.script.clone(),
        script_source: render.script_source.clone(),
        scope,
        duration_seconds: F64CacheKey(duration_seconds),
        params: render
            .params
            .iter()
            .filter(|param| {
                included_params
                    .map(|names| names.contains(&param.name))
                    .unwrap_or(true)
            })
            .map(|param| PreparedEffectParamCacheKey {
                name: param.name.clone(),
                value: effect_param_cache_value(
                    &param.value,
                    &document.mark_collections,
                    effect_start_seconds,
                    duration_seconds,
                ),
            })
            .collect(),
        target_pixels: render
            .target_pixels
            .iter()
            .map(|pixel| PreparedEffectPixelCacheKey {
                fixture_index: pixel.fixture_index,
                pixel_index: pixel.pixel_index,
                pixel_count: pixel.pixel_count,
            })
            .collect(),
    }
}

fn parent_path_for_render(render: &SequenceRenderEffect) -> Utf8PathBuf {
    Utf8PathBuf::from(&render.script.path)
}

fn effect_param_cache_value(
    param: &EffectParam<Resolved>,
    mark_collections: &[SequenceMarkCollectionDocument],
    effect_start_seconds: f64,
    effect_duration_seconds: f64,
) -> EffectParamCacheValue {
    match param {
        EffectParam::Integer { value } => EffectParamCacheValue::Integer(*value),
        EffectParam::Float { value } => EffectParamCacheValue::Float(F64CacheKey(*value)),
        EffectParam::Boolean { value } => EffectParamCacheValue::Boolean(*value),
        EffectParam::Enum { value } => EffectParamCacheValue::Enum(value.clone()),
        EffectParam::Flags { value } => EffectParamCacheValue::Flags(value.values.clone()),
        EffectParam::Color { value } => EffectParamCacheValue::Color(color_cache_key(*value)),
        EffectParam::Curve { curve } => effect_param_curve_cache_value(curve),
        EffectParam::Array { values, .. } => EffectParamCacheValue::Array(
            values
                .iter()
                .map(effect_param_array_value_cache_key)
                .collect(),
        ),
        EffectParam::Marks { key } => {
            let global_seconds = mark_collections
                .iter()
                .find(|collection| collection.key == *key)
                .map(|collection| {
                    sorted_local_mark_cache_keys(&collection.marks_seconds, effect_start_seconds)
                });
            let windowed_seconds = global_seconds.as_ref().map(|marks| {
                marks
                    .iter()
                    .copied()
                    .filter(|mark| mark.0 >= 0.0 && mark.0 < effect_duration_seconds)
                    .collect::<Vec<_>>()
            });
            EffectParamCacheValue::Marks {
                collection_key: key.clone(),
                windowed_seconds,
                global_seconds,
            }
        }
    }
}

fn sorted_local_mark_cache_keys(
    marks_seconds: &[f64],
    effect_start_seconds: f64,
) -> Vec<F64CacheKey> {
    let mut marks = marks_seconds
        .iter()
        .map(|mark_seconds| F64CacheKey(*mark_seconds - effect_start_seconds))
        .collect::<Vec<_>>();
    marks.sort_by(|left, right| left.0.total_cmp(&right.0));
    marks
}

fn effect_param_array_value_cache_key(
    value: &dawn_project::EffectParamArrayValue<Resolved>,
) -> EffectParamArrayValueCacheKey {
    match value {
        dawn_project::EffectParamArrayValue::Integer(value) => {
            EffectParamArrayValueCacheKey::Integer(*value)
        }
        dawn_project::EffectParamArrayValue::Float(value) => {
            EffectParamArrayValueCacheKey::Float(F64CacheKey(*value))
        }
        dawn_project::EffectParamArrayValue::Boolean(value) => {
            EffectParamArrayValueCacheKey::Boolean(*value)
        }
        dawn_project::EffectParamArrayValue::Color(value) => {
            EffectParamArrayValueCacheKey::Color(color_cache_key(*value))
        }
        dawn_project::EffectParamArrayValue::Curve(curve) => {
            effect_param_array_curve_cache_key(curve)
        }
    }
}

fn effect_param_curve_cache_value(
    curve: &dawn_project::CurveUse<Resolved>,
) -> EffectParamCacheValue {
    match &curve.curve {
        ResolvedInlineOrRef::Inline(curve) => EffectParamCacheValue::Curve(curve_cache_key(curve)),
        ResolvedInlineOrRef::Ref(_) => unreachable!("curve refs are resolved before cache keying"),
    }
}

fn effect_param_array_curve_cache_key(
    curve: &dawn_project::CurveUse<Resolved>,
) -> EffectParamArrayValueCacheKey {
    match &curve.curve {
        ResolvedInlineOrRef::Inline(curve) => {
            EffectParamArrayValueCacheKey::Curve(curve_cache_key(curve))
        }
        ResolvedInlineOrRef::Ref(_) => unreachable!("curve refs are resolved before cache keying"),
    }
}

fn curve_cache_key(curve: &Curve) -> CurveCacheKey {
    CurveCacheKey {
        value_type: match curve.value_type {
            CurveValueType::Float => CurveValueTypeCacheKey::Float,
            CurveValueType::Color => CurveValueTypeCacheKey::Color,
        },
        points: curve
            .points
            .iter()
            .map(|point| CurvePointCacheKey {
                time: F64CacheKey(point.time),
                value: match &point.value {
                    CurveValue::Float(value) => CurveValueCacheKey::Float(F64CacheKey(*value)),
                    CurveValue::Color(value) => CurveValueCacheKey::Color(color_cache_key(*value)),
                },
            })
            .collect(),
    }
}

fn curve_for_runtime<'a>(
    project: &'a DawnProject,
    curve: &'a dawn_project::CurveUse<Resolved>,
) -> Result<&'a Curve, RuntimeError> {
    match &curve.curve {
        ResolvedInlineOrRef::Inline(curve) => Ok(curve),
        ResolvedInlineOrRef::Ref(reference) => project
            .stores
            .curves
            .get(&reference.key)
            .map(|curve| &curve.value)
            .ok_or_else(|| RuntimeError {
                message: format!("curve `{}` was not found", reference.key.display_key()),
            }),
    }
}

fn resolve_effect_params(
    project: &DawnProject,
    params: &[SequenceEffectParamDocument],
) -> Result<Vec<SequenceEffectParamDocument>, RuntimeError> {
    params
        .iter()
        .map(|param| {
            Ok(SequenceEffectParamDocument {
                name: param.name.clone(),
                value: resolve_effect_param_value(project, &param.value)?,
                curve_source: param.curve_source.clone(),
            })
        })
        .collect()
}

fn resolve_effect_param_value(
    project: &DawnProject,
    param: &EffectParam<Resolved>,
) -> Result<EffectParam<Resolved>, RuntimeError> {
    match param {
        EffectParam::Curve { curve } => Ok(EffectParam::Curve {
            curve: resolve_curve_use(project, curve)?,
        }),
        EffectParam::Array {
            element_type,
            values,
        } => Ok(EffectParam::Array {
            element_type: *element_type,
            values: values
                .iter()
                .map(|value| resolve_effect_param_array_value(project, value))
                .collect::<Result<Vec<_>, _>>()?,
        }),
        _ => Ok(param.clone()),
    }
}

fn resolve_effect_param_array_value(
    project: &DawnProject,
    value: &dawn_project::EffectParamArrayValue<Resolved>,
) -> Result<dawn_project::EffectParamArrayValue<Resolved>, RuntimeError> {
    match value {
        dawn_project::EffectParamArrayValue::Curve(curve) => Ok(
            dawn_project::EffectParamArrayValue::Curve(resolve_curve_use(project, curve)?),
        ),
        _ => Ok(value.clone()),
    }
}

fn resolve_curve_use(
    project: &DawnProject,
    curve: &CurveUse<Resolved>,
) -> Result<CurveUse<Resolved>, RuntimeError> {
    match &curve.curve {
        ResolvedInlineOrRef::Inline(_) => Ok(curve.clone()),
        ResolvedInlineOrRef::Ref(reference) => {
            let resolved = project
                .stores
                .curves
                .get(&reference.key)
                .map(|curve| curve.value.clone())
                .ok_or_else(|| RuntimeError {
                    message: format!("curve `{}` was not found", reference.key.display_key()),
                })?;
            Ok(CurveUse {
                id: curve.id,
                curve: ResolvedInlineOrRef::Inline(resolved),
            })
        }
    }
}

fn color_cache_key(color: Color) -> ColorCacheKey {
    ColorCacheKey {
        red: color.red,
        green: color.green,
        blue: color.blue,
    }
}

fn prepared_params_cache_key(params: &PreparedEffectParams) -> Vec<RuntimeValueCacheKey> {
    params
        .values()
        .iter()
        .map(runtime_value_cache_key)
        .collect()
}

fn runtime_value_cache_key(value: &RuntimeValue) -> RuntimeValueCacheKey {
    match value {
        RuntimeValue::Float(value) => RuntimeValueCacheKey::Float(F64CacheKey(*value)),
        RuntimeValue::Int(value) => RuntimeValueCacheKey::Int(*value),
        RuntimeValue::Bool(value) => RuntimeValueCacheKey::Bool(*value),
        RuntimeValue::Color(value) => RuntimeValueCacheKey::Color(color_cache_key(*value)),
        RuntimeValue::Marks(values) => RuntimeValueCacheKey::Marks {
            windowed: values.windowed.iter().copied().map(F64CacheKey).collect(),
            global: values.global.iter().copied().map(F64CacheKey).collect(),
        },
        RuntimeValue::Curve(curve) => RuntimeValueCacheKey::Curve(curve_cache_key(curve)),
        RuntimeValue::Array(array) => {
            RuntimeValueCacheKey::Array(array.values.iter().map(runtime_value_cache_key).collect())
        }
        RuntimeValue::Enum(value) => RuntimeValueCacheKey::Enum(value.clone()),
        RuntimeValue::Flags(value) => RuntimeValueCacheKey::Flags(value.values.clone()),
        RuntimeValue::Fixture(value) => RuntimeValueCacheKey::Fixture(value.index),
        RuntimeValue::Pixel(value) => RuntimeValueCacheKey::Pixel {
            index: value.index,
            count: value.count,
        },
        RuntimeValue::Target(_) | RuntimeValue::TargetItems(_) | RuntimeValue::TargetItem(_) => {
            unreachable!("sample params do not contain generator target values")
        }
    }
}

fn localize_prepared_effects(
    effects: &[PreparedSequenceEffect],
    parent_start_seconds: f64,
) -> Vec<PreparedSequenceEffect> {
    effects
        .iter()
        .cloned()
        .map(|mut effect| {
            effect.start_seconds -= parent_start_seconds;
            effect
        })
        .collect()
}

fn shift_prepared_effects_to_parent_start(
    effects: &[PreparedSequenceEffect],
    parent_start_seconds: f64,
) -> Vec<PreparedSequenceEffect> {
    effects
        .iter()
        .cloned()
        .map(|mut effect| {
            effect.start_seconds += parent_start_seconds;
            effect
        })
        .collect()
}

fn prepared_generated_child_count(effects: &[PreparedSequenceEffect]) -> usize {
    effects.iter().filter(|effect| !effect.authored).count()
}

fn normalized_f64_bits(value: f64) -> u64 {
    if value == 0.0 {
        0.0f64.to_bits()
    } else {
        value.to_bits()
    }
}

fn build_effect_indices_by_frame(
    effects: &[PreparedSequenceEffect],
    duration_seconds: f64,
    frame_rate: u32,
) -> Vec<Vec<usize>> {
    let sequence_duration =
        TimeSpan::try_from_seconds_f64_rounded(duration_seconds.max(0.0)).unwrap_or(TimeSpan::ZERO);
    let frame_count = frame_count(sequence_duration.as_seconds_f64(), frame_rate);
    let mut indices_by_frame = vec![Vec::new(); frame_count];
    if frame_count == 0 {
        return indices_by_frame;
    }

    for (effect_index, effect) in effects.iter().enumerate() {
        let Some((start_frame, end_frame)) =
            effect_frame_range(effect, duration_seconds, frame_rate, frame_count)
        else {
            continue;
        };
        for frame_indices in &mut indices_by_frame[start_frame..end_frame] {
            frame_indices.push(effect_index);
        }
    }
    indices_by_frame
}

fn effect_frame_range(
    effect: &PreparedSequenceEffect,
    duration_seconds: f64,
    frame_rate: u32,
    frame_count: usize,
) -> Option<(usize, usize)> {
    let sequence_end = duration_seconds.max(0.0);
    let effect_start = effect.start_seconds;
    let effect_end = effect.end_seconds();
    if !effect_start.is_finite()
        || !effect.duration_seconds.is_finite()
        || effect.duration_seconds <= 0.0
        || effect_end <= 0.0
        || effect_start >= sequence_end
    {
        return None;
    }

    let clamped_start = effect_start.max(0.0).min(sequence_end);
    let clamped_end = effect_end.max(0.0).min(sequence_end);
    if clamped_start >= clamped_end {
        return None;
    }

    let start_frame = floor_frame(time_from_seconds_clamped(clamped_start), frame_rate);
    let end_frame = ceil_frame(time_from_seconds_clamped(clamped_end), frame_rate);
    let start_frame = usize::try_from(start_frame).ok()?.min(frame_count);
    let end_frame = usize::try_from(end_frame).ok()?.min(frame_count);
    (start_frame < end_frame).then_some((start_frame, end_frame))
}

fn time_from_seconds_clamped(seconds: f64) -> Time {
    Time::try_from_seconds_f64_rounded(seconds.max(0.0)).unwrap_or(Time::ZERO)
}

fn floor_frame(time: Time, frame_rate: u32) -> u64 {
    if frame_rate == 0 {
        return 0;
    }
    (time.as_seconds_f64() * f64::from(frame_rate))
        .floor()
        .max(0.0) as u64
}

fn ceil_frame(time: Time, frame_rate: u32) -> u64 {
    if frame_rate == 0 {
        return 0;
    }
    (time.as_seconds_f64() * f64::from(frame_rate))
        .ceil()
        .max(0.0) as u64
}

fn frame_count(duration_seconds: f64, frame_rate: u32) -> usize {
    if frame_rate == 0 || !duration_seconds.is_finite() || duration_seconds <= 0.0 {
        return 0;
    }
    (duration_seconds * f64::from(frame_rate)).ceil() as usize
}

fn prepare_generated_topology(
    project: &DawnProject,
    document: &SequenceEditorDocument,
    parent_start_seconds: f64,
    parent_duration_seconds: f64,
    parent_scope: SequenceEffectScope,
    generator: &CompiledEffect,
    render: &SequenceRenderEffect,
) -> Result<Vec<GeneratedChildTopology>, RuntimeError> {
    let prepared_params = prepare_params_from_document(
        project,
        generator,
        &render.params,
        &document.mark_collections,
        parent_start_seconds,
        parent_duration_seconds,
    )?;
    let target = GeneratorTarget {
        pixels: render
            .target_pixels
            .iter()
            .map(|pixel| GeneratorTargetPixel {
                fixture_index: pixel.fixture_index,
                pixel_index: pixel.pixel_index,
                pixel_count: pixel.pixel_count,
            })
            .collect(),
    };
    let targets = generator_targets_for_scope(parent_scope, target);
    let mut children = Vec::new();
    for target in targets {
        children.extend(generator.generator_topology(
            &prepared_params,
            target,
            parent_duration_seconds,
        )?);
    }
    let max_child_end = children
        .iter()
        .map(|child| child.start_seconds + child.duration_seconds)
        .fold(0.0, f64::max);
    if max_child_end > parent_duration_seconds {
        let timeline_scale = parent_duration_seconds / max_child_end;
        for child in &mut children {
            child.start_seconds *= timeline_scale;
            child.duration_seconds *= timeline_scale;
        }
    }
    Ok(children)
}

struct GeneratedEffectTopologyInput<'a> {
    project: &'a DawnProject,
    parent_path: Utf8PathBuf,
    parent_id: u32,
    parent_start_seconds: f64,
    parent_duration_seconds: f64,
    generator_id: EffectDefinitionKey,
    generator: &'a CompiledEffect,
    render: &'a SequenceRenderEffect,
    mark_collections: &'a [SequenceMarkCollectionDocument],
    fixture_templates: &'a [OutputFixtureFrame],
    children: Vec<GeneratedChildTopology>,
    is_cancelled: &'a dyn Fn() -> bool,
}

fn prepare_generated_effects_from_topology(
    input: GeneratedEffectTopologyInput<'_>,
) -> Result<Vec<PreparedSequenceEffect>, RuntimeError> {
    let prepared_parent_params = prepare_params_from_document(
        input.project,
        input.generator,
        &input.render.params,
        input.mark_collections,
        input.parent_start_seconds,
        input.parent_duration_seconds,
    )?;
    let mut effects = Vec::new();
    let mut stack = vec![input.generator_id];
    let mut child_count = 0;
    if (input.is_cancelled)() {
        return Ok(effects);
    }
    flatten_generated_children(
        GeneratedChildFlattenInput {
            project: input.project,
            parent_path: &input.parent_path,
            parent_id: input.parent_id,
            parent_start_seconds: input.parent_start_seconds,
            parent_script: input.generator,
            parent_params: &prepared_parent_params,
            fixture_templates: input.fixture_templates,
            children: input.children,
            is_cancelled: input.is_cancelled,
        },
        &mut stack,
        &mut child_count,
        &mut effects,
    )?;
    Ok(effects)
}

struct GeneratedChildFlattenInput<'a> {
    project: &'a DawnProject,
    parent_path: &'a Utf8PathBuf,
    parent_id: u32,
    parent_start_seconds: f64,
    parent_script: &'a CompiledEffect,
    parent_params: &'a PreparedEffectParams,
    fixture_templates: &'a [OutputFixtureFrame],
    children: Vec<GeneratedChildTopology>,
    is_cancelled: &'a dyn Fn() -> bool,
}

fn flatten_generated_children(
    input: GeneratedChildFlattenInput<'_>,
    stack: &mut Vec<EffectDefinitionKey>,
    child_count: &mut usize,
    effects: &mut Vec<PreparedSequenceEffect>,
) -> Result<(), RuntimeError> {
    for child in input.children {
        if (input.is_cancelled)() {
            return Ok(());
        }
        let child_ref = resolve_generated_child_effect(
            input.project,
            input.parent_path,
            input.parent_script,
            &child.effect,
        )?;
        let emitted_params = input
            .parent_script
            .generator_child_params(&child, input.parent_params)?;
        let prepared_params = child_ref.script.prepare_params(&emitted_params)?;
        match child_ref.script.kind() {
            EffectScriptKind::Sample => {
                if *child_count >= MAX_FLATTENED_GENERATED_CHILDREN {
                    return Err(RuntimeError {
                        message: format!(
                            "generator exceeded maximum flattened child count ({MAX_FLATTENED_GENERATED_CHILDREN})"
                        ),
                    });
                }
                *child_count += 1;
                let sample_reuse_effect_key = sample_reuse_effect_key(
                    child_ref.id.clone(),
                    child_ref.id.display_key(),
                    &prepared_params,
                );
                let target_pixels = prepare_effect_pixels(
                    SequenceEffectScope::WholeTarget,
                    &sequence_effect_pixels_for_generator_target(&child.target),
                    input.fixture_templates,
                );
                let target_pixel_groups = prepare_effect_pixel_groups(&target_pixels);
                effects.push(PreparedSequenceEffect {
                    start_seconds: input.parent_start_seconds + child.start_seconds,
                    duration_seconds: child.duration_seconds,
                    authored: false,
                    render: PreparedEffectRender::Ready {
                        script: Box::new(child_ref.script.clone()),
                        target_pixels,
                        target_pixel_groups,
                        prepared_params: Box::new(prepared_params),
                        sample_reuse_effect_key: Box::new(sample_reuse_effect_key),
                        scratch: Box::new(EffectSampleScratch::new(
                            child_ref.script.bytecode_stats(),
                        )),
                        _bytecode_stats: child_ref.script.bytecode_stats(),
                    },
                });
            }
            EffectScriptKind::Generator => {
                if let Some(cycle_start) = stack
                    .iter()
                    .position(|script_id| *script_id == child_ref.id)
                {
                    let mut cycle = stack[cycle_start..]
                        .iter()
                        .map(EffectDefinitionKey::display_key)
                        .collect::<Vec<_>>();
                    cycle.push(child_ref.id.display_key());
                    return Err(RuntimeError {
                        message: format!("generator cycle detected: {}", cycle.join(" -> ")),
                    });
                }
                let nested_children = prepare_child_generator_topology(
                    child_ref.script,
                    &prepared_params,
                    child.target,
                    child.duration_seconds,
                )?;
                stack.push(child_ref.id.clone());
                flatten_generated_children(
                    GeneratedChildFlattenInput {
                        project: input.project,
                        parent_path: &child_ref.id.path,
                        parent_id: input.parent_id,
                        parent_start_seconds: input.parent_start_seconds + child.start_seconds,
                        parent_script: child_ref.script,
                        parent_params: &prepared_params,
                        fixture_templates: input.fixture_templates,
                        children: nested_children,
                        is_cancelled: input.is_cancelled,
                    },
                    stack,
                    child_count,
                    effects,
                )?;
                stack.pop();
            }
        }
    }
    Ok(())
}

struct ResolvedGeneratedChildEffect<'a> {
    id: EffectDefinitionKey,
    script: &'a CompiledEffect,
}

fn resolve_generated_child_effect<'a>(
    project: &'a DawnProject,
    parent_path: &Utf8PathBuf,
    parent_script: &CompiledEffect,
    child: &GeneratedChildEffectRef,
) -> Result<ResolvedGeneratedChildEffect<'a>, RuntimeError> {
    let (child_path, child_name) = match child {
        GeneratedChildEffectRef::Local { name } => (parent_path.clone(), name.clone()),
        GeneratedChildEffectRef::Imported { alias, name } => {
            let import =
                parent_script
                    .import_path_for_alias(alias)
                    .ok_or_else(|| RuntimeError {
                        message: format!("generator import alias `{alias}` was not found"),
                    })?;
            (
                resolve_import_path(parent_path, &Utf8PathBuf::from(import)),
                name.clone(),
            )
        }
    };
    let child_id = EffectDefinitionKey::new(child_path, child_name.clone());
    let child_script = project
        .stores
        .effect_definitions
        .get(&child_id)
        .map(|effect| &effect.value.compiled)
        .ok_or_else(|| RuntimeError {
            message: format!(
                "compiled child script `{}` was not found",
                child_id.display_key()
            ),
        })?;
    if child_script.name() != child_name {
        return Err(RuntimeError {
            message: format!(
                "compiled child script `{}` did not match emitted effect `{child_name}`",
                child_id.display_key()
            ),
        });
    }
    Ok(ResolvedGeneratedChildEffect {
        id: child_id,
        script: child_script,
    })
}

fn prepare_child_generator_topology(
    generator: &CompiledEffect,
    prepared_params: &PreparedEffectParams,
    target: GeneratorTarget,
    duration_seconds: f64,
) -> Result<Vec<GeneratedChildTopology>, RuntimeError> {
    let mut children = generator.generator_topology(prepared_params, target, duration_seconds)?;
    scale_generated_children_to_duration(&mut children, duration_seconds);
    Ok(children)
}

fn scale_generated_children_to_duration(
    children: &mut [GeneratedChildTopology],
    duration_seconds: f64,
) {
    let max_child_end = children
        .iter()
        .map(|child| child.start_seconds + child.duration_seconds)
        .fold(0.0, f64::max);
    if max_child_end > duration_seconds {
        let timeline_scale = duration_seconds / max_child_end;
        for child in children {
            child.start_seconds *= timeline_scale;
            child.duration_seconds *= timeline_scale;
        }
    }
}

fn sequence_effect_pixels_for_generator_target(
    target: &GeneratorTarget,
) -> Vec<SequenceRenderPixel> {
    target
        .pixels
        .iter()
        .map(|pixel| SequenceRenderPixel {
            fixture_index: pixel.fixture_index,
            pixel_index: pixel.pixel_index,
            pixel_count: pixel.pixel_count,
        })
        .collect()
}

fn generator_targets_for_scope(
    scope: SequenceEffectScope,
    target: GeneratorTarget,
) -> Vec<GeneratorTarget> {
    match scope {
        SequenceEffectScope::WholeTarget => vec![target],
        SequenceEffectScope::PerFixture => {
            let mut targets = Vec::new();
            for pixel in target.pixels {
                match targets.last_mut() {
                    Some(last) if same_generator_target_fixture(last, pixel.fixture_index) => {
                        last.pixels.push(pixel);
                    }
                    _ => targets.push(GeneratorTarget {
                        pixels: vec![pixel],
                    }),
                }
            }
            targets
        }
    }
}

fn same_generator_target_fixture(target: &GeneratorTarget, fixture_index: usize) -> bool {
    target
        .pixels
        .first()
        .is_some_and(|pixel| pixel.fixture_index == fixture_index)
}

#[derive(Debug, Clone)]
struct PreparedSequenceEffect {
    start_seconds: f64,
    duration_seconds: f64,
    authored: bool,
    render: PreparedEffectRender,
}

impl PreparedSequenceEffect {
    fn end_seconds(&self) -> f64 {
        self.start_seconds + self.duration_seconds
    }
}

#[derive(Debug, Clone)]
enum PreparedEffectRender {
    Ready {
        script: Box<CompiledEffect>,
        target_pixels: Vec<PreparedEffectPixel>,
        target_pixel_groups: Vec<PreparedEffectPixelGroup>,
        prepared_params: Box<PreparedEffectParams>,
        sample_reuse_effect_key: Box<SampleReuseEffectKey>,
        scratch: Box<EffectSampleScratch>,
        _bytecode_stats: BytecodeStats,
    },
    Error {
        message: String,
    },
}

#[derive(Debug, Clone)]
struct PreparedEffectPixel {
    fixture_index: usize,
    pixel_index: usize,
    fixture_context: FixtureContext,
    pixel_context: PixelContext,
}

#[derive(Debug, Clone)]
struct PreparedEffectPixelGroup {
    first_pixel_index: usize,
    pixel_context: PixelContext,
    output_pixel_indices: Vec<usize>,
}

fn prepare_effect_pixels(
    scope: SequenceEffectScope,
    target_pixels: &[SequenceRenderPixel],
    fixture_templates: &[OutputFixtureFrame],
) -> Vec<PreparedEffectPixel> {
    let target_pixel_count = target_pixels.len();
    target_pixels
        .iter()
        .enumerate()
        .filter_map(|(target_pixel_index, pixel)| {
            fixture_templates
                .get(pixel.fixture_index)?
                .pixels
                .get(pixel.pixel_index)?;
            Some(PreparedEffectPixel {
                fixture_index: pixel.fixture_index,
                pixel_index: pixel.pixel_index,
                fixture_context: FixtureContext {
                    index: pixel.fixture_index,
                },
                pixel_context: pixel_context_for_effect(
                    scope,
                    target_pixel_index,
                    target_pixel_count,
                    pixel.pixel_index,
                    pixel.pixel_count,
                ),
            })
        })
        .collect()
}

fn prepare_effect_pixel_groups(
    target_pixels: &[PreparedEffectPixel],
) -> Vec<PreparedEffectPixelGroup> {
    let mut group_indices_by_context: HashMap<(usize, usize), usize> = HashMap::new();
    let mut groups: Vec<PreparedEffectPixelGroup> = Vec::new();
    for (pixel_index, pixel) in target_pixels.iter().enumerate() {
        let context = (pixel.pixel_context.index, pixel.pixel_context.count);
        match group_indices_by_context.get(&context).copied() {
            Some(group_index) => groups[group_index].output_pixel_indices.push(pixel_index),
            None => {
                group_indices_by_context.insert(context, groups.len());
                groups.push(PreparedEffectPixelGroup {
                    first_pixel_index: pixel_index,
                    pixel_context: pixel.pixel_context,
                    output_pixel_indices: vec![pixel_index],
                });
            }
        }
    }
    groups
}

fn diagnostic_prepared_effect(
    effect: &SequenceEffectDocument,
    message: String,
    diagnostic_pixels: &[SequenceRenderPixel],
    scope: SequenceEffectScope,
    fixture_templates: &[OutputFixtureFrame],
) -> PreparedSequenceEffect {
    PreparedSequenceEffect {
        start_seconds: effect.start_seconds,
        duration_seconds: effect.duration_seconds,
        authored: true,
        render: diagnostic_render(message, diagnostic_pixels, scope, fixture_templates),
    }
}

fn diagnostic_render(
    message: String,
    _diagnostic_pixels: &[SequenceRenderPixel],
    _scope: SequenceEffectScope,
    _fixture_templates: &[OutputFixtureFrame],
) -> PreparedEffectRender {
    PreparedEffectRender::Error { message }
}

pub fn pixel_context_for_effect(
    scope: SequenceEffectScope,
    target_pixel_index: usize,
    target_pixel_count: usize,
    fixture_pixel_index: usize,
    fixture_pixel_count: usize,
) -> PixelContext {
    match scope {
        SequenceEffectScope::PerFixture => PixelContext {
            index: fixture_pixel_index,
            count: fixture_pixel_count,
        },
        SequenceEffectScope::WholeTarget => PixelContext {
            index: target_pixel_index,
            count: target_pixel_count,
        },
    }
}

fn add_clamped(target: &mut Color, color: Color) {
    target.red = target.red.saturating_add(color.red);
    target.green = target.green.saturating_add(color.green);
    target.blue = target.blue.saturating_add(color.blue);
}

fn elapsed_ms(started: Instant) -> f64 {
    started.elapsed().as_secs_f64() * 1000.0
}

pub fn runtime_params_from_document(
    project: &DawnProject,
    params: &[SequenceEffectParamDocument],
    mark_collections: &[SequenceMarkCollectionDocument],
    effect_start_seconds: f64,
    effect_duration_seconds: f64,
) -> Result<BTreeMap<String, RuntimeValue>, RuntimeError> {
    let mut values = BTreeMap::new();
    for param in params {
        if let Some(value) = runtime_value_from_param(
            project,
            &param.value,
            mark_collections,
            effect_start_seconds,
            effect_duration_seconds,
        )? {
            values.insert(param.name.clone(), value);
        }
    }
    Ok(values)
}

pub fn prepare_params_from_document(
    project: &DawnProject,
    script: &CompiledEffect,
    params: &[SequenceEffectParamDocument],
    mark_collections: &[SequenceMarkCollectionDocument],
    effect_start_seconds: f64,
    effect_duration_seconds: f64,
) -> Result<PreparedEffectParams, RuntimeError> {
    let values = runtime_params_from_document(
        project,
        params,
        mark_collections,
        effect_start_seconds,
        effect_duration_seconds,
    )?;
    script.prepare_params(&values)
}

pub fn runtime_value_from_param(
    project: &DawnProject,
    param: &EffectParam<Resolved>,
    mark_collections: &[SequenceMarkCollectionDocument],
    effect_start_seconds: f64,
    effect_duration_seconds: f64,
) -> Result<Option<RuntimeValue>, RuntimeError> {
    match param {
        EffectParam::Integer { value } => Ok(Some(RuntimeValue::Int(*value as i64))),
        EffectParam::Float { value } => Ok(Some(RuntimeValue::Float(*value))),
        EffectParam::Boolean { value } => Ok(Some(RuntimeValue::Bool(*value))),
        EffectParam::Enum { value } => Ok(Some(RuntimeValue::Enum(value.clone()))),
        EffectParam::Flags { value } => Ok(Some(RuntimeValue::Flags(value.clone()))),
        EffectParam::Color { value } => Ok(Some(RuntimeValue::Color(*value))),
        EffectParam::Curve { curve } => Ok(Some(RuntimeValue::Curve(
            curve_for_runtime(project, curve)?.clone(),
        ))),
        EffectParam::Array {
            element_type,
            values,
        } => {
            let mut runtime_values = Vec::new();
            for value in values {
                let Some(value) = runtime_value_from_array_param(project, value)? else {
                    return Ok(None);
                };
                runtime_values.push(value);
            }
            Ok(Some(RuntimeValue::Array(RuntimeArrayValue {
                element_type: *element_type,
                values: runtime_values,
            })))
        }
        EffectParam::Marks { key } => {
            let Some(collection) = mark_collections
                .iter()
                .find(|collection| collection.key == *key)
            else {
                return Ok(None);
            };
            let global = collection
                .marks_seconds
                .iter()
                .map(|mark_seconds| *mark_seconds - effect_start_seconds)
                .collect::<Vec<_>>();
            Ok(Some(RuntimeValue::Marks(runtime_marks(
                global,
                effect_duration_seconds,
            ))))
        }
    }
}

fn runtime_marks(mut global: Vec<f64>, effect_duration_seconds: f64) -> RuntimeMarks {
    global.sort_by(f64::total_cmp);
    let windowed = global
        .iter()
        .copied()
        .filter(|mark| *mark >= 0.0 && *mark < effect_duration_seconds)
        .collect();
    RuntimeMarks { windowed, global }
}

fn runtime_value_from_array_param(
    project: &DawnProject,
    value: &dawn_project::EffectParamArrayValue<Resolved>,
) -> Result<Option<RuntimeValue>, RuntimeError> {
    match value {
        dawn_project::EffectParamArrayValue::Integer(value) => {
            Ok(Some(RuntimeValue::Int(*value as i64)))
        }
        dawn_project::EffectParamArrayValue::Float(value) => Ok(Some(RuntimeValue::Float(*value))),
        dawn_project::EffectParamArrayValue::Boolean(value) => Ok(Some(RuntimeValue::Bool(*value))),
        dawn_project::EffectParamArrayValue::Color(value) => Ok(Some(RuntimeValue::Color(*value))),
        dawn_project::EffectParamArrayValue::Curve(curve) => Ok(Some(RuntimeValue::Curve(
            curve_for_runtime(project, curve)?.clone(),
        ))),
    }
}

pub fn empty_geometry() -> OutputGeometryModel {
    let bounds = GeometryRenderBounds {
        min_x: Distance::from_micrometers(-5_000_000),
        min_y: Distance::from_micrometers(-4_000_000),
        max_x: Distance::from_micrometers(5_000_000),
        max_y: Distance::from_micrometers(4_000_000),
    };
    let fixtures = Vec::new();
    OutputGeometryModel {
        geometry_id: OutputGeometryIdentity::from_parts(bounds, &fixtures).stable_key(),
        bounds,
        fixtures,
        target_pixels_by_target: HashMap::new(),
    }
}

pub fn empty_frame(
    geometry: &OutputGeometryModel,
    generation: u64,
    message: impl Into<String>,
) -> RenderedOutputFrame {
    RenderedOutputFrame {
        geometry_id: geometry.geometry_id.clone(),
        time_seconds: 0.0,
        generation,
        status: OutputFrameStatus::Idle(message.into()),
        rgb: vec![0; geometry_pixel_count(geometry) * 3],
    }
}

pub fn geometry_pixel_count(geometry: &OutputGeometryModel) -> usize {
    geometry
        .fixtures
        .iter()
        .map(|fixture| fixture.pixels.len())
        .sum()
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use crate::document::{
        GeometryRenderBounds, GeometryRenderPoint, SequenceEditorDocument,
        SequenceEffectParamDocument,
    };
    use crate::workspace::WorkspaceService;
    use dawn_project::{
        Color, CurveValue, Distance, EffectParam, Resolved, ResolvedInlineOrRef,
        SequenceEffectScope,
    };
    use dawn_project::{DawnProject, Utf8PathBuf};

    use dawn_project::{GeneratorTarget, GeneratorTargetPixel};

    use super::{
        generator_targets_for_scope, pixel_context_for_effect, OutputFixtureFrame,
        OutputGeometryBoundsIdentity, OutputGeometryIdentity, OutputPixelFrame,
        RenderedOutputFrame, SequenceChangeImpact, SequenceRenderPlan,
    };

    fn christmas_house_project_path() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/christmas-house/project.dawn")
    }

    fn load_sequence(
        project_path: PathBuf,
        sequence_path: Utf8PathBuf,
        object_key: &str,
    ) -> (DawnProject, SequenceEditorDocument) {
        let mut workspace = WorkspaceService::default();
        workspace
            .open_project(
                std::fs::canonicalize(&project_path).expect("example project path should exist"),
            )
            .expect("example project should open");
        let result = workspace.load_project();
        assert!(result.diagnostics.is_empty(), "{:?}", result.diagnostics);
        let project = result.project.expect("example project should load");
        let document = workspace
            .sequence_document(&project, sequence_path, object_key)
            .expect("example sequence should load");
        (project, document)
    }

    fn christmas_house_project_and_sequence() -> (DawnProject, SequenceEditorDocument) {
        load_sequence(
            christmas_house_project_path(),
            Utf8PathBuf::from("sequences/christmas.sequence.dawn"),
            "christmas",
        )
    }

    fn thirty_output_controller_project_and_sequence() -> (DawnProject, SequenceEditorDocument) {
        load_sequence(
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../examples/thirty-output-controller/project.dawn"),
            Utf8PathBuf::from("sequences/empty.sequence.dawn"),
            "empty",
        )
    }

    fn mutate_curve_point(
        document: &mut SequenceEditorDocument,
        effect_id: u32,
        param_name: &str,
        point_index: usize,
        value: f64,
    ) {
        let param = render_param_mut(document, effect_id, param_name);
        match &mut param.value {
            EffectParam::<Resolved>::Curve { curve } => {
                let ResolvedInlineOrRef::Inline(curve) = &mut curve.curve else {
                    panic!("expected inline curve param `{param_name}`");
                };
                curve.points[point_index].value = CurveValue::Float(value);
            }
            _ => panic!("expected curve param `{param_name}`"),
        }
    }

    fn render_param_mut<'a>(
        document: &'a mut SequenceEditorDocument,
        effect_id: u32,
        param_name: &str,
    ) -> &'a mut SequenceEffectParamDocument {
        document
            .effects
            .iter_mut()
            .find(|effect| effect.id == effect_id)
            .and_then(|effect| {
                effect
                    .params
                    .iter_mut()
                    .find(|param| param.name == param_name)
            })
            .unwrap_or_else(|| panic!("effect `{effect_id}` param `{param_name}` should exist"))
    }

    fn assert_only_invalidated(impact: &SequenceChangeImpact, prepared: &[u32], topology: &[u32]) {
        assert_eq!(
            impact
                .invalidated_prepared_effect_ids()
                .iter()
                .copied()
                .collect::<std::collections::BTreeSet<_>>(),
            prepared
                .iter()
                .copied()
                .collect::<std::collections::BTreeSet<_>>()
        );
        assert_eq!(
            impact
                .invalidated_topology_effect_ids()
                .iter()
                .copied()
                .collect::<std::collections::BTreeSet<_>>(),
            topology
                .iter()
                .copied()
                .collect::<std::collections::BTreeSet<_>>()
        );
    }

    #[test]
    fn sequence_change_impact_invalidates_only_chase_pulse_shape_prepared_entries() {
        let (project, document) = thirty_output_controller_project_and_sequence();
        let mut edited = document.clone();
        mutate_curve_point(&mut edited, 3, "pulse_shape", 1, 0.25);

        let impact = SequenceChangeImpact::between(&document, &edited, &project);

        assert_only_invalidated(&impact, &[3], &[]);
    }

    fn frame_colors(frame: &RenderedOutputFrame) -> Vec<Color> {
        frame
            .rgb
            .chunks_exact(3)
            .map(|rgb| Color::new(rgb[0], rgb[1], rgb[2]))
            .collect()
    }

    fn lit_pixel_count(frame: &RenderedOutputFrame) -> usize {
        frame_colors(frame)
            .into_iter()
            .filter(|color| *color != Color::new(0, 0, 0))
            .count()
    }

    fn topology_fixture(pixel_count: usize) -> OutputFixtureFrame {
        OutputFixtureFrame {
            id: dawn_project::FixtureId(1),
            name: "fixture".to_string(),
            bulb_radius: dawn_project::DistanceSpan::from_micrometers(100_000),
            pixels: (0..pixel_count)
                .map(|index| OutputPixelFrame {
                    position: GeometryRenderPoint {
                        x: Distance::from_micrometers(index as i64),
                        y: Distance::from_micrometers(0),
                        z: Distance::from_micrometers(0),
                    },
                    color: Color::new(0, 0, 0),
                })
                .collect(),
        }
    }

    #[test]
    fn output_geometry_identity_tracks_pixel_count_and_bounds() {
        let bounds = GeometryRenderBounds {
            min_x: Distance::from_micrometers(0),
            min_y: Distance::from_micrometers(0),
            max_x: Distance::from_micrometers(1_000_000),
            max_y: Distance::from_micrometers(1_000_000),
        };
        let base = OutputGeometryIdentity::from_parts(bounds, &[topology_fixture(2)]);

        assert_ne!(
            base,
            OutputGeometryIdentity::from_parts(bounds, &[topology_fixture(3)])
        );
        let changed_bounds = GeometryRenderBounds {
            max_x: Distance::from_micrometers(2_000_000),
            ..bounds
        };
        assert_ne!(
            base.bounds,
            OutputGeometryBoundsIdentity {
                min_x_micrometers: changed_bounds.min_x.as_micrometers(),
                min_y_micrometers: changed_bounds.min_y.as_micrometers(),
                max_x_micrometers: changed_bounds.max_x.as_micrometers(),
                max_y_micrometers: changed_bounds.max_y.as_micrometers()
            }
        );
        assert_ne!(
            base,
            OutputGeometryIdentity::from_parts(changed_bounds, &[topology_fixture(2)])
        );
    }

    #[test]
    fn per_fixture_scope_repeats_pixel_context_for_group_members() {
        let contexts = [
            pixel_context_for_effect(SequenceEffectScope::PerFixture, 0, 5, 0, 2),
            pixel_context_for_effect(SequenceEffectScope::PerFixture, 1, 5, 1, 2),
            pixel_context_for_effect(SequenceEffectScope::PerFixture, 2, 5, 0, 3),
            pixel_context_for_effect(SequenceEffectScope::PerFixture, 3, 5, 1, 3),
            pixel_context_for_effect(SequenceEffectScope::PerFixture, 4, 5, 2, 3),
        ];

        assert_eq!(
            contexts.map(|context| (context.index, context.count)),
            [(0, 2), (1, 2), (0, 3), (1, 3), (2, 3)]
        );
    }

    #[test]
    fn whole_target_scope_uses_continuous_group_pixel_context() {
        let contexts = [
            pixel_context_for_effect(SequenceEffectScope::WholeTarget, 0, 5, 0, 2),
            pixel_context_for_effect(SequenceEffectScope::WholeTarget, 1, 5, 1, 2),
            pixel_context_for_effect(SequenceEffectScope::WholeTarget, 2, 5, 0, 3),
            pixel_context_for_effect(SequenceEffectScope::WholeTarget, 3, 5, 1, 3),
            pixel_context_for_effect(SequenceEffectScope::WholeTarget, 4, 5, 2, 3),
        ];

        assert_eq!(
            contexts.map(|context| (context.index, context.count)),
            [(0, 5), (1, 5), (2, 5), (3, 5), (4, 5)]
        );
    }

    #[test]
    fn fixture_target_context_matches_for_both_scopes() {
        let per_fixture = pixel_context_for_effect(SequenceEffectScope::PerFixture, 1, 3, 1, 3);
        let whole_target = pixel_context_for_effect(SequenceEffectScope::WholeTarget, 1, 3, 1, 3);

        assert_eq!(
            (per_fixture.index, per_fixture.count),
            (whole_target.index, whole_target.count)
        );
    }

    #[test]
    fn generator_per_fixture_scope_splits_target_before_generation() {
        let target = GeneratorTarget {
            pixels: vec![
                GeneratorTargetPixel {
                    fixture_index: 0,
                    pixel_index: 0,
                    pixel_count: 2,
                },
                GeneratorTargetPixel {
                    fixture_index: 0,
                    pixel_index: 1,
                    pixel_count: 2,
                },
                GeneratorTargetPixel {
                    fixture_index: 1,
                    pixel_index: 0,
                    pixel_count: 3,
                },
                GeneratorTargetPixel {
                    fixture_index: 1,
                    pixel_index: 1,
                    pixel_count: 3,
                },
                GeneratorTargetPixel {
                    fixture_index: 1,
                    pixel_index: 2,
                    pixel_count: 3,
                },
            ],
        };

        let per_fixture =
            generator_targets_for_scope(SequenceEffectScope::PerFixture, target.clone());
        let whole_target = generator_targets_for_scope(SequenceEffectScope::WholeTarget, target);

        assert_eq!(per_fixture.len(), 2);
        assert_eq!(per_fixture[0].pixels.len(), 2);
        assert_eq!(per_fixture[1].pixels.len(), 3);
        assert_eq!(whole_target.len(), 1);
        assert_eq!(whole_target[0].pixels.len(), 5);
    }

    #[test]
    fn per_fixture_sample_reuse_reduces_vm_evaluations_without_skipping_output_pixels() {
        let (project, document) = christmas_house_project_and_sequence();
        let mut evaluator =
            SequenceRenderPlan::new(&project, &document).expect("renderer should build");

        let (_frame, timing) = evaluator.render_frame_timed(1.0, 1);

        assert!(timing.sampled_pixels > 0);
        assert!(timing.vm_sample_evaluations > 0);
        assert!(timing.vm_sample_evaluations < timing.sampled_pixels);
        assert_eq!(
            timing.sampled_pixels,
            timing.vm_sample_evaluations + timing.sample_reuse_saved_evaluations
        );
    }

    #[test]
    fn reusable_sequence_evaluator_updates_frame_output_over_time() {
        let (project, document) = christmas_house_project_and_sequence();
        let mut evaluator =
            SequenceRenderPlan::new(&project, &document).expect("renderer should build");

        let first = evaluator.render_frame(2.0, 1);
        let second = evaluator.render_frame(6.0, 2);

        assert_ne!(frame_colors(&first), frame_colors(&second));
        assert!(lit_pixel_count(&first) > 0);
        assert!(lit_pixel_count(&second) > 0);
    }
}
