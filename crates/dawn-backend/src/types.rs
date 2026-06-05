use camino::Utf8PathBuf;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum EditorViewMode {
    #[default]
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

#[derive(Debug, Clone)]
pub(crate) struct ProjectFileMetadata {
    pub(crate) len: u64,
}

#[derive(Debug, Clone)]
pub(crate) struct ProjectFileSnapshot {
    pub(crate) text: String,
    pub(crate) version: FileVersion,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ProjectPathMove {
    pub(crate) old_path: Utf8PathBuf,
    pub(crate) new_path: Utf8PathBuf,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ProjectSessionPreferences {
    pub(crate) tabs: Vec<ProjectSessionTabPreference>,
    pub(crate) active_file: Option<Utf8PathBuf>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ProjectSessionTabPreference {
    pub(crate) path: Utf8PathBuf,
    pub(crate) view_mode: EditorViewMode,
}
