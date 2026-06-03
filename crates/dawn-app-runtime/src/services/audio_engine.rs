use crate::contracts::{Event, Revision, RuntimeResult, SequenceId};
use crate::runtime::ServiceCore;

#[derive(Debug, Clone)]
pub enum AudioEngineCommand {
    SetReadiness {
        sequence: SequenceId,
        revision: Revision,
        ready: bool,
    },
}

#[derive(Debug, Default, Clone)]
pub struct AudioEngineCore {
    revision: Revision,
}

impl AudioEngineCore {
    pub fn handle(&mut self, command: AudioEngineCommand) -> RuntimeResult<Vec<Event>> {
        match command {
            AudioEngineCommand::SetReadiness {
                sequence,
                revision,
                ready,
            } => {
                self.revision = revision;
                Ok(vec![Event::AudioReadinessChanged {
                    sequence,
                    revision,
                    ready,
                }])
            }
        }
    }
}

impl ServiceCore for AudioEngineCore {
    type Command = AudioEngineCommand;

    fn service_name(&self) -> crate::contracts::ServiceName {
        crate::contracts::ServiceName::AudioEngine
    }

    fn revision(&self) -> Revision {
        self.revision
    }

    fn handle(&mut self, command: Self::Command) -> RuntimeResult<Vec<Event>> {
        AudioEngineCore::handle(self, command)
    }
}
