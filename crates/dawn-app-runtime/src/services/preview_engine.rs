use crate::contracts::{Event, Revision, RuntimeResult, SequenceId};
use crate::runtime::ServiceCore;

#[derive(Debug, Clone)]
pub enum PreviewEngineCommand {
    QueueRender {
        sequence: SequenceId,
        request_revision: Revision,
    },
    PublishFrame {
        sequence: SequenceId,
        request_revision: Revision,
    },
}

#[derive(Debug, Default, Clone)]
pub struct PreviewEngineCore {
    latest_request: Option<(SequenceId, Revision)>,
    frame_revision: Revision,
}

impl PreviewEngineCore {
    pub fn handle(&mut self, command: PreviewEngineCommand) -> RuntimeResult<Vec<Event>> {
        match command {
            PreviewEngineCommand::QueueRender {
                sequence,
                request_revision,
            } => {
                self.latest_request = Some((sequence.clone(), request_revision));
                Ok(vec![Event::PreviewQueued {
                    sequence,
                    request_revision,
                }])
            }
            PreviewEngineCommand::PublishFrame {
                sequence,
                request_revision,
            } => {
                if self.latest_request.as_ref() != Some(&(sequence.clone(), request_revision)) {
                    return Ok(Vec::new());
                }
                self.frame_revision = self.frame_revision.next();
                Ok(vec![Event::PreviewFramePublished {
                    sequence,
                    request_revision,
                    frame_revision: self.frame_revision,
                }])
            }
        }
    }
}

impl ServiceCore for PreviewEngineCore {
    type Command = PreviewEngineCommand;

    fn service_name(&self) -> crate::contracts::ServiceName {
        crate::contracts::ServiceName::PreviewEngine
    }

    fn revision(&self) -> Revision {
        self.frame_revision
    }

    fn handle(&mut self, command: Self::Command) -> RuntimeResult<Vec<Event>> {
        PreviewEngineCore::handle(self, command)
    }
}
