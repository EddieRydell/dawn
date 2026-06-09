use crate::document::SequenceEditorDocument;
use crate::output_runtime::{
    OutputGeometryModel, RenderedOutputFrame, SequenceFrameRenderTiming, SequenceRenderPlan,
    SequenceRenderPlanBuildTiming,
};
use dawn_project::DawnProject;

#[derive(Debug, Clone)]
pub struct RenderFrameInput<'a> {
    pub project: &'a DawnProject,
    pub sequence: &'a SequenceEditorDocument,
    pub time_seconds: f64,
    pub generation: u64,
}

#[derive(Debug, Clone)]
pub struct RenderFrameOutput {
    pub geometry: OutputGeometryModel,
    pub frame: RenderedOutputFrame,
    pub diagnostics: Vec<RenderDiagnostic>,
    pub timing: RenderFrameTiming,
}

#[derive(Debug, Clone)]
pub struct RenderDiagnostic {
    pub message: String,
}

#[derive(Debug, Clone, Default)]
pub struct RenderFrameTiming {
    pub build: SequenceRenderPlanBuildTiming,
    pub frame: SequenceFrameRenderTiming,
}

#[derive(Debug, Clone)]
pub enum RenderError {
    Fatal(String),
}

impl std::fmt::Display for RenderError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Fatal(message) => write!(formatter, "{message}"),
        }
    }
}

impl std::error::Error for RenderError {}

pub fn render_sequence_frame(
    input: RenderFrameInput<'_>,
) -> Result<RenderFrameOutput, RenderError> {
    let (mut renderer, build_timing) =
        SequenceRenderPlan::new_timed(input.project, input.sequence).map_err(RenderError::Fatal)?;
    let geometry = renderer.geometry().clone();
    let (frame, frame_timing, diagnostics) =
        renderer.render_frame_timed_with_diagnostics(input.time_seconds, input.generation);

    Ok(RenderFrameOutput {
        geometry,
        frame,
        diagnostics: diagnostics
            .into_iter()
            .map(|message| RenderDiagnostic { message })
            .collect(),
        timing: RenderFrameTiming {
            build: build_timing,
            frame: frame_timing,
        },
    })
}
