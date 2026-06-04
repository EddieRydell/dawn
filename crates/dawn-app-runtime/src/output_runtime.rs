use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::hash::{Hash, Hasher};
use std::time::Instant;

use dawn_language::analysis::ProjectAnalysis;
use dawn_language::document::{
    SequenceDocument, SequenceEffectParamDocument, SequenceEffectPixelDocument,
    SequenceMarkCollectionDocument,
};
use dawn_language::effect_script::{
    evaluate_generated_child_params, generator_topology_param_names, run_generator_topology,
    BytecodeStats, CompiledEffect, EffectSampleScratch, EffectScriptKind, FixtureContext,
    GeneratedChildEffectRef, GeneratedChildTopology, GeneratorTarget, GeneratorTargetPixel,
    PixelContext, PreparedEffectParams, RuntimeError, RuntimeValue,
};
use dawn_language::frame::{ceil_frame, floor_frame, frame_count, frame_start};
use dawn_language::model::EffectScriptId;
use dawn_language::model::{
    Color, Curve, CurveValue, CurveValueType, Distance, DistanceSpan, EffectParam, FixtureId,
    Resolved, SequenceEffectScope, Time, TimeSpan,
};
use dawn_language::path::{resolve_import_path, Utf8PathBuf};
use dawn_language::render::{layout_render_plan, GeometryRenderBounds, GeometryRenderPoint};

const MAX_FLATTENED_GENERATED_CHILDREN: usize = 65_536;

#[derive(Debug, Clone)]
pub struct OutputFrame {
    pub source: OutputSourceMetadata,
    pub time_seconds: f64,
    pub generation: u64,
    pub status: OutputFrameStatus,
    pub bounds: GeometryRenderBounds,
    pub fixtures: Vec<OutputFixtureFrame>,
}

#[derive(Debug, Clone)]
pub struct OutputSourceMetadata {
    pub label: String,
    pub kind: OutputSourceKind,
    pub duration_seconds: f64,
    pub fps: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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

pub trait OutputSink {
    fn write_frame(&self, frame: OutputFrame);
}

#[derive(Debug, Clone, Copy, Default)]
pub struct SequenceFrameEvaluationTiming {
    pub total_ms: f64,
    pub fixture_clone_ms: f64,
    pub effect_loop_ms: f64,
    pub output_frame_ms: f64,
    pub active_effects: u32,
    pub active_authored_effects: u32,
    pub active_prepared_effects: u32,
    pub visited_prepared_effects: u32,
    pub sampled_pixels: u32,
}

#[derive(Debug, Clone, Default)]
pub struct SequenceFrameEvaluatorPreparationTiming {
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
    pub script_id: EffectScriptId,
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
pub struct SequenceRenderCache {
    preparation: SequencePreparationCache,
    effect_thumbnails: HashMap<SequenceEffectThumbnailCacheKey, SequenceEffectThumbnail>,
}

impl SequenceRenderCache {
    pub fn clear(&mut self) {
        self.preparation.clear();
        self.effect_thumbnails.clear();
    }

    pub fn apply_change_impact(&mut self, impact: &SequenceChangeImpact) {
        if impact.clear_all {
            self.clear();
            return;
        }
        for effect_id in &impact.invalidated_prepared_effect_ids {
            self.preparation.remove_prepared(*effect_id);
            self.effect_thumbnails
                .retain(|key, _| key.effect_id != *effect_id);
        }
        for effect_id in &impact.invalidated_topology_effect_ids {
            self.preparation.remove_topology(*effect_id);
        }
        self.preparation.prune(&impact.active_effect_ids);
        self.effect_thumbnails
            .retain(|key, _| impact.active_effect_ids.contains(&key.effect_id));
    }

    pub fn build_evaluator(
        &mut self,
        analysis: &ProjectAnalysis,
        document: &SequenceDocument,
    ) -> Result<
        (
            SequenceFrameEvaluator,
            SequenceFrameEvaluatorPreparationTiming,
        ),
        String,
    > {
        SequenceFrameEvaluator::new_with_preparation_cache(
            analysis,
            document,
            &mut self.preparation,
        )
    }

    pub fn effect_thumbnail(
        &mut self,
        analysis: &ProjectAnalysis,
        document: &SequenceDocument,
        effect: &dawn_language::document::SequenceEffectDocument,
        max_columns: usize,
        max_rows: usize,
    ) -> Result<Option<SequenceEffectThumbnail>, String> {
        match self.effect_thumbnail_cancellable(
            analysis,
            document,
            effect,
            max_columns,
            max_rows,
            || false,
        )? {
            SequenceEffectThumbnailResult::Ready(thumbnail) => Ok(Some(thumbnail)),
            SequenceEffectThumbnailResult::Unavailable
            | SequenceEffectThumbnailResult::Cancelled => Ok(None),
        }
    }

    pub fn effect_thumbnail_cancellable(
        &mut self,
        analysis: &ProjectAnalysis,
        document: &SequenceDocument,
        effect: &dawn_language::document::SequenceEffectDocument,
        max_columns: usize,
        max_rows: usize,
        is_cancelled: impl Fn() -> bool,
    ) -> Result<SequenceEffectThumbnailResult, String> {
        sequence_effect_thumbnail(
            analysis,
            document,
            effect,
            max_columns,
            max_rows,
            self,
            &is_cancelled,
        )
    }

    pub fn prepared_entry_count(&self) -> usize {
        self.preparation.prepared_entry_count()
    }

    pub fn topology_entry_count(&self) -> usize {
        self.preparation.topology_entry_count()
    }

    pub fn thumbnail_entry_count(&self) -> usize {
        self.effect_thumbnails.len()
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
        previous: &SequenceDocument,
        refreshed: &SequenceDocument,
        analysis: &ProjectAnalysis,
    ) -> Self {
        let active_effect_ids = refreshed
            .effects
            .iter()
            .map(|effect| effect.id)
            .collect::<HashSet<_>>();
        let mut invalidated_prepared_effect_ids = HashSet::new();
        let mut invalidated_topology_effect_ids = HashSet::new();
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
                if is_generator_effect(analysis, effect) {
                    invalidated_topology_effect_ids.insert(effect.id);
                }
                continue;
            };
            let impact =
                effect_change_impact(previous, refreshed, previous_effect, effect, analysis);
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

#[derive(Debug, Clone)]
pub struct SequenceEffectThumbnail {
    pub effect_id: u32,
    pub duration_seconds: f64,
    pub source_pixel_count: u32,
    pub sampled_pixel_indices: Vec<u32>,
    pub columns: u32,
    pub rows: u32,
    pub colors: Vec<Color>,
}

#[derive(Debug, Clone)]
pub enum SequenceEffectThumbnailResult {
    Ready(SequenceEffectThumbnail),
    Unavailable,
    Cancelled,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct SequenceEffectThumbnailCacheKey {
    sequence_path: String,
    object_key: String,
    effect_id: u32,
    duration_nanoseconds: u64,
    frame_rate: u32,
    scope: SequenceEffectScope,
    script_id: EffectScriptId,
    script_source: String,
    params: Vec<PreparedEffectParamCacheKey>,
    target_pixels: Vec<PreparedEffectPixelCacheKey>,
    sampled_pixel_indices: Vec<usize>,
    max_columns: usize,
    max_rows: usize,
}

#[derive(Debug, Clone, Copy, Default)]
struct EffectChangeImpact {
    invalidate_prepared: bool,
    invalidate_topology: bool,
}

fn sequence_source_requires_full_clear(
    previous: &SequenceDocument,
    refreshed: &SequenceDocument,
) -> bool {
    previous.path != refreshed.path
        || previous.object_key != refreshed.object_key
        || previous.frame_rate != refreshed.frame_rate
        || previous.degraded != refreshed.degraded
}

fn effect_change_impact(
    previous_document: &SequenceDocument,
    refreshed_document: &SequenceDocument,
    previous: &dawn_language::document::SequenceEffectDocument,
    refreshed: &dawn_language::document::SequenceEffectDocument,
    analysis: &ProjectAnalysis,
) -> EffectChangeImpact {
    let mut impact = EffectChangeImpact::default();
    if previous.index != refreshed.index
        || previous.start_seconds != refreshed.start_seconds
        || previous.duration_seconds != refreshed.duration_seconds
        || previous.target != refreshed.target
        || previous.scope != refreshed.scope
        || previous.script != refreshed.script
        || previous.script_source != refreshed.script_source
        || render_shape_changed(previous.render.as_ref(), refreshed.render.as_ref())
    {
        impact.invalidate_prepared = true;
    }

    match (previous.render.as_ref(), refreshed.render.as_ref()) {
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
                analysis,
            ) {
                impact.invalidate_topology = true;
            }
        }
        (None, Some(_)) | (Some(_), None) => {
            impact.invalidate_prepared = true;
            if is_generator_effect(analysis, previous) || is_generator_effect(analysis, refreshed) {
                impact.invalidate_topology = true;
            }
        }
        (None, None) => {}
    }

    impact
}

fn render_shape_changed(
    previous: Option<&dawn_language::document::SequenceEffectRenderDocument>,
    refreshed: Option<&dawn_language::document::SequenceEffectRenderDocument>,
) -> bool {
    match (previous, refreshed) {
        (Some(previous), Some(refreshed)) => {
            previous.script != refreshed.script
                || previous.script_source != refreshed.script_source
                || previous.target_pixels.len() != refreshed.target_pixels.len()
                || previous.params.len() != refreshed.params.len()
        }
        (None, None) => false,
        (None, Some(_)) | (Some(_), None) => true,
    }
}

fn generator_topology_key_changed(
    previous_document: &SequenceDocument,
    refreshed_document: &SequenceDocument,
    previous: &dawn_language::document::SequenceEffectDocument,
    refreshed: &dawn_language::document::SequenceEffectDocument,
    previous_render: &dawn_language::document::SequenceEffectRenderDocument,
    refreshed_render: &dawn_language::document::SequenceEffectRenderDocument,
    analysis: &ProjectAnalysis,
) -> bool {
    let script_id = refreshed_render.script.to_script_id();
    let Some(script) = analysis.compiled_script_for_id(&script_id) else {
        return false;
    };
    if script.kind != EffectScriptKind::Generator {
        return false;
    }
    let topology_param_names = script
        .generator_statements()
        .map(|statements| {
            let param_names = script
                .params
                .iter()
                .map(|param| param.name.clone())
                .collect::<Vec<_>>();
            generator_topology_param_names(statements, &param_names)
        })
        .unwrap_or_default();
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

fn is_generator_effect(
    analysis: &ProjectAnalysis,
    effect: &dawn_language::document::SequenceEffectDocument,
) -> bool {
    effect
        .render
        .as_ref()
        .and_then(|render| analysis.compiled_script_for_id(&render.script.to_script_id()))
        .is_some_and(|script| script.kind == EffectScriptKind::Generator)
}

fn changed_mark_collection_keys(
    previous: &SequenceDocument,
    refreshed: &SequenceDocument,
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
    previous: &SequenceDocument,
    refreshed: &SequenceDocument,
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
    effect: &dawn_language::document::SequenceEffectDocument,
    collection_key: &str,
) -> bool {
    effect_params_reference_mark_collection(&effect.params, collection_key)
        || effect.render.as_ref().is_some_and(|render| {
            effect_params_reference_mark_collection(&render.params, collection_key)
        })
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
    script_id: EffectScriptId,
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
    Marks {
        collection_key: String,
        local_seconds: Option<Vec<F64CacheKey>>,
    },
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
pub struct SequenceFrameEvaluator {
    source: OutputSourceMetadata,
    bounds: GeometryRenderBounds,
    fixture_templates: Vec<OutputFixtureFrame>,
    effects: Vec<PreparedSequenceEffect>,
    effect_indices_by_frame: Vec<Vec<usize>>,
    authored_intervals_by_id: HashMap<u32, EffectInterval>,
}

impl SequenceFrameEvaluator {
    pub fn new(analysis: &ProjectAnalysis, document: &SequenceDocument) -> Result<Self, String> {
        Self::new_filtered(analysis, document, None)
    }

    pub fn new_timed(
        analysis: &ProjectAnalysis,
        document: &SequenceDocument,
    ) -> Result<(Self, SequenceFrameEvaluatorPreparationTiming), String> {
        Self::new_filtered_timed(analysis, document, None)
    }

    pub fn new_filtered(
        analysis: &ProjectAnalysis,
        document: &SequenceDocument,
        effect_filter: Option<&HashSet<u32>>,
    ) -> Result<Self, String> {
        Self::new_filtered_timed(analysis, document, effect_filter)
            .map(|(evaluator, _timing)| evaluator)
    }

    pub fn new_filtered_timed(
        analysis: &ProjectAnalysis,
        document: &SequenceDocument,
        effect_filter: Option<&HashSet<u32>>,
    ) -> Result<(Self, SequenceFrameEvaluatorPreparationTiming), String> {
        Self::new_filtered_timed_with_cache(analysis, document, effect_filter, None)
    }

    pub fn new_filtered_with_preparation_cache(
        analysis: &ProjectAnalysis,
        document: &SequenceDocument,
        effect_filter: Option<&HashSet<u32>>,
        preparation_cache: &mut SequencePreparationCache,
    ) -> Result<Self, String> {
        Self::new_filtered_timed_with_preparation_cache(
            analysis,
            document,
            effect_filter,
            preparation_cache,
        )
        .map(|(evaluator, _timing)| evaluator)
    }

    pub fn new_filtered_timed_with_preparation_cache(
        analysis: &ProjectAnalysis,
        document: &SequenceDocument,
        effect_filter: Option<&HashSet<u32>>,
        preparation_cache: &mut SequencePreparationCache,
    ) -> Result<(Self, SequenceFrameEvaluatorPreparationTiming), String> {
        Self::new_filtered_timed_with_cache(
            analysis,
            document,
            effect_filter,
            Some(preparation_cache),
        )
    }

    pub(crate) fn new_with_preparation_cache(
        analysis: &ProjectAnalysis,
        document: &SequenceDocument,
        preparation_cache: &mut SequencePreparationCache,
    ) -> Result<(Self, SequenceFrameEvaluatorPreparationTiming), String> {
        Self::new_filtered_timed_with_cache(analysis, document, None, Some(preparation_cache))
    }

    fn new_filtered_timed_with_cache(
        analysis: &ProjectAnalysis,
        document: &SequenceDocument,
        effect_filter: Option<&HashSet<u32>>,
        mut preparation_cache: Option<&mut SequencePreparationCache>,
    ) -> Result<(Self, SequenceFrameEvaluatorPreparationTiming), String> {
        let total_started = Instant::now();
        let Some(project) = analysis.resolved.as_ref() else {
            return Err("Project must resolve before preview is available".to_string());
        };
        let layout_started = Instant::now();
        let render_plan = layout_render_plan(&project.display.layout.fixtures);
        let fixture_templates = render_plan
            .fixtures
            .iter()
            .zip(project.display.layout.fixtures.iter())
            .map(|(plan, fixture)| OutputFixtureFrame {
                id: fixture.id,
                name: fixture.name.clone(),
                bulb_radius: plan.bulb_radius,
                pixels: plan
                    .emitters
                    .iter()
                    .map(|position| OutputPixelFrame {
                        position: *position,
                        color: Color::new(0, 0, 0),
                    })
                    .collect(),
            })
            .collect::<Vec<_>>();
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
        for effect in document.effects.iter().filter(|effect| {
            effect_filter
                .map(|ids| ids.contains(&effect.id))
                .unwrap_or(true)
        }) {
            let Some(render) = effect.render.as_ref() else {
                continue;
            };
            let cache_key = prepared_effect_cache_key(
                document,
                effect.start_seconds,
                effect.duration_seconds,
                effect.scope,
                render,
            );
            let script_id = render.script.to_script_id();
            match analysis.compiled_script_for_id(&script_id) {
                Some(script) if script.kind == EffectScriptKind::Generator => {
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
                                .generator_statements()
                                .map(|statements| {
                                    let param_names = script
                                        .params
                                        .iter()
                                        .map(|param| param.name.clone())
                                        .collect::<Vec<_>>();
                                    generator_topology_param_names(statements, &param_names)
                                })
                                .unwrap_or_default();
                            let topology_cache_key = prepared_effect_cache_key_for_params(
                                document,
                                effect.start_seconds,
                                effect.duration_seconds,
                                effect.scope,
                                render,
                                Some(&topology_param_names),
                            );
                            let prepared = match preparation_cache.as_deref().and_then(|cache| {
                                cache.generator_topology(effect.id, &topology_cache_key)
                            }) {
                                Some(children) => {
                                    topology_cache_hit = true;
                                    prepare_generated_effects_from_topology(
                                        GeneratedEffectTopologyInput {
                                            analysis,
                                            parent_path: parent_path_for_render(render),
                                            parent_id: effect.id,
                                            parent_start_seconds: effect.start_seconds,
                                            generator_id: script_id.clone(),
                                            generator: script,
                                            render,
                                            mark_collections: &document.mark_collections,
                                            fixture_templates: &fixture_templates,
                                            children,
                                        },
                                    )
                                }
                                None => {
                                    let topology = prepare_generated_topology(
                                        document,
                                        effect.start_seconds,
                                        effect.duration_seconds,
                                        effect.scope,
                                        script,
                                        render,
                                    );
                                    match topology {
                                        Ok(children) => {
                                            if let Some(cache) = preparation_cache.as_deref_mut() {
                                                cache.store_generator_topology(
                                                    effect.id,
                                                    topology_cache_key,
                                                    &children,
                                                );
                                            }
                                            prepare_generated_effects_from_topology(
                                                GeneratedEffectTopologyInput {
                                                    analysis,
                                                    parent_path: parent_path_for_render(render),
                                                    parent_id: effect.id,
                                                    parent_start_seconds: effect.start_seconds,
                                                    generator_id: script_id.clone(),
                                                    generator: script,
                                                    render,
                                                    mark_collections: &document.mark_collections,
                                                    fixture_templates: &fixture_templates,
                                                    children,
                                                },
                                            )
                                        }
                                        Err(error) => Err(error),
                                    }
                                }
                            }
                            .unwrap_or_else(|error| {
                                vec![PreparedSequenceEffect {
                                    id: effect.id,
                                    start_seconds: effect.start_seconds,
                                    duration_seconds: effect.duration_seconds,
                                    authored: true,
                                    render: PreparedEffectRender::BadParams(error),
                                }]
                            });
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
                        id: effect.id,
                        start_seconds: effect.start_seconds,
                        duration_seconds: effect.duration_seconds,
                        authored: true,
                        render: prepare_sample_render(
                            script,
                            &render.params,
                            &document.mark_collections,
                            effect.start_seconds,
                            effect.scope,
                            &render.target_pixels,
                            &fixture_templates,
                        ),
                    }];
                    if let Some(cache) = preparation_cache.as_deref_mut() {
                        cache.store(effect.id, cache_key, effect.start_seconds, &prepared);
                    }
                    authored_sample_ms += elapsed_ms(sample_started);
                    effects.extend(prepared);
                }
                None => {
                    let sample_started = Instant::now();
                    if let Some(prepared) = preparation_cache.as_deref().and_then(|cache| {
                        cache.prepared_effects(effect.id, &cache_key, effect.start_seconds)
                    }) {
                        authored_sample_ms += elapsed_ms(sample_started);
                        effects.extend(prepared);
                        continue;
                    }
                    let prepared = vec![PreparedSequenceEffect {
                        id: effect.id,
                        start_seconds: effect.start_seconds,
                        duration_seconds: effect.duration_seconds,
                        authored: true,
                        render: PreparedEffectRender::MissingScript(script_id.clone()),
                    }];
                    if let Some(cache) = preparation_cache.as_deref_mut() {
                        cache.store(effect.id, cache_key, effect.start_seconds, &prepared);
                    }
                    authored_sample_ms += elapsed_ms(sample_started);
                    effects.extend(prepared);
                }
            }
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
        let authored_intervals_by_id = document
            .effects
            .iter()
            .map(|effect| {
                (
                    effect.id,
                    EffectInterval {
                        start_seconds: effect.start_seconds,
                        duration_seconds: effect.duration_seconds,
                    },
                )
            })
            .collect();

        let timing = SequenceFrameEvaluatorPreparationTiming {
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

        Ok((
            Self {
                source,
                bounds: render_plan.bounds,
                fixture_templates,
                effects,
                effect_indices_by_frame,
                authored_intervals_by_id,
            },
            timing,
        ))
    }

    pub fn evaluate(&mut self, time_seconds: f64, generation: u64) -> OutputFrame {
        self.evaluate_timed(time_seconds, generation).0
    }

    pub fn evaluate_timed(
        &mut self,
        time_seconds: f64,
        generation: u64,
    ) -> (OutputFrame, SequenceFrameEvaluationTiming) {
        let total_started = Instant::now();
        let clone_started = Instant::now();
        let mut fixtures = self.fixture_templates.clone();
        let fixture_clone_ms = elapsed_ms(clone_started);
        let mut status = OutputFrameStatus::Live;
        let mut counters = SequenceEffectEvaluationCounters::default();

        let effect_loop_started = Instant::now();
        if let Some(effect_indices) = self.effect_indices_for_time(time_seconds) {
            for effect_index in effect_indices.clone() {
                evaluate_prepared_effect_at_time(
                    &mut self.effects[effect_index],
                    time_seconds,
                    &mut fixtures,
                    &mut status,
                    &mut counters,
                );
            }
        }
        let effect_loop_ms = elapsed_ms(effect_loop_started);

        let output_started = Instant::now();
        let frame = self.output_frame(time_seconds, generation, status, fixtures);
        let output_frame_ms = elapsed_ms(output_started);
        let total_ms = elapsed_ms(total_started);
        (
            frame,
            SequenceFrameEvaluationTiming {
                total_ms,
                fixture_clone_ms,
                effect_loop_ms,
                output_frame_ms,
                active_effects: counters.active_prepared_effects,
                active_authored_effects: counters.active_authored_effects,
                active_prepared_effects: counters.active_prepared_effects,
                visited_prepared_effects: counters.visited_prepared_effects,
                sampled_pixels: counters.sampled_pixels,
            },
        )
    }

    pub fn evaluate_effect_preview(
        &mut self,
        preview_seconds: f64,
        generation: u64,
    ) -> OutputFrame {
        self.evaluate_effect_preview_filtered(preview_seconds, generation, None)
    }

    pub fn evaluate_effect_preview_filtered(
        &mut self,
        preview_seconds: f64,
        generation: u64,
        effect_filter: Option<&HashSet<u32>>,
    ) -> OutputFrame {
        self.evaluate_effect_preview_filtered_timed(preview_seconds, generation, effect_filter)
            .0
    }

    pub fn evaluate_effect_preview_filtered_timed(
        &mut self,
        preview_seconds: f64,
        generation: u64,
        effect_filter: Option<&HashSet<u32>>,
    ) -> (OutputFrame, SequenceFrameEvaluationTiming) {
        let total_started = Instant::now();
        let clone_started = Instant::now();
        let mut fixtures = self.fixture_templates.clone();
        let fixture_clone_ms = elapsed_ms(clone_started);
        let mut status = OutputFrameStatus::Live;
        let mut counters = SequenceEffectEvaluationCounters::default();

        let effect_loop_started = Instant::now();
        let preview_frame_times = self.preview_frame_times(preview_seconds, effect_filter);
        let mut visited_effect_indices = HashSet::new();
        for (preview_id, preview_frame_time) in preview_frame_times {
            if let Some(effect_indices) = self.effect_indices_for_time(preview_frame_time) {
                for effect_index in effect_indices.clone() {
                    if !visited_effect_indices.insert(effect_index) {
                        continue;
                    }
                    let effect = &mut self.effects[effect_index];
                    if effect.id != preview_id {
                        continue;
                    }
                    if effect_filter
                        .map(|ids| !ids.contains(&effect.id))
                        .unwrap_or(false)
                    {
                        continue;
                    }
                    evaluate_prepared_effect_at_time(
                        effect,
                        preview_frame_time,
                        &mut fixtures,
                        &mut status,
                        &mut counters,
                    );
                }
            }
        }
        let effect_loop_ms = elapsed_ms(effect_loop_started);

        let output_started = Instant::now();
        let frame = self.output_frame(preview_seconds, generation, status, fixtures);
        let output_frame_ms = elapsed_ms(output_started);
        let total_ms = elapsed_ms(total_started);
        (
            frame,
            SequenceFrameEvaluationTiming {
                total_ms,
                fixture_clone_ms,
                effect_loop_ms,
                output_frame_ms,
                active_effects: counters.active_prepared_effects,
                active_authored_effects: counters.active_authored_effects,
                active_prepared_effects: counters.active_prepared_effects,
                visited_prepared_effects: counters.visited_prepared_effects,
                sampled_pixels: counters.sampled_pixels,
            },
        )
    }

    pub fn prepared_effect_count(&self) -> usize {
        self.effects.len()
    }

    pub fn evaluate_generator_effect_thumbnail(
        &mut self,
        effect_id: u32,
        local_seconds_by_column: &[f64],
        sampled_pixels_by_row: &[SequenceEffectPixelDocument],
    ) -> Result<Vec<Color>, String> {
        match self.evaluate_generator_effect_thumbnail_cancellable(
            effect_id,
            local_seconds_by_column,
            sampled_pixels_by_row,
            || false,
        )? {
            EffectThumbnailColorsResult::Ready(colors) => Ok(colors),
            EffectThumbnailColorsResult::Cancelled => Ok(Vec::new()),
        }
    }

    fn evaluate_generator_effect_thumbnail_cancellable(
        &mut self,
        effect_id: u32,
        local_seconds_by_column: &[f64],
        sampled_pixels_by_row: &[SequenceEffectPixelDocument],
        is_cancelled: impl Fn() -> bool,
    ) -> Result<EffectThumbnailColorsResult, String> {
        let interval = self
            .authored_intervals_by_id
            .get(&effect_id)
            .ok_or_else(|| format!("sequence effect `{effect_id}` was not found"))?;
        if interval.duration_seconds <= 0.0 {
            return Err(format!(
                "sequence effect `{effect_id}` must have a positive duration"
            ));
        }

        let mut row_indices_by_pixel = HashMap::new();
        for (row_index, pixel) in sampled_pixels_by_row.iter().enumerate() {
            if self
                .fixture_templates
                .get(pixel.fixture_index)
                .and_then(|fixture| fixture.pixels.get(pixel.pixel_index))
                .is_none()
            {
                return Err(format!(
                    "sequence effect `{effect_id}` references an unavailable preview pixel"
                ));
            }
            row_indices_by_pixel.insert((pixel.fixture_index, pixel.pixel_index), row_index);
        }

        let columns = local_seconds_by_column.len();
        let rows = sampled_pixels_by_row.len();
        let mut colors = vec![Color::new(0, 0, 0); columns * rows];

        for (column_index, local_seconds) in local_seconds_by_column.iter().copied().enumerate() {
            if is_cancelled() {
                return Ok(EffectThumbnailColorsResult::Cancelled);
            }
            if !local_seconds.is_finite() || local_seconds < 0.0 {
                return Err(format!(
                    "sequence effect `{effect_id}` has an invalid preview sample time"
                ));
            }
            let sequence_seconds = interval.start_seconds + local_seconds;
            let Some(effect_indices) = self.effect_indices_for_time(sequence_seconds).cloned()
            else {
                continue;
            };

            for effect_index in effect_indices {
                let effect = &mut self.effects[effect_index];
                if effect.id != effect_id {
                    continue;
                }
                sample_prepared_effect_thumbnail_column(
                    effect,
                    sequence_seconds,
                    column_index,
                    columns,
                    &row_indices_by_pixel,
                    &mut colors,
                    &is_cancelled,
                )?;
                if is_cancelled() {
                    return Ok(EffectThumbnailColorsResult::Cancelled);
                }
            }
        }

        Ok(EffectThumbnailColorsResult::Ready(colors))
    }

    fn effect_indices_for_time(&self, time_seconds: f64) -> Option<&Vec<usize>> {
        if !time_seconds.is_finite() || time_seconds < 0.0 {
            return None;
        }
        let frame_index = floor_frame(time_from_seconds_clamped(time_seconds), self.source.fps);
        self.effect_indices_by_frame
            .get(usize::try_from(frame_index).ok()?)
    }

    fn preview_frame_times(
        &self,
        preview_seconds: f64,
        effect_filter: Option<&HashSet<u32>>,
    ) -> Vec<(u32, f64)> {
        let ids = match effect_filter {
            Some(ids) => ids.iter().copied().collect::<Vec<_>>(),
            None => self
                .authored_intervals_by_id
                .keys()
                .copied()
                .collect::<Vec<_>>(),
        };
        ids.into_iter()
            .filter_map(|id| {
                let interval = self.authored_intervals_by_id.get(&id)?;
                (interval.duration_seconds > 0.0).then(|| {
                    (
                        id,
                        interval.start_seconds
                            + preview_seconds.rem_euclid(interval.duration_seconds),
                    )
                })
            })
            .collect()
    }

    fn output_frame(
        &self,
        time_seconds: f64,
        generation: u64,
        status: OutputFrameStatus,
        fixtures: Vec<OutputFixtureFrame>,
    ) -> OutputFrame {
        OutputFrame {
            source: self.source.clone(),
            time_seconds,
            generation,
            status,
            bounds: self.bounds,
            fixtures,
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct EffectInterval {
    start_seconds: f64,
    duration_seconds: f64,
}

#[derive(Debug, Clone, Copy, Default)]
struct SequenceEffectEvaluationCounters {
    active_authored_effects: u32,
    active_prepared_effects: u32,
    visited_prepared_effects: u32,
    sampled_pixels: u32,
}

fn evaluate_prepared_effect_at_time(
    effect: &mut PreparedSequenceEffect,
    time_seconds: f64,
    fixtures: &mut [OutputFixtureFrame],
    status: &mut OutputFrameStatus,
    counters: &mut SequenceEffectEvaluationCounters,
) {
    counters.visited_prepared_effects = counters.visited_prepared_effects.saturating_add(1);
    let local_seconds =
        if time_seconds < effect.start_seconds || time_seconds >= effect.end_seconds() {
            return;
        } else {
            time_seconds - effect.start_seconds
        };
    sample_prepared_effect(effect, local_seconds, fixtures, status, counters);
}

fn sample_prepared_effect(
    effect: &mut PreparedSequenceEffect,
    local_seconds: f64,
    fixtures: &mut [OutputFixtureFrame],
    status: &mut OutputFrameStatus,
    counters: &mut SequenceEffectEvaluationCounters,
) {
    let progress = if effect.duration_seconds == 0.0 {
        0.0
    } else {
        (local_seconds / effect.duration_seconds).clamp(0.0, 1.0)
    };

    let PreparedEffectRender::Ready {
        script,
        target_pixels,
        prepared_params,
        scratch,
        ..
    } = &mut effect.render
    else {
        *status = effect.render.error_status();
        return;
    };

    if effect.authored {
        counters.active_authored_effects = counters.active_authored_effects.saturating_add(1);
    }
    counters.active_prepared_effects = counters.active_prepared_effects.saturating_add(1);
    for pixel in target_pixels {
        let output_pixel = &mut fixtures[pixel.fixture_index].pixels[pixel.pixel_index];
        match script.sample_prepared_with_scratch(
            progress,
            local_seconds,
            pixel.fixture_context,
            pixel.pixel_context,
            prepared_params,
            scratch,
        ) {
            Ok(color) => add_clamped(&mut output_pixel.color, color),
            Err(error) => *status = OutputFrameStatus::Error(error.to_string()),
        }
        counters.sampled_pixels = counters.sampled_pixels.saturating_add(1);
    }
}

fn sample_prepared_effect_thumbnail_column(
    effect: &mut PreparedSequenceEffect,
    sequence_seconds: f64,
    column_index: usize,
    columns: usize,
    row_indices_by_pixel: &HashMap<(usize, usize), usize>,
    colors: &mut [Color],
    is_cancelled: impl Fn() -> bool,
) -> Result<(), String> {
    let local_seconds =
        if sequence_seconds < effect.start_seconds || sequence_seconds >= effect.end_seconds() {
            return Ok(());
        } else {
            sequence_seconds - effect.start_seconds
        };
    let progress = if effect.duration_seconds == 0.0 {
        0.0
    } else {
        (local_seconds / effect.duration_seconds).clamp(0.0, 1.0)
    };

    let PreparedEffectRender::Ready {
        script,
        target_pixels,
        prepared_params,
        scratch,
        ..
    } = &mut effect.render
    else {
        return Err(effect.render.error_message());
    };

    for pixel in target_pixels {
        if is_cancelled() {
            return Ok(());
        }
        let Some(row_index) = row_indices_by_pixel
            .get(&(pixel.fixture_index, pixel.pixel_index))
            .copied()
        else {
            continue;
        };
        let target_index = row_index
            .checked_mul(columns)
            .and_then(|row_start| row_start.checked_add(column_index))
            .ok_or_else(|| "effect thumbnail raster dimensions overflowed".to_string())?;
        let color = script
            .sample_prepared_with_scratch(
                progress,
                local_seconds,
                pixel.fixture_context,
                pixel.pixel_context,
                prepared_params,
                scratch,
            )
            .map_err(|error| error.to_string())?;
        add_clamped(&mut colors[target_index], color);
    }

    Ok(())
}

enum EffectThumbnailColorsResult {
    Ready(Vec<Color>),
    Cancelled,
}

fn sequence_effect_thumbnail(
    analysis: &ProjectAnalysis,
    document: &SequenceDocument,
    effect: &dawn_language::document::SequenceEffectDocument,
    max_columns: usize,
    max_rows: usize,
    cache: &mut SequenceRenderCache,
    is_cancelled: impl Fn() -> bool,
) -> Result<SequenceEffectThumbnailResult, String> {
    let Some(render) = &effect.render else {
        return Ok(SequenceEffectThumbnailResult::Unavailable);
    };
    if document.frame_rate == 0 || effect.duration_seconds == 0.0 || render.target_pixels.is_empty()
    {
        return Ok(SequenceEffectThumbnailResult::Unavailable);
    }

    let duration =
        TimeSpan::try_from_seconds_f64_rounded(effect.duration_seconds).map_err(str::to_string)?;
    if duration == TimeSpan::ZERO {
        return Ok(SequenceEffectThumbnailResult::Unavailable);
    }

    let source_pixel_count = render.target_pixels.len();
    let sampled_pixel_indices = evenly_sample_indices(source_pixel_count, max_rows);
    let cache_key = SequenceEffectThumbnailCacheKey {
        sequence_path: document.path.clone(),
        object_key: document.object_key.clone(),
        effect_id: effect.id,
        duration_nanoseconds: duration.as_nanoseconds(),
        frame_rate: document.frame_rate,
        scope: effect.scope,
        script_id: render.script.to_script_id(),
        script_source: render.script_source.clone(),
        params: prepared_effect_cache_key(
            document,
            effect.start_seconds,
            effect.duration_seconds,
            effect.scope,
            render,
        )
        .params,
        target_pixels: render
            .target_pixels
            .iter()
            .map(|pixel| PreparedEffectPixelCacheKey {
                fixture_index: pixel.fixture_index,
                pixel_index: pixel.pixel_index,
                pixel_count: pixel.pixel_count,
            })
            .collect(),
        sampled_pixel_indices: sampled_pixel_indices.clone(),
        max_columns,
        max_rows,
    };
    if let Some(thumbnail) = cache.effect_thumbnails.get(&cache_key).cloned() {
        return Ok(SequenceEffectThumbnailResult::Ready(thumbnail));
    }
    if is_cancelled() {
        return Ok(SequenceEffectThumbnailResult::Cancelled);
    }

    let Some(script) = analysis.compiled_script_for_id(&render.script.to_script_id()) else {
        return Ok(SequenceEffectThumbnailResult::Unavailable);
    };
    let sampled_frame_indices = evenly_sample_indices(
        total_preview_frames(duration, document.frame_rate),
        max_columns,
    );
    let colors = if script.kind == EffectScriptKind::Generator {
        generator_effect_thumbnail_colors(GeneratorEffectThumbnailInput {
            analysis,
            document,
            effect,
            render,
            duration,
            sampled_pixel_indices: &sampled_pixel_indices,
            sampled_frame_indices: &sampled_frame_indices,
            cache,
            is_cancelled: &is_cancelled,
        })?
    } else {
        sample_effect_thumbnail_colors(SampleEffectThumbnailInput {
            script,
            document,
            effect,
            render,
            duration,
            source_pixel_count,
            sampled_pixel_indices: &sampled_pixel_indices,
            sampled_frame_indices: &sampled_frame_indices,
            is_cancelled: &is_cancelled,
        })?
    };
    let colors = match colors {
        EffectThumbnailColorsResult::Ready(colors) => colors,
        EffectThumbnailColorsResult::Cancelled => {
            return Ok(SequenceEffectThumbnailResult::Cancelled);
        }
    };
    if colors.len() != sampled_frame_indices.len() * sampled_pixel_indices.len() {
        return Ok(SequenceEffectThumbnailResult::Unavailable);
    }
    let thumbnail = SequenceEffectThumbnail {
        effect_id: effect.id,
        duration_seconds: effect.duration_seconds,
        source_pixel_count: source_pixel_count.min(u32::MAX as usize) as u32,
        sampled_pixel_indices: sampled_pixel_indices
            .iter()
            .map(|index| (*index).min(u32::MAX as usize) as u32)
            .collect(),
        columns: sampled_frame_indices.len().min(u32::MAX as usize) as u32,
        rows: sampled_pixel_indices.len().min(u32::MAX as usize) as u32,
        colors,
    };
    cache.effect_thumbnails.insert(cache_key, thumbnail.clone());
    Ok(SequenceEffectThumbnailResult::Ready(thumbnail))
}

struct SampleEffectThumbnailInput<'a> {
    script: &'a CompiledEffect,
    document: &'a SequenceDocument,
    effect: &'a dawn_language::document::SequenceEffectDocument,
    render: &'a dawn_language::document::SequenceEffectRenderDocument,
    duration: TimeSpan,
    source_pixel_count: usize,
    sampled_pixel_indices: &'a [usize],
    sampled_frame_indices: &'a [usize],
    is_cancelled: &'a dyn Fn() -> bool,
}

fn sample_effect_thumbnail_colors(
    input: SampleEffectThumbnailInput<'_>,
) -> Result<EffectThumbnailColorsResult, String> {
    let prepared_params = match prepare_params_from_document(
        input.script,
        &input.render.params,
        &input.document.mark_collections,
        input.effect.start_seconds,
    ) {
        Ok(params) => params,
        Err(_) => return Ok(EffectThumbnailColorsResult::Ready(Vec::new())),
    };
    let mut colors =
        Vec::with_capacity(input.sampled_frame_indices.len() * input.sampled_pixel_indices.len());
    for target_pixel_index in input.sampled_pixel_indices {
        if (input.is_cancelled)() {
            return Ok(EffectThumbnailColorsResult::Cancelled);
        }
        let Some(pixel) = input.render.target_pixels.get(*target_pixel_index) else {
            return Ok(EffectThumbnailColorsResult::Ready(Vec::new()));
        };
        for frame_index in input.sampled_frame_indices {
            if (input.is_cancelled)() {
                return Ok(EffectThumbnailColorsResult::Cancelled);
            }
            let local_seconds =
                local_seconds_for_frame(*frame_index, input.document.frame_rate, input.duration);
            let progress = (local_seconds / input.effect.duration_seconds).clamp(0.0, 1.0);
            let pixel_context = pixel_context_for_effect(
                input.effect.scope,
                *target_pixel_index,
                input.source_pixel_count,
                pixel.pixel_index,
                pixel.pixel_count,
            );
            let color = match input.script.sample_prepared(
                progress,
                local_seconds,
                FixtureContext {
                    index: pixel.fixture_index,
                },
                pixel_context,
                &prepared_params,
            ) {
                Ok(color) => color,
                Err(_) => return Ok(EffectThumbnailColorsResult::Ready(Vec::new())),
            };
            colors.push(color);
        }
    }
    Ok(EffectThumbnailColorsResult::Ready(colors))
}

struct GeneratorEffectThumbnailInput<'a> {
    analysis: &'a ProjectAnalysis,
    document: &'a SequenceDocument,
    effect: &'a dawn_language::document::SequenceEffectDocument,
    render: &'a dawn_language::document::SequenceEffectRenderDocument,
    duration: TimeSpan,
    sampled_pixel_indices: &'a [usize],
    sampled_frame_indices: &'a [usize],
    cache: &'a mut SequenceRenderCache,
    is_cancelled: &'a dyn Fn() -> bool,
}

fn generator_effect_thumbnail_colors(
    input: GeneratorEffectThumbnailInput<'_>,
) -> Result<EffectThumbnailColorsResult, String> {
    if (input.is_cancelled)() {
        return Ok(EffectThumbnailColorsResult::Cancelled);
    }
    let filter = [input.effect.id].into_iter().collect();
    let (mut evaluator, _) = SequenceFrameEvaluator::new_filtered_timed_with_preparation_cache(
        input.analysis,
        input.document,
        Some(&filter),
        &mut input.cache.preparation,
    )?;
    let local_seconds_by_column = input
        .sampled_frame_indices
        .iter()
        .map(|frame_index| {
            local_seconds_for_frame(*frame_index, input.document.frame_rate, input.duration)
        })
        .collect::<Vec<_>>();
    let sampled_pixels_by_row = input
        .sampled_pixel_indices
        .iter()
        .map(|target_pixel_index| input.render.target_pixels.get(*target_pixel_index).cloned())
        .collect::<Option<Vec<_>>>()
        .ok_or_else(|| "effect thumbnail references an unavailable target pixel".to_string())?;
    evaluator.evaluate_generator_effect_thumbnail_cancellable(
        input.effect.id,
        &local_seconds_by_column,
        &sampled_pixels_by_row,
        input.is_cancelled,
    )
}

fn total_preview_frames(duration: TimeSpan, frame_rate: u32) -> usize {
    frame_count(duration, frame_rate).max(1)
}

fn local_seconds_for_frame(frame_index: usize, frame_rate: u32, duration: TimeSpan) -> f64 {
    let local_nanoseconds = frame_start(frame_index as u64, frame_rate)
        .as_nanoseconds()
        .min(duration.as_nanoseconds().saturating_sub(1));
    local_nanoseconds as f64 / 1_000_000_000.0
}

fn evenly_sample_indices(source_count: usize, max_count: usize) -> Vec<usize> {
    if source_count == 0 || max_count == 0 {
        return Vec::new();
    }
    let count = source_count.min(max_count);
    if count == 1 {
        return vec![0];
    }
    (0..count)
        .map(|index| {
            ((index as f64) * ((source_count - 1) as f64) / ((count - 1) as f64)).round() as usize
        })
        .collect()
}

fn prepare_sample_render(
    script: &CompiledEffect,
    params: &[SequenceEffectParamDocument],
    mark_collections: &[SequenceMarkCollectionDocument],
    effect_start_seconds: f64,
    scope: SequenceEffectScope,
    target_pixels: &[dawn_language::document::SequenceEffectPixelDocument],
    fixture_templates: &[OutputFixtureFrame],
) -> PreparedEffectRender {
    match prepare_params_from_document(script, params, mark_collections, effect_start_seconds) {
        Ok(prepared_params) => PreparedEffectRender::Ready {
            script: Box::new(script.clone()),
            target_pixels: prepare_effect_pixels(scope, target_pixels, fixture_templates),
            prepared_params,
            scratch: Box::new(EffectSampleScratch::new(script.bytecode_stats())),
            _bytecode_stats: script.bytecode_stats(),
        },
        Err(error) => PreparedEffectRender::BadParams(error),
    }
}

fn prepared_effect_cache_key(
    document: &SequenceDocument,
    effect_start_seconds: f64,
    duration_seconds: f64,
    scope: SequenceEffectScope,
    render: &dawn_language::document::SequenceEffectRenderDocument,
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
    document: &SequenceDocument,
    effect_start_seconds: f64,
    duration_seconds: f64,
    scope: SequenceEffectScope,
    render: &dawn_language::document::SequenceEffectRenderDocument,
    included_params: Option<&BTreeSet<String>>,
) -> PreparedEffectCacheKey {
    PreparedEffectCacheKey {
        script_id: render.script.to_script_id(),
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

fn parent_path_for_render(
    render: &dawn_language::document::SequenceEffectRenderDocument,
) -> Utf8PathBuf {
    Utf8PathBuf::from(&render.script.path)
}

fn effect_param_cache_value(
    param: &EffectParam<Resolved>,
    mark_collections: &[SequenceMarkCollectionDocument],
    effect_start_seconds: f64,
) -> EffectParamCacheValue {
    match param {
        EffectParam::Integer { value } => EffectParamCacheValue::Integer(*value),
        EffectParam::Float { value } => EffectParamCacheValue::Float(F64CacheKey(*value)),
        EffectParam::Boolean { value } => EffectParamCacheValue::Boolean(*value),
        EffectParam::Enum { value } => EffectParamCacheValue::Enum(value.clone()),
        EffectParam::Flags { value } => EffectParamCacheValue::Flags(value.values.clone()),
        EffectParam::Color { value } => EffectParamCacheValue::Color(color_cache_key(*value)),
        EffectParam::Curve { curve } => EffectParamCacheValue::Curve(curve_cache_key(curve)),
        EffectParam::Marks { key } => {
            let mut local_seconds = mark_collections
                .iter()
                .find(|collection| collection.key == *key)
                .map(|collection| {
                    collection
                        .marks_seconds
                        .iter()
                        .map(|mark_seconds| F64CacheKey(*mark_seconds - effect_start_seconds))
                        .collect::<Vec<_>>()
                });
            if let Some(local_seconds) = local_seconds.as_mut() {
                local_seconds.sort_by(|left, right| left.0.total_cmp(&right.0));
            }
            EffectParamCacheValue::Marks {
                collection_key: key.clone(),
                local_seconds,
            }
        }
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

fn color_cache_key(color: Color) -> ColorCacheKey {
    ColorCacheKey {
        red: color.red,
        green: color.green,
        blue: color.blue,
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
    let frame_count = frame_count(sequence_duration, frame_rate);
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

fn prepare_generated_topology(
    document: &SequenceDocument,
    parent_start_seconds: f64,
    parent_duration_seconds: f64,
    parent_scope: SequenceEffectScope,
    generator: &CompiledEffect,
    render: &dawn_language::document::SequenceEffectRenderDocument,
) -> Result<Vec<GeneratedChildTopology>, RuntimeError> {
    let prepared_params = prepare_params_from_document(
        generator,
        &render.params,
        &document.mark_collections,
        parent_start_seconds,
    )?;
    let param_names = generator
        .params
        .iter()
        .map(|param| param.name.clone())
        .collect::<Vec<_>>();
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
    let statements = generator
        .generator_statements()
        .ok_or_else(|| RuntimeError {
            message: format!("effect `{}` is not a generator effect", generator.name),
        })?;
    let mut children = Vec::new();
    for target in targets {
        children.extend(run_generator_topology(
            statements,
            &prepared_params,
            &param_names,
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
    analysis: &'a ProjectAnalysis,
    parent_path: Utf8PathBuf,
    parent_id: u32,
    parent_start_seconds: f64,
    generator_id: EffectScriptId,
    generator: &'a CompiledEffect,
    render: &'a dawn_language::document::SequenceEffectRenderDocument,
    mark_collections: &'a [SequenceMarkCollectionDocument],
    fixture_templates: &'a [OutputFixtureFrame],
    children: Vec<GeneratedChildTopology>,
}

fn prepare_generated_effects_from_topology(
    input: GeneratedEffectTopologyInput<'_>,
) -> Result<Vec<PreparedSequenceEffect>, RuntimeError> {
    let prepared_parent_params = prepare_params_from_document(
        input.generator,
        &input.render.params,
        input.mark_collections,
        input.parent_start_seconds,
    )?;
    let mut effects = Vec::new();
    let mut stack = vec![input.generator_id];
    let mut child_count = 0;
    flatten_generated_children(
        GeneratedChildFlattenInput {
            analysis: input.analysis,
            parent_path: &input.parent_path,
            parent_id: input.parent_id,
            parent_start_seconds: input.parent_start_seconds,
            parent_script: input.generator,
            parent_params: &prepared_parent_params,
            fixture_templates: input.fixture_templates,
            children: input.children,
        },
        &mut stack,
        &mut child_count,
        &mut effects,
    )?;
    Ok(effects)
}

struct GeneratedChildFlattenInput<'a> {
    analysis: &'a ProjectAnalysis,
    parent_path: &'a Utf8PathBuf,
    parent_id: u32,
    parent_start_seconds: f64,
    parent_script: &'a CompiledEffect,
    parent_params: &'a PreparedEffectParams,
    fixture_templates: &'a [OutputFixtureFrame],
    children: Vec<GeneratedChildTopology>,
}

fn flatten_generated_children(
    input: GeneratedChildFlattenInput<'_>,
    stack: &mut Vec<EffectScriptId>,
    child_count: &mut usize,
    effects: &mut Vec<PreparedSequenceEffect>,
) -> Result<(), RuntimeError> {
    let param_names = input
        .parent_script
        .params
        .iter()
        .map(|param| param.name.clone())
        .collect::<Vec<_>>();
    for child in input.children {
        let child_ref = resolve_generated_child_effect(
            input.analysis,
            input.parent_path,
            input.parent_script,
            &child.effect,
        )?;
        let emitted_params =
            evaluate_generated_child_params(&child, input.parent_params, &param_names)?;
        let prepared_params = child_ref.script.prepare_params(&emitted_params)?;
        match child_ref.script.kind {
            EffectScriptKind::Sample => {
                if *child_count >= MAX_FLATTENED_GENERATED_CHILDREN {
                    return Err(RuntimeError {
                        message: format!(
                            "generator exceeded maximum flattened child count ({MAX_FLATTENED_GENERATED_CHILDREN})"
                        ),
                    });
                }
                *child_count += 1;
                effects.push(PreparedSequenceEffect {
                    id: input.parent_id,
                    start_seconds: input.parent_start_seconds + child.start_seconds,
                    duration_seconds: child.duration_seconds,
                    authored: false,
                    render: PreparedEffectRender::Ready {
                        script: Box::new(child_ref.script.clone()),
                        target_pixels: prepare_effect_pixels(
                            SequenceEffectScope::WholeTarget,
                            &sequence_effect_pixels_for_generator_target(&child.target),
                            input.fixture_templates,
                        ),
                        prepared_params,
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
                        .map(EffectScriptId::display_key)
                        .collect::<Vec<_>>();
                    cycle.push(child_ref.id.display_key());
                    return Err(RuntimeError {
                        message: format!("generator cycle detected: {}", cycle.join(" -> ")),
                    });
                }
                let statements =
                    child_ref
                        .script
                        .generator_statements()
                        .ok_or_else(|| RuntimeError {
                            message: format!(
                                "effect `{}` is not a generator effect",
                                child_ref.script.name
                            ),
                        })?;
                let child_param_names = child_ref
                    .script
                    .params
                    .iter()
                    .map(|param| param.name.clone())
                    .collect::<Vec<_>>();
                let nested_children = prepare_child_generator_topology(
                    statements,
                    &prepared_params,
                    &child_param_names,
                    child.target,
                    child.duration_seconds,
                )?;
                stack.push(child_ref.id.clone());
                flatten_generated_children(
                    GeneratedChildFlattenInput {
                        analysis: input.analysis,
                        parent_path: &child_ref.id.path,
                        parent_id: input.parent_id,
                        parent_start_seconds: input.parent_start_seconds + child.start_seconds,
                        parent_script: child_ref.script,
                        parent_params: &prepared_params,
                        fixture_templates: input.fixture_templates,
                        children: nested_children,
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
    id: EffectScriptId,
    script: &'a CompiledEffect,
}

fn resolve_generated_child_effect<'a>(
    analysis: &'a ProjectAnalysis,
    parent_path: &Utf8PathBuf,
    parent_script: &CompiledEffect,
    child: &GeneratedChildEffectRef,
) -> Result<ResolvedGeneratedChildEffect<'a>, RuntimeError> {
    let (child_path, child_name) = match child {
        GeneratedChildEffectRef::Local { name } => (parent_path.clone(), name.clone()),
        GeneratedChildEffectRef::Imported { alias, name } => {
            let import = parent_script
                .imports
                .iter()
                .find(|import| import.alias == *alias)
                .ok_or_else(|| RuntimeError {
                    message: format!("generator import alias `{alias}` was not found"),
                })?;
            (
                resolve_import_path(parent_path, &Utf8PathBuf::from(import.path.clone())),
                name.clone(),
            )
        }
    };
    let child_id = EffectScriptId::new(child_path, child_name.clone());
    let child_script = analysis
        .compiled_script_for_id(&child_id)
        .ok_or_else(|| RuntimeError {
            message: format!(
                "compiled child script `{}` was not found",
                child_id.display_key()
            ),
        })?;
    if child_script.name != child_name {
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
    statements: &[dawn_language::effect_script::Stmt],
    prepared_params: &PreparedEffectParams,
    param_names: &[String],
    target: GeneratorTarget,
    duration_seconds: f64,
) -> Result<Vec<GeneratedChildTopology>, RuntimeError> {
    let mut children = run_generator_topology(
        statements,
        prepared_params,
        param_names,
        target,
        duration_seconds,
    )?;
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
) -> Vec<dawn_language::document::SequenceEffectPixelDocument> {
    target
        .pixels
        .iter()
        .map(
            |pixel| dawn_language::document::SequenceEffectPixelDocument {
                fixture_index: pixel.fixture_index,
                pixel_index: pixel.pixel_index,
                pixel_count: pixel.pixel_count,
            },
        )
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
    id: u32,
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
        prepared_params: PreparedEffectParams,
        scratch: Box<EffectSampleScratch>,
        _bytecode_stats: BytecodeStats,
    },
    MissingScript(EffectScriptId),
    BadParams(RuntimeError),
}

#[derive(Debug, Clone)]
struct PreparedEffectPixel {
    fixture_index: usize,
    pixel_index: usize,
    fixture_context: FixtureContext,
    pixel_context: PixelContext,
}

impl PreparedEffectRender {
    fn error_status(&self) -> OutputFrameStatus {
        match self {
            Self::Ready { .. } => OutputFrameStatus::Live,
            Self::MissingScript(script_id) => OutputFrameStatus::Error(format!(
                "compiled script `{}` was not found",
                script_id.display_key()
            )),
            Self::BadParams(error) => OutputFrameStatus::Error(error.to_string()),
        }
    }

    fn error_message(&self) -> String {
        match self {
            Self::Ready { .. } => "effect render is ready".to_string(),
            Self::MissingScript(script_id) => {
                format!(
                    "compiled script `{}` was not found",
                    script_id.display_key()
                )
            }
            Self::BadParams(error) => error.to_string(),
        }
    }
}

fn prepare_effect_pixels(
    scope: SequenceEffectScope,
    target_pixels: &[dawn_language::document::SequenceEffectPixelDocument],
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

pub fn evaluate_sequence_frame(
    analysis: &ProjectAnalysis,
    document: &SequenceDocument,
    time_seconds: f64,
    generation: u64,
) -> OutputFrame {
    match SequenceFrameEvaluator::new(analysis, document) {
        Ok(mut evaluator) => evaluator.evaluate(time_seconds, generation),
        Err(message) => empty_frame(generation, message),
    }
}

pub fn evaluate_sequence_frame_filtered(
    analysis: &ProjectAnalysis,
    document: &SequenceDocument,
    time_seconds: f64,
    generation: u64,
    effect_filter: Option<&HashSet<u32>>,
) -> OutputFrame {
    match SequenceFrameEvaluator::new_filtered(analysis, document, effect_filter) {
        Ok(mut evaluator) => evaluator.evaluate(time_seconds, generation),
        Err(message) => empty_frame(generation, message),
    }
}

pub fn evaluate_sequence_effect_preview_frame(
    analysis: &ProjectAnalysis,
    document: &SequenceDocument,
    preview_seconds: f64,
    generation: u64,
    effect_filter: &HashSet<u32>,
) -> OutputFrame {
    match SequenceFrameEvaluator::new_filtered(analysis, document, Some(effect_filter)) {
        Ok(mut evaluator) => evaluator.evaluate_effect_preview(preview_seconds, generation),
        Err(message) => empty_frame(generation, message),
    }
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
    params: &[SequenceEffectParamDocument],
    mark_collections: &[SequenceMarkCollectionDocument],
    effect_start_seconds: f64,
) -> BTreeMap<String, RuntimeValue> {
    params
        .iter()
        .filter_map(|param| {
            runtime_value_from_param(&param.value, mark_collections, effect_start_seconds)
                .map(|value| (param.name.clone(), value))
        })
        .collect()
}

pub fn prepare_params_from_document(
    script: &CompiledEffect,
    params: &[SequenceEffectParamDocument],
    mark_collections: &[SequenceMarkCollectionDocument],
    effect_start_seconds: f64,
) -> Result<PreparedEffectParams, RuntimeError> {
    script.prepare_params_with(|name| {
        params
            .iter()
            .find(|param| param.name == name)
            .and_then(|param| {
                runtime_value_from_param(&param.value, mark_collections, effect_start_seconds)
            })
    })
}

pub fn runtime_value_from_param(
    param: &EffectParam<Resolved>,
    mark_collections: &[SequenceMarkCollectionDocument],
    effect_start_seconds: f64,
) -> Option<RuntimeValue> {
    match param {
        EffectParam::Integer { value } => Some(RuntimeValue::Int(*value as i64)),
        EffectParam::Float { value } => Some(RuntimeValue::Float(*value)),
        EffectParam::Boolean { value } => Some(RuntimeValue::Bool(*value)),
        EffectParam::Enum { value } => Some(RuntimeValue::Enum(value.clone())),
        EffectParam::Flags { value } => Some(RuntimeValue::Flags(value.clone())),
        EffectParam::Color { value } => Some(RuntimeValue::Color(*value)),
        EffectParam::Curve { curve } => Some(RuntimeValue::Curve(curve.clone())),
        EffectParam::Marks { key } => {
            let mut marks = mark_collections
                .iter()
                .find(|collection| collection.key == *key)?
                .marks_seconds
                .iter()
                .map(|mark_seconds| *mark_seconds - effect_start_seconds)
                .collect::<Vec<_>>();
            marks.sort_by(f64::total_cmp);
            Some(RuntimeValue::Marks(marks))
        }
    }
}

pub fn empty_frame(generation: u64, message: impl Into<String>) -> OutputFrame {
    OutputFrame {
        source: OutputSourceMetadata {
            label: "No preview source".to_string(),
            kind: OutputSourceKind::Empty,
            duration_seconds: 0.0,
            fps: 0,
        },
        time_seconds: 0.0,
        generation,
        status: OutputFrameStatus::Idle(message.into()),
        bounds: GeometryRenderBounds {
            min_x: Distance::from_micrometers(-5_000_000),
            min_y: Distance::from_micrometers(-4_000_000),
            max_x: Distance::from_micrometers(5_000_000),
            max_y: Distance::from_micrometers(4_000_000),
        },
        fixtures: Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use dawn_language::analysis::{analyze_project, ProjectAnalysis};
    use dawn_language::document::{get_sequence_document, SequenceDocument};
    use dawn_language::fs::WorkspaceFs;
    use dawn_language::model::{
        Color, CurveValue, Distance, EffectParam, EffectScriptId, Resolved, SequenceEffectScope,
    };
    use dawn_language::path::{utf8_path, Utf8PathBuf};
    use dawn_language::render::GeometryRenderBounds;

    use dawn_language::effect_script::{GeneratorTarget, GeneratorTargetPixel};

    use super::{
        build_effect_indices_by_frame, generator_targets_for_scope, pixel_context_for_effect,
        OutputFixtureFrame, OutputFrame, OutputFrameStatus, OutputSourceKind, OutputSourceMetadata,
        PreparedEffectRender, PreparedSequenceEffect, SequenceChangeImpact, SequenceFrameEvaluator,
        SequenceRenderCache,
    };

    fn club_rig_project_path() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/club-rig/project.dawn")
    }

    fn club_rig_context() -> (WorkspaceFs, Utf8PathBuf, Utf8PathBuf) {
        let project_path = club_rig_project_path();
        let root = project_path
            .parent()
            .expect("club rig project should have a parent");
        let fs = WorkspaceFs::open(root).expect("club rig root should open");
        let project_path = utf8_path(
            project_path
                .strip_prefix(root)
                .expect("project path should be under root"),
        )
        .expect("project path should be valid UTF-8");
        let sequence_path = utf8_path(Path::new("sequences/opening.sequence.dawn"))
            .expect("sequence path should be valid UTF-8");
        (fs, project_path, sequence_path)
    }

    fn club_rig_analysis_and_sequence() -> (ProjectAnalysis, SequenceDocument) {
        let (fs, project_path, sequence_path) = club_rig_context();
        let analysis = analyze_project(&fs, project_path.clone(), "club_rig");
        assert!(
            analysis.diagnostics.is_empty(),
            "{:?}",
            analysis.diagnostics
        );
        let document =
            get_sequence_document(&fs, sequence_path, "opening", project_path, Vec::new())
                .expect("club rig sequence should load");
        (analysis, document)
    }

    fn thirty_output_controller_analysis_and_sequence() -> (ProjectAnalysis, SequenceDocument) {
        let project_path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../examples/thirty-output-controller/project.dawn");
        let root = project_path
            .parent()
            .expect("thirty output controller project should have a parent");
        let fs = WorkspaceFs::open(root).expect("thirty output controller root should open");
        let project_path = utf8_path(
            project_path
                .strip_prefix(root)
                .expect("project path should be under root"),
        )
        .expect("project path should be valid UTF-8");
        let sequence_path = utf8_path(Path::new("sequences/empty.sequence.dawn"))
            .expect("sequence path should be valid UTF-8");
        let analysis = analyze_project(&fs, project_path.clone(), "thirty_output_controller");
        assert!(
            analysis.diagnostics.is_empty(),
            "{:?}",
            analysis.diagnostics
        );
        let document = get_sequence_document(&fs, sequence_path, "empty", project_path, Vec::new())
            .expect("thirty output controller sequence should load");
        (analysis, document)
    }

    fn mutate_int_param(
        document: &mut SequenceDocument,
        effect_id: u32,
        param_name: &str,
        value: u64,
    ) {
        let param = render_param_mut(document, effect_id, param_name);
        match &mut param.value {
            EffectParam::<Resolved>::Integer { value: current } => *current = value,
            _ => panic!("expected integer param `{param_name}`"),
        }
    }

    fn mutate_curve_point(
        document: &mut SequenceDocument,
        effect_id: u32,
        param_name: &str,
        point_index: usize,
        value: f64,
    ) {
        let param = render_param_mut(document, effect_id, param_name);
        match &mut param.value {
            EffectParam::<Resolved>::Curve { curve } => {
                curve.points[point_index].value = CurveValue::Float(value);
            }
            _ => panic!("expected curve param `{param_name}`"),
        }
    }

    fn render_param_mut<'a>(
        document: &'a mut SequenceDocument,
        effect_id: u32,
        param_name: &str,
    ) -> &'a mut dawn_language::document::SequenceEffectParamDocument {
        document
            .effects
            .iter_mut()
            .find(|effect| effect.id == effect_id)
            .and_then(|effect| effect.render.as_mut())
            .and_then(|render| {
                render
                    .params
                    .iter_mut()
                    .find(|param| param.name == param_name)
            })
            .unwrap_or_else(|| panic!("effect `{effect_id}` param `{param_name}` should exist"))
    }

    fn generator_parent_timing(
        timing: &super::SequenceFrameEvaluatorPreparationTiming,
        effect_id: u32,
    ) -> &super::GeneratorParentPreparationTiming {
        timing
            .generator_parents
            .iter()
            .find(|parent| parent.parent_effect_id == effect_id)
            .unwrap_or_else(|| panic!("generator parent `{effect_id}` should be timed"))
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
        let (analysis, document) = thirty_output_controller_analysis_and_sequence();
        let mut edited = document.clone();
        mutate_curve_point(&mut edited, 3, "pulse_shape", 1, 0.25);

        let impact = SequenceChangeImpact::between(&document, &edited, &analysis);

        assert_only_invalidated(&impact, &[3], &[]);
    }

    #[test]
    fn sequence_change_impact_invalidates_only_chase_topology_param_entries() {
        let (analysis, document) = thirty_output_controller_analysis_and_sequence();
        let mut edited = document.clone();
        mutate_int_param(&mut edited, 8, "section_width_pixels", 7);

        let impact = SequenceChangeImpact::between(&document, &edited, &analysis);

        assert_only_invalidated(&impact, &[8], &[8]);
    }

    #[test]
    fn sequence_render_cache_keeps_unrelated_generator_prepared_hits_across_local_edits() {
        let (analysis, document) = thirty_output_controller_analysis_and_sequence();
        let mut cache = SequenceRenderCache::default();
        let (_, initial_timing) = cache
            .build_evaluator(&analysis, &document)
            .expect("initial evaluator should build");
        assert!(
            initial_timing.generator_parents.len() > 2,
            "example should contain multiple generator effects"
        );

        let mut edited = document.clone();
        mutate_curve_point(&mut edited, 3, "pulse_shape", 1, 0.25);
        let impact = SequenceChangeImpact::between(&document, &edited, &analysis);
        cache.apply_change_impact(&impact);
        let (_, edited_timing) = cache
            .build_evaluator(&analysis, &edited)
            .expect("edited evaluator should build");

        let edited_parent = generator_parent_timing(&edited_timing, 3);
        let unrelated_parent = generator_parent_timing(&edited_timing, 8);
        assert!(!edited_parent.prepared_cache_hit);
        assert!(edited_parent.topology_cache_hit);
        assert!(unrelated_parent.prepared_cache_hit);
    }

    #[test]
    fn sequence_render_cache_deleting_effect_prunes_only_that_effect_entries() {
        let (analysis, document) = thirty_output_controller_analysis_and_sequence();
        let mut cache = SequenceRenderCache::default();
        cache
            .build_evaluator(&analysis, &document)
            .expect("initial evaluator should build");
        let initial_prepared_entries = cache.prepared_entry_count();
        let initial_topology_entries = cache.topology_entry_count();

        let mut edited = document.clone();
        edited.effects.retain(|effect| effect.id != 3);
        let impact = SequenceChangeImpact::between(&document, &edited, &analysis);
        cache.apply_change_impact(&impact);

        assert_eq!(cache.prepared_entry_count(), initial_prepared_entries - 1);
        assert_eq!(cache.topology_entry_count(), initial_topology_entries - 1);
    }

    fn frame_colors(frame: &OutputFrame) -> Vec<Color> {
        frame
            .fixtures
            .iter()
            .flat_map(|fixture| fixture.pixels.iter().map(|pixel| pixel.color))
            .collect()
    }

    fn lit_pixel_count(frame: &OutputFrame) -> usize {
        frame_colors(frame)
            .into_iter()
            .filter(|color| *color != Color::new(0, 0, 0))
            .count()
    }

    fn bad_effect(
        id: u32,
        start_seconds: f64,
        duration_seconds: f64,
        authored: bool,
    ) -> PreparedSequenceEffect {
        PreparedSequenceEffect {
            id,
            start_seconds,
            duration_seconds,
            authored,
            render: PreparedEffectRender::MissingScript(EffectScriptId::new(
                Utf8PathBuf::from("missing.effect.dawn"),
                "Missing",
            )),
        }
    }

    fn evaluator_for_effects(effects: Vec<PreparedSequenceEffect>) -> SequenceFrameEvaluator {
        let source = OutputSourceMetadata {
            label: "Test".to_string(),
            kind: OutputSourceKind::Sequence,
            duration_seconds: 3.0,
            fps: 10,
        };
        let effect_indices_by_frame =
            build_effect_indices_by_frame(&effects, source.duration_seconds, source.fps);
        SequenceFrameEvaluator {
            source,
            bounds: GeometryRenderBounds {
                min_x: Distance::from_micrometers(0),
                min_y: Distance::from_micrometers(0),
                max_x: Distance::from_micrometers(0),
                max_y: Distance::from_micrometers(0),
            },
            fixture_templates: Vec::<OutputFixtureFrame>::new(),
            effects,
            effect_indices_by_frame,
            authored_intervals_by_id: [(
                1,
                super::EffectInterval {
                    start_seconds: 0.0,
                    duration_seconds: 3.0,
                },
            )]
            .into_iter()
            .collect(),
        }
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
    fn prepared_timeline_index_visits_only_current_frame_bucket() {
        let mut evaluator = evaluator_for_effects(vec![
            bad_effect(1, 0.5, 0.2, true),
            bad_effect(2, 2.0, 0.2, true),
        ]);

        let (frame, timing) = evaluator.evaluate_timed(0.55, 0);

        assert!(matches!(frame.status, OutputFrameStatus::Error(_)));
        assert_eq!(timing.visited_prepared_effects, 1);
    }

    #[test]
    fn prepared_timeline_index_preserves_effect_boundaries() {
        let mut evaluator = evaluator_for_effects(vec![bad_effect(1, 0.5, 0.2, true)]);

        let (at_start, at_start_timing) = evaluator.evaluate_timed(0.5, 0);
        let (at_end, at_end_timing) = evaluator.evaluate_timed(0.7, 0);

        assert!(matches!(at_start.status, OutputFrameStatus::Error(_)));
        assert_eq!(at_start_timing.visited_prepared_effects, 1);
        assert!(matches!(at_end.status, OutputFrameStatus::Live));
        assert_eq!(at_end_timing.visited_prepared_effects, 0);
    }

    #[test]
    fn generated_children_are_indexed_by_their_own_interval() {
        let mut evaluator = evaluator_for_effects(vec![bad_effect(1, 1.2, 0.4, false)]);

        let (frame, timing) = evaluator.evaluate_timed(1.3, 0);

        assert!(matches!(frame.status, OutputFrameStatus::Error(_)));
        assert_eq!(timing.visited_prepared_effects, 1);
    }

    #[test]
    fn bad_prepared_renders_surface_errors_only_during_indexed_interval() {
        let mut evaluator = evaluator_for_effects(vec![bad_effect(1, 0.5, 0.2, true)]);

        let (before, before_timing) = evaluator.evaluate_timed(0.4, 0);
        let (during, during_timing) = evaluator.evaluate_timed(0.55, 0);
        let (after, after_timing) = evaluator.evaluate_timed(0.8, 0);

        assert!(matches!(before.status, OutputFrameStatus::Live));
        assert_eq!(before_timing.visited_prepared_effects, 0);
        assert!(matches!(during.status, OutputFrameStatus::Error(_)));
        assert_eq!(during_timing.visited_prepared_effects, 1);
        assert!(matches!(after.status, OutputFrameStatus::Live));
        assert_eq!(after_timing.visited_prepared_effects, 0);
    }

    #[test]
    fn generator_heavy_sequence_visits_only_indexed_prepared_children() {
        let (analysis, document) = thirty_output_controller_analysis_and_sequence();
        let mut evaluator =
            SequenceFrameEvaluator::new(&analysis, &document).expect("renderer should build");

        let (first_frame, timing) = evaluator.evaluate_timed(41.0, 1);
        let second_frame = evaluator.evaluate(41.0, 2);

        assert_eq!(frame_colors(&first_frame), frame_colors(&second_frame));
        assert!(evaluator.prepared_effect_count() > document.effects.len());
        assert!(timing.visited_prepared_effects < evaluator.prepared_effect_count() as u32);
        assert!(timing.active_prepared_effects >= timing.active_authored_effects);
    }

    #[test]
    fn reusable_sequence_evaluator_updates_frame_output_over_time() {
        let (analysis, document) = club_rig_analysis_and_sequence();
        let mut evaluator =
            SequenceFrameEvaluator::new(&analysis, &document).expect("renderer should build");

        let first = evaluator.evaluate(2.0, 1);
        let second = evaluator.evaluate(6.0, 2);

        assert_ne!(frame_colors(&first), frame_colors(&second));
        assert!(lit_pixel_count(&first) > 0);
        assert!(lit_pixel_count(&second) > 0);
    }

    #[test]
    fn selected_effect_preview_filters_the_reusable_evaluator() {
        let (analysis, document) = club_rig_analysis_and_sequence();
        let mut evaluator =
            SequenceFrameEvaluator::new(&analysis, &document).expect("renderer should build");
        let first_ids = [1].into_iter().collect();
        let second_ids = [23].into_iter().collect();

        let first = evaluator.evaluate_effect_preview_filtered(1.0, 1, Some(&first_ids));
        let second = evaluator.evaluate_effect_preview_filtered(1.0, 2, Some(&second_ids));

        assert_ne!(frame_colors(&first), frame_colors(&second));
        assert!(lit_pixel_count(&first) > 0);
        assert!(lit_pixel_count(&second) > 0);
    }
}
