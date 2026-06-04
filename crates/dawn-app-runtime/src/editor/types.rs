use dawn_language::path::Utf8PathBuf;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum EditorViewMode {
    Text,
    Gui,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileVersion {
    pub len: u64,
    pub modified_millis: Option<u128>,
    pub content_hash: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum BufferExternalState {
    Current,
    ChangedOnDisk,
    DeletedOnDisk,
}

#[derive(Debug, Clone)]
pub struct BufferTab {
    pub path: Utf8PathBuf,
    pub text: String,
    pub saved_text: String,
    pub disk_version: Option<FileVersion>,
    pub external_state: BufferExternalState,
    pub view_mode: EditorViewMode,
    pub revision: crate::runtime::contracts::Revision,
}

impl BufferTab {
    pub fn is_dirty(&self) -> bool {
        self.text != self.saved_text
    }

    pub fn is_conflicted(&self) -> bool {
        self.external_state != BufferExternalState::Current
    }
}
