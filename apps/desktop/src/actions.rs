use std::path::PathBuf;

use dawn_project::path::ProjectPath;

#[derive(Debug, Clone)]
pub enum AppAction {
    OpenProject(PathBuf),
    NewProject,
    CloseProject,
    OpenSettings,
    Reload,
    Check,
    OpenFile(ProjectPath),
    CloseFile(ProjectPath),
    SetActiveFile(ProjectPath),
    UpdateActiveText(String),
    SaveActiveFile,
    SetEditorViewMode {
        path: ProjectPath,
        mode: crate::editor_session::EditorViewMode,
    },
    CycleTabs {
        reverse: bool,
    },
    RenamePath {
        path: ProjectPath,
        new_name: String,
    },
    CreateFile {
        parent: ProjectPath,
        name: String,
    },
    CreateDirectory {
        parent: ProjectPath,
        name: String,
    },
    DeletePath(ProjectPath),
    MovePaths {
        paths: Vec<ProjectPath>,
        new_parent: ProjectPath,
    },
    NudgeLayoutFixture {
        id: String,
        dx: f64,
        dy: f64,
    },
    DuplicateLayoutFixture {
        id: String,
    },
    DeleteLayoutFixture {
        id: String,
    },
    AdjustFixtureBulb {
        object_key: String,
        delta: f64,
    },
    DuplicateFixtureDefinition {
        object_key: String,
    },
    DeleteFixtureDefinition {
        object_key: String,
    },
    OpenSequence(ProjectPath),
    Play,
    Pause,
    Stop,
    About,
    Seek(f64),
    ToggleProjectTree,
    ToggleInspector,
    SetInspectorTab(crate::layout_persistence::InspectorTab),
    ResetLayout,
}
