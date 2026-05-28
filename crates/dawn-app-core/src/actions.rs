use std::path::PathBuf;

use dawn_project::path::Utf8PathBuf;

#[derive(Debug, Clone)]
pub enum AppAction {
    OpenProject(PathBuf),
    ReloadProject,
    OpenFile(Utf8PathBuf),
    CloseFile(Utf8PathBuf),
    SetActiveFile(Utf8PathBuf),
    UpdateActiveText(String),
    FlushAutosave,
    CreateFile { parent: Utf8PathBuf, name: String },
    CreateDirectory { parent: Utf8PathBuf, name: String },
    RenamePath { path: Utf8PathBuf, new_name: String },
    DeletePath(Utf8PathBuf),
    ToggleProjectTree,
}
