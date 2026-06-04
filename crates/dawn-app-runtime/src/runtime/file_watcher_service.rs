use crate::editor::{BufferExternalState, FileVersion};
use crate::runtime::contracts::{Event, Revision, RuntimeResult, SelfWriteTag};
use dawn_language::path::Utf8PathBuf;

#[derive(Debug, Clone)]
pub enum FileWatcherCommand {
    DiskChanged {
        path: Utf8PathBuf,
        disk_version: FileVersion,
        matching_self_write: Option<SelfWriteTag>,
    },
}

#[derive(Debug, Default, Clone)]
pub struct FileWatcherCore {
    revision: Revision,
}

impl FileWatcherCore {
    pub fn handle(&mut self, command: FileWatcherCommand) -> RuntimeResult<Vec<Event>> {
        match command {
            FileWatcherCommand::DiskChanged {
                path,
                disk_version,
                matching_self_write,
            } => {
                if matching_self_write.is_some() {
                    return Ok(Vec::new());
                }
                self.revision = self.revision.next();
                Ok(vec![Event::BufferConflict {
                    path,
                    clean_revision: self.revision,
                    disk_version: Some(disk_version),
                    external_state: BufferExternalState::ChangedOnDisk,
                }])
            }
        }
    }
}
