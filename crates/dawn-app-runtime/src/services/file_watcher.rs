use crate::contracts::{Event, Revision, RuntimeResult, SelfWriteTag};
use crate::runtime::ServiceCore;
use dawn_project::path::Utf8PathBuf;

#[derive(Debug, Clone)]
pub enum FileWatcherCommand {
    DiskChanged {
        path: Utf8PathBuf,
        disk_revision: Revision,
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
                disk_revision,
                matching_self_write,
            } => {
                if matching_self_write.is_some() {
                    return Ok(Vec::new());
                }
                self.revision = disk_revision;
                Ok(vec![Event::BufferConflict {
                    path,
                    clean_revision: disk_revision,
                    disk_revision,
                }])
            }
        }
    }
}

impl ServiceCore for FileWatcherCore {
    type Command = FileWatcherCommand;

    fn service_name(&self) -> crate::contracts::ServiceName {
        crate::contracts::ServiceName::FileWatcher
    }

    fn revision(&self) -> Revision {
        self.revision
    }

    fn handle(&mut self, command: Self::Command) -> RuntimeResult<Vec<Event>> {
        FileWatcherCore::handle(self, command)
    }
}
