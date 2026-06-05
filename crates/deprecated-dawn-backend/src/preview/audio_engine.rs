use crate::runtime::contracts::{Event, Revision, RuntimeResult, SequenceId};

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
