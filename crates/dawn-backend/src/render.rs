use dawn_language::{
    analysis::ProjectAnalysis,
    document::SequenceDocument,
    sequence_render::{empty_frame, OutputFrameStatus, SequenceRenderCache},
};

use crate::{
    types::{
        ExportFseqTask, ExportFseqTaskOutput, FseqExportOptions, RenderEffectPreviewRequestEffect,
        RenderEffectPreviewTask, RenderEffectPreviewTaskOutput, RenderFrameTask,
        RenderFrameTaskOutput, RenderTaskId, RenderView, SequenceEffectPreviewErrorResult,
        SequenceEffectPreviewReadyResult, SequenceEffectPreviewResult,
        SequenceEffectPreviewUnavailableResult,
    },
    BackendError, BackendErrorKind, BackendResult,
};

const EFFECT_PREVIEW_MAX_COLUMNS: usize = 360;
const EFFECT_PREVIEW_MAX_ROWS: usize = 50;

#[derive(Debug, Default)]
pub(crate) struct Renderer {
    next_task_id: u64,
    latest_frame_task: Option<RenderTaskId>,
    latest_effect_preview_task: Option<RenderTaskId>,
    latest_export_task: Option<RenderTaskId>,
    cache: SequenceRenderCache,
    snapshot: RenderView,
}

impl Renderer {
    pub(crate) fn request_frame(
        &mut self,
        analysis: ProjectAnalysis,
        document: SequenceDocument,
        position_seconds: f64,
        generation: u64,
    ) -> RenderFrameTask {
        let id = self.next_id();
        self.latest_frame_task = Some(id);
        RenderFrameTask {
            id,
            analysis,
            document,
            position_seconds,
            generation,
            cache: self.cache.clone(),
        }
    }

    pub(crate) fn request_effect_previews(
        &mut self,
        analysis: ProjectAnalysis,
        document: SequenceDocument,
        effects: Vec<RenderEffectPreviewRequestEffect>,
    ) -> RenderEffectPreviewTask {
        let id = self.next_id();
        self.latest_effect_preview_task = Some(id);
        RenderEffectPreviewTask {
            id,
            analysis,
            document,
            effects,
            cache: self.cache.clone(),
        }
    }

    pub(crate) fn request_export_fseq(
        &mut self,
        analysis: ProjectAnalysis,
        document: SequenceDocument,
        output_path: camino::Utf8PathBuf,
        options: FseqExportOptions,
    ) -> ExportFseqTask {
        let id = self.next_id();
        self.latest_export_task = Some(id);
        ExportFseqTask {
            id,
            analysis,
            document,
            output_path,
            options,
            cache: self.cache.clone(),
        }
    }

    pub(crate) fn accept_frame(&mut self, output: RenderFrameTaskOutput) {
        if self.latest_frame_task != Some(output.id) {
            return;
        }
        self.cache = output.cache;
        self.snapshot.frame = Some(output.frame);
    }

    pub(crate) fn accept_effect_previews(&mut self, output: RenderEffectPreviewTaskOutput) {
        if self.latest_effect_preview_task != Some(output.id) {
            return;
        }
        self.cache = output.cache;
        self.snapshot.effect_previews = output.results;
    }

    pub(crate) fn accept_export(&mut self, output: ExportFseqTaskOutput) {
        if self.latest_export_task != Some(output.id) {
            return;
        }
        self.cache = output.cache;
        self.snapshot.export_report = Some(output.report);
    }

    pub(crate) fn snapshot(&self) -> RenderView {
        self.snapshot.clone()
    }

    fn next_id(&mut self) -> RenderTaskId {
        let id = RenderTaskId(self.next_task_id);
        self.next_task_id = self.next_task_id.saturating_add(1);
        id
    }
}

pub(crate) fn execute_render_frame(mut task: RenderFrameTask) -> RenderFrameTaskOutput {
    let frame = match task.cache.build_evaluator(&task.analysis, &task.document) {
        Ok((mut evaluator, _)) => evaluator.evaluate(task.position_seconds, task.generation),
        Err(message) => {
            let mut frame = empty_frame(task.generation, "Render failed");
            frame.status = OutputFrameStatus::Error(message);
            frame
        }
    };
    RenderFrameTaskOutput {
        id: task.id,
        frame: frame.into(),
        cache: task.cache,
    }
}

pub(crate) fn execute_effect_previews(
    mut task: RenderEffectPreviewTask,
) -> RenderEffectPreviewTaskOutput {
    let mut results = Vec::with_capacity(task.effects.len());
    for requested in task.effects {
        let Some(effect) = task
            .document
            .effects
            .iter()
            .find(|effect| effect.id == requested.effect_id)
        else {
            results.push(SequenceEffectPreviewResult::Unavailable(
                SequenceEffectPreviewUnavailableResult {
                    effect_id: requested.effect_id,
                    signature: requested.signature,
                },
            ));
            continue;
        };
        let result = task.cache.effect_thumbnail(
            &task.analysis,
            &task.document,
            effect,
            EFFECT_PREVIEW_MAX_COLUMNS,
            EFFECT_PREVIEW_MAX_ROWS,
        );
        match result {
            Ok(Some(thumbnail)) => {
                results.push(SequenceEffectPreviewResult::Ready(
                    SequenceEffectPreviewReadyResult {
                        signature: requested.signature,
                        preview: thumbnail.into(),
                    },
                ));
            }
            Ok(None) => {
                results.push(SequenceEffectPreviewResult::Unavailable(
                    SequenceEffectPreviewUnavailableResult {
                        effect_id: requested.effect_id,
                        signature: requested.signature,
                    },
                ));
            }
            Err(message) => {
                results.push(SequenceEffectPreviewResult::Error(
                    SequenceEffectPreviewErrorResult {
                        effect_id: requested.effect_id,
                        signature: requested.signature,
                        message,
                    },
                ));
            }
        }
    }
    RenderEffectPreviewTaskOutput {
        id: task.id,
        results,
        cache: task.cache,
    }
}

pub(crate) fn require_analysis(
    analysis: Option<ProjectAnalysis>,
) -> BackendResult<ProjectAnalysis> {
    analysis.ok_or_else(|| {
        BackendError::new(
            BackendErrorKind::InvalidInput,
            "project analysis is not available",
        )
    })
}

impl From<dawn_language::sequence_render::SequenceEffectThumbnail>
    for crate::types::SequenceEffectPreview
{
    fn from(value: dawn_language::sequence_render::SequenceEffectThumbnail) -> Self {
        Self {
            effect_id: value.effect_id,
            duration_seconds: value.duration_seconds,
            source_pixel_count: value.source_pixel_count,
            sampled_pixel_indices: value.sampled_pixel_indices,
            columns: value.columns,
            rows: value.rows,
            colors: value.colors,
        }
    }
}
