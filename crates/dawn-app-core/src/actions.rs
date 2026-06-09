use std::path::PathBuf;

use crate::dto::{EditorViewModeDto, FixtureGuiEditDto, LayoutGuiEditDto, SequenceGuiEditDto};
use dawn_project::Utf8PathBuf;

#[derive(Debug, Clone)]
pub enum AppAction {
    OpenProject(PathBuf),
    ReloadProject,
    OpenFile(Utf8PathBuf),
    CloseFile(Utf8PathBuf),
    SetActiveFile(Utf8PathBuf),
    SetActiveViewMode(EditorViewModeDto),
    UpdateActiveText(String),
    UndoActiveEdit,
    RedoActiveEdit,
    ApplySequenceGuiEdit(SequenceGuiEditDto),
    ApplyLayoutGuiEdit(LayoutGuiEditDto),
    ApplyFixtureGuiEdit(FixtureGuiEditDto),
    FlushAutosave,
    FilesystemChanged(Vec<Utf8PathBuf>),
    ReloadActiveBufferFromDisk,
    KeepActiveBuffer,
    CreateFile { parent: Utf8PathBuf, name: String },
    CreateDirectory { parent: Utf8PathBuf, name: String },
    RenamePath { path: Utf8PathBuf, new_name: String },
    DeletePath(Utf8PathBuf),
    ToggleProjectTree,
    SetEffectPreviewEnabled(bool),
    SetEffectPreviewEffects(Vec<u32>),
    PreviewPlay,
    PreviewPause,
    PreviewStop,
    PreviewRewindToZero,
    PreviewSeek(f64),
}
