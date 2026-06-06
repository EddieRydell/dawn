use std::{collections::HashSet, time::Instant};

use dawn_language::{
    analysis::ProjectAnalysis,
    document::SequenceDocument,
    sequence_render::{
        empty_frame, OutputFrameStatus, SequenceChangeImpact, SequenceFrameEvaluationTiming,
        SequenceFrameEvaluator, SequenceRenderCache,
    },
};

use crate::{
    preview::{PreviewRenderTiming, SequenceKey},
    types::RenderedFrame,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PreviewRenderMode {
    Sequence,
    EffectPreview,
}

#[derive(Debug, Clone)]
pub struct PreviewFrameRenderTask {
    pub id: u64,
    pub dirty_revision: u64,
    pub generation: u64,
    pub key: SequenceKey,
    pub analysis: Option<ProjectAnalysis>,
    pub document: SequenceDocument,
    pub position_seconds: f64,
    pub preview_seconds: f64,
    pub effect_filter: HashSet<u32>,
    pub status: String,
    pub mode: PreviewRenderMode,
}

#[derive(Debug, Clone)]
pub struct PreviewFrameRenderOutput {
    pub id: u64,
    pub dirty_revision: u64,
    pub generation: u64,
    pub key: SequenceKey,
    pub status: String,
    pub frame: RenderedFrame,
    pub timing: PreviewRenderTiming,
}

#[derive(Debug, Default)]
pub struct PreviewFrameExecutor {
    sequence_cache: SequenceRenderCache,
    render_cache: Option<SequenceFrameEvaluator>,
    previous_key: Option<SequenceKey>,
    previous_document: Option<SequenceDocument>,
}

impl PreviewFrameExecutor {
    pub fn render(&mut self, task: PreviewFrameRenderTask) -> PreviewFrameRenderOutput {
        let Some(analysis) = task.analysis.as_ref() else {
            let generation = task.generation;
            return self.output(task, empty_frame(generation, "No project analysis"), None);
        };

        self.apply_request_cache_invalidation(analysis, &task);
        let mut timing = PreviewRenderTiming::default();
        let frame = match self.cached_renderer(analysis, &task.document) {
            Ok((renderer, renderer_build_ms)) => {
                let (frame, evaluation_timing) = match task.mode {
                    PreviewRenderMode::Sequence => {
                        renderer.evaluate_timed(task.position_seconds, task.generation)
                    }
                    PreviewRenderMode::EffectPreview => renderer
                        .evaluate_effect_preview_filtered_timed(
                            task.preview_seconds,
                            task.generation,
                            Some(&task.effect_filter),
                        ),
                };
                timing = PreviewRenderTiming::from_evaluation(renderer_build_ms, evaluation_timing);
                frame
            }
            Err(message) => {
                let mut frame = empty_frame(task.generation, "Render failed");
                frame.status = OutputFrameStatus::Error(message);
                frame
            }
        };
        self.output(task, frame, Some(timing))
    }

    fn output(
        &mut self,
        task: PreviewFrameRenderTask,
        frame: dawn_language::sequence_render::OutputFrame,
        timing: Option<PreviewRenderTiming>,
    ) -> PreviewFrameRenderOutput {
        self.previous_key = Some(task.key.clone());
        self.previous_document = Some(task.document.clone());
        PreviewFrameRenderOutput {
            id: task.id,
            dirty_revision: task.dirty_revision,
            generation: task.generation,
            key: task.key,
            status: status_from_frame(&frame.status).unwrap_or(task.status),
            frame: frame.into(),
            timing: timing.unwrap_or_default(),
        }
    }

    fn apply_request_cache_invalidation(
        &mut self,
        analysis: &ProjectAnalysis,
        task: &PreviewFrameRenderTask,
    ) {
        if self.previous_key.as_ref() != Some(&task.key) {
            self.sequence_cache.clear();
            self.render_cache = None;
            return;
        }
        let Some(previous_document) = self.previous_document.as_ref() else {
            return;
        };
        let impact = SequenceChangeImpact::between(previous_document, &task.document, analysis);
        if impact.requires_full_clear()
            || !impact.invalidated_prepared_effect_ids().is_empty()
            || !impact.invalidated_topology_effect_ids().is_empty()
        {
            self.sequence_cache.apply_change_impact(&impact);
            self.render_cache = None;
        }
    }

    fn cached_renderer(
        &mut self,
        analysis: &ProjectAnalysis,
        document: &SequenceDocument,
    ) -> Result<(&mut SequenceFrameEvaluator, f64), String> {
        let mut renderer_build_ms = 0.0;
        if self.render_cache.is_none() {
            let build_started = Instant::now();
            let (renderer, _) = self.sequence_cache.build_evaluator(analysis, document)?;
            self.render_cache = Some(renderer);
            renderer_build_ms = build_started.elapsed().as_secs_f64() * 1000.0;
        }
        self.render_cache
            .as_mut()
            .map(|renderer| (renderer, renderer_build_ms))
            .ok_or_else(|| "Sequence preview renderer was not prepared".to_string())
    }
}

impl PreviewRenderTiming {
    pub fn from_evaluation(
        renderer_build_ms: f64,
        evaluation: SequenceFrameEvaluationTiming,
    ) -> Self {
        Self {
            total_ms: renderer_build_ms + evaluation.total_ms,
            renderer_build_ms,
            frame_evaluate_ms: evaluation.total_ms,
            fixture_clone_ms: evaluation.fixture_clone_ms,
            effect_loop_ms: evaluation.effect_loop_ms,
            output_frame_ms: evaluation.output_frame_ms,
            active_effects: evaluation.active_effects,
            sampled_pixels: evaluation.sampled_pixels,
        }
    }
}

fn status_from_frame(status: &OutputFrameStatus) -> Option<String> {
    match status {
        OutputFrameStatus::Live => None,
        OutputFrameStatus::Idle(message) | OutputFrameStatus::Error(message) => {
            Some(message.clone())
        }
    }
}
