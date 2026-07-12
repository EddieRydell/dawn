use dawn_language::model::DawnProject;
use dawn_language::sequence::SequenceId;
use dawn_language::setup::SetupId;
use dawn_runtime::{PreparedSequenceRenderer, RenderError, RenderedFrame, SequenceRenderScratch};

use crate::dto::{AudioTransportSnapshot, AudioTransportState};

pub(crate) struct ShowRenderService {
    session: Option<RenderSession>,
    session_generation: u64,
}

pub struct PreparedRenderSession {
    setup_id: SetupId,
    sequence_id: SequenceId,
    renderer: PreparedSequenceRenderer,
}

#[derive(Clone, Debug, PartialEq)]
pub struct AudioClockRenderedFrame {
    pub audio_generation: u32,
    pub frame: RenderedFrame,
}

#[derive(Clone, Debug, PartialEq)]
pub struct AudioClockRenderIdentity {
    pub session_generation: u64,
    pub audio_generation: u32,
    pub audio_state: AudioTransportState,
    pub position_seconds: f64,
    pub frame_rate: u32,
    pub frame_count: u64,
    pub frame_index: u64,
}

#[derive(Clone, Debug, PartialEq)]
pub enum ShowRenderError {
    NoRenderSession,
    ClockUnavailable { state: AudioTransportState },
    Render(RenderError),
}

impl ShowRenderService {
    pub(crate) fn new() -> Self {
        Self {
            session: None,
            session_generation: 0,
        }
    }

    pub fn prepare(
        &mut self,
        project: &DawnProject,
        setup_id: &SetupId,
        sequence_id: &SequenceId,
    ) -> Result<(), RenderError> {
        let session = prepare_render_session(project, setup_id, sequence_id)?;
        self.apply_prepared(session);
        Ok(())
    }

    pub fn unload(&mut self) {
        if self.session.is_some() {
            self.session_generation = self.session_generation.saturating_add(1);
            self.session = None;
        }
    }

    pub fn refresh_project(&mut self, project: &DawnProject) -> Result<(), RenderError> {
        let Some(session) = self.session.as_ref() else {
            return Ok(());
        };
        let setup_id = session.setup_id.clone();
        let sequence_id = session.sequence_id.clone();
        if !project.sequences.contains_key(&sequence_id) {
            self.unload();
            return Ok(());
        }
        self.prepare(project, &setup_id, &sequence_id)
    }

    pub fn render_current_sequence_frame(
        &mut self,
        audio: &AudioTransportSnapshot,
    ) -> Result<AudioClockRenderedFrame, ShowRenderError> {
        let session = self
            .session
            .as_mut()
            .ok_or(ShowRenderError::NoRenderSession)?;
        match audio.state {
            AudioTransportState::Stopped
            | AudioTransportState::Paused
            | AudioTransportState::Playing
            | AudioTransportState::Ended => {}
            AudioTransportState::Unloaded | AudioTransportState::Error => {
                return Err(ShowRenderError::ClockUnavailable {
                    state: audio.state.clone(),
                });
            }
        }
        let frame = session
            .renderer
            .render_seconds_with_scratch(audio.position_seconds, &mut session.scratch)
            .map_err(ShowRenderError::Render)?;
        Ok(AudioClockRenderedFrame {
            audio_generation: audio.generation,
            frame,
        })
    }

    pub fn active_render_identity(
        &self,
        audio: &AudioTransportSnapshot,
    ) -> Result<AudioClockRenderIdentity, ShowRenderError> {
        let session = self
            .session
            .as_ref()
            .ok_or(ShowRenderError::NoRenderSession)?;
        match audio.state {
            AudioTransportState::Stopped
            | AudioTransportState::Paused
            | AudioTransportState::Playing
            | AudioTransportState::Ended => {}
            AudioTransportState::Unloaded | AudioTransportState::Error => {
                return Err(ShowRenderError::ClockUnavailable {
                    state: audio.state.clone(),
                });
            }
        }
        Ok(AudioClockRenderIdentity {
            session_generation: self.session_generation,
            audio_generation: audio.generation,
            audio_state: audio.state.clone(),
            position_seconds: audio.position_seconds,
            frame_rate: session.renderer.frame_rate(),
            frame_count: session.renderer.frame_count(),
            frame_index: frame_index_for_audio_seconds(
                audio.position_seconds,
                session.renderer.frame_rate(),
                session.renderer.frame_count(),
            ),
        })
    }

    pub fn active_target(&self) -> Option<(SetupId, SequenceId)> {
        self.session
            .as_ref()
            .map(|session| (session.setup_id.clone(), session.sequence_id.clone()))
    }

    #[cfg(test)]
    fn active_sequence_id(&self) -> Option<&SequenceId> {
        self.session.as_ref().map(|session| &session.sequence_id)
    }

    pub fn apply_prepared(&mut self, session: PreparedRenderSession) {
        self.session_generation = self.session_generation.saturating_add(1);
        self.session = Some(RenderSession {
            setup_id: session.setup_id,
            sequence_id: session.sequence_id,
            renderer: session.renderer,
            scratch: SequenceRenderScratch::default(),
        });
    }
}

struct RenderSession {
    setup_id: SetupId,
    sequence_id: SequenceId,
    renderer: PreparedSequenceRenderer,
    scratch: SequenceRenderScratch,
}

pub fn prepare_render_session(
    project: &DawnProject,
    setup_id: &SetupId,
    sequence_id: &SequenceId,
) -> Result<PreparedRenderSession, RenderError> {
    let renderer = PreparedSequenceRenderer::prepare(project, setup_id, sequence_id)?;
    Ok(PreparedRenderSession {
        setup_id: setup_id.clone(),
        sequence_id: sequence_id.clone(),
        renderer,
    })
}

fn frame_index_for_audio_seconds(audio_seconds: f64, frame_rate: u32, frame_count: u64) -> u64 {
    let max_frame = frame_count.saturating_sub(1);
    let frame_index = (audio_seconds * f64::from(frame_rate)).floor();
    if frame_index < 0.0 {
        0
    } else if frame_index > max_frame as f64 {
        max_frame
    } else {
        frame_index as u64
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use dawn_language::dsl::compile_effects;
    use dawn_language::effect::{
        EffectDefinition, EffectDefinitionId, EffectInst, EffectInstId, EffectScope, EffectTarget,
    };
    use dawn_language::identity::SourceIdentity;
    use dawn_language::model::{ProjectDefinitionStores, ProjectId, ProjectRoot};
    use dawn_language::sequence::{
        CompositionGraphNode, CompositionGraphNodeId, CompositionGraphNodeKind, EffectGraphEdge,
        GraphNodePosition, GraphPortId, Sequence, SequenceAudio, SequenceCompositionGraph,
        SequenceLayer, SequenceLayerId,
    };
    use dawn_language::setup::{
        ControllerDefinitionStore, FixtureDefinition, FixtureDefinitionId, FixtureDefinitionStore,
        FixtureGroup, FixtureGroupId, FixtureInst, FixtureInstanceId, Geometry, Layout, LayoutId,
        PatchId, Setup,
    };
    use dawn_language::values::{
        Color, DawnDuration, DawnTime, DistanceSpan, Point3, Rotation3, Scale3,
    };
    use indexmap::IndexMap;
    use std::time::Duration;

    fn source_identity(object: &str) -> SourceIdentity {
        SourceIdentity::new("test.dawn".into(), object.to_string())
    }

    #[test]
    fn prepares_unloads_rejects_unavailable_clocks_and_renders_loaded_states() {
        let project = project();
        let mut service = ShowRenderService::new();
        let setup_id = SetupId(source_identity("setup"));
        let sequence_id = SequenceId(source_identity("seq"));

        service.prepare(&project, &setup_id, &sequence_id).unwrap();
        assert_eq!(service.active_sequence_id(), Some(&sequence_id));
        assert!(matches!(
            service.render_current_sequence_frame(&snapshot(AudioTransportState::Unloaded, 0.0)),
            Err(ShowRenderError::ClockUnavailable { .. })
        ));
        assert!(matches!(
            service.render_current_sequence_frame(&snapshot(AudioTransportState::Error, 0.0)),
            Err(ShowRenderError::ClockUnavailable { .. })
        ));

        for state in [
            AudioTransportState::Stopped,
            AudioTransportState::Paused,
            AudioTransportState::Playing,
            AudioTransportState::Ended,
        ] {
            let rendered = service
                .render_current_sequence_frame(&snapshot(state, 0.25))
                .unwrap();
            assert_eq!(rendered.audio_generation, 7);
            assert_eq!(rendered.frame.clock_seconds, 0.25);
        }

        service.unload();
        assert!(matches!(
            service.render_current_sequence_frame(&snapshot(AudioTransportState::Paused, 0.0)),
            Err(ShowRenderError::NoRenderSession)
        ));
    }

    fn project() -> DawnProject {
        let mut effects = dawn_language::effect::EffectDefinitionStore::default();
        let compiled =
            compile_effects("effect Solid { color sample() { return rgb(1.0, 0.0, 0.0); } }")
                .unwrap()
                .into_iter()
                .next()
                .unwrap();
        effects.insert(
            EffectDefinitionId(SourceIdentity::new(
                "effects.dawn".into(),
                "Solid".to_string(),
            )),
            EffectDefinition { compiled },
        );

        let mut fixtures = FixtureDefinitionStore::default();
        fixtures.insert(
            FixtureDefinitionId(SourceIdentity::new(
                "fixtures.dawn".into(),
                "pixel".to_string(),
            )),
            FixtureDefinition {
                bulb_radius: DistanceSpan::ZERO,
                geometry: Geometry::Points {
                    points: vec![Point3::default()],
                },
            },
        );

        let sequence_effects = vec![EffectInst {
            id: EffectInstId(1),
            layer_id: SequenceLayerId(0),
            start: DawnTime(Duration::ZERO),
            duration: DawnDuration(Duration::from_secs_f64(1.0)),
            target: EffectTarget::Group(FixtureGroupId(1)),
            scope: EffectScope::WholeTarget,
            definition: EffectDefinitionId(SourceIdentity::new(
                "effects.dawn".into(),
                "Solid".to_string(),
            )),
            param_overrides: IndexMap::new(),
        }];
        DawnProject {
            root: ProjectRoot {
                id: ProjectId(source_identity("project")),
                setup: SetupId(source_identity("setup")),
                sequences: vec![SequenceId(source_identity("seq"))],
            },
            setups: IndexMap::from([(
                SetupId(source_identity("setup")),
                Setup {
                    id: SetupId(source_identity("setup")),
                    layout: LayoutId(source_identity("layout")),
                    patch: PatchId(source_identity("patch")),
                    controllers: Vec::new(),
                },
            )]),
            layouts: IndexMap::from([(
                LayoutId(source_identity("layout")),
                Layout {
                    id: LayoutId(source_identity("layout")),
                    target_order: Vec::new(),
                    fixtures: vec![FixtureInst {
                        id: FixtureInstanceId(1),
                        name: "Pixel".to_string(),
                        definition: FixtureDefinitionId(SourceIdentity::new(
                            "fixtures.dawn".into(),
                            "pixel".to_string(),
                        )),
                        position: Point3::default(),
                        rotation: Rotation3::default(),
                        scale: Scale3::default(),
                    }],
                    groups: vec![FixtureGroup {
                        id: FixtureGroupId(1),
                        name: "All".to_string(),
                        fixtures: vec![FixtureInstanceId(1)],
                    }],
                },
            )]),
            patches: IndexMap::new(),
            controllers: IndexMap::new(),
            sequences: IndexMap::from([(
                SequenceId(source_identity("seq")),
                Sequence {
                    id: SequenceId(source_identity("seq")),
                    duration: DawnDuration(Duration::from_secs_f64(1.0)),
                    frame_rate: 4,
                    audio: SequenceAudio::None,
                    mark_collections: Vec::new(),
                    layers: vec![SequenceLayer {
                        id: SequenceLayerId(0),
                        name: "Default".to_string(),
                        color: Color {
                            red: 80,
                            green: 160,
                            blue: 255,
                        },
                        enabled: true,
                    }],
                    effects: sequence_effects,
                    composition_graph: SequenceCompositionGraph {
                        nodes: vec![
                            CompositionGraphNode {
                                id: CompositionGraphNodeId(1),
                                position: GraphNodePosition { x: 0.0, y: 0.0 },
                                kind: CompositionGraphNodeKind::Layer {
                                    layer_id: SequenceLayerId(0),
                                },
                            },
                            CompositionGraphNode {
                                id: CompositionGraphNodeId(2),
                                position: GraphNodePosition { x: 200.0, y: 0.0 },
                                kind: CompositionGraphNodeKind::Output,
                            },
                        ],
                        edges: vec![EffectGraphEdge {
                            from: CompositionGraphNodeId(1),
                            from_port: GraphPortId("output".to_string()),
                            to: CompositionGraphNodeId(2),
                            to_port: GraphPortId("input".to_string()),
                        }],
                    },
                    automation_clips: Vec::new(),
                },
            )]),
            definitions: ProjectDefinitionStores {
                effects,
                fixtures,
                curves: dawn_language::effect::CurveDefinitionStore::default(),
                gradients: dawn_language::effect::GradientDefinitionStore::default(),
                controllers: ControllerDefinitionStore::default(),
                operators: dawn_language::operator::OperatorDefinitionStore::default(),
            },
        }
    }

    fn snapshot(state: AudioTransportState, position_seconds: f64) -> AudioTransportSnapshot {
        AudioTransportSnapshot {
            state,
            source: None,
            generation: 7,
            position_seconds,
            home_seconds: 0.0,
            duration_seconds: 1.0,
            last_error: None,
        }
    }

    fn _color(red: u8, green: u8, blue: u8) -> Color {
        Color { red, green, blue }
    }
}
