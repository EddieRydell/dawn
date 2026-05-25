use std::path::PathBuf;

use dawn_project::model::FixtureId;
use dawn_project::path::Utf8PathBuf;

use crate::app_model::PreviewRigKind;

#[derive(Debug, Clone)]
pub enum AppAction {
    OpenProject(PathBuf),
    NewProject,
    CloseProject,
    Quit,
    OpenSettings,
    Reload,
    Check,
    OpenFile(Utf8PathBuf),
    CloseFile(Utf8PathBuf),
    SetActiveFile(Utf8PathBuf),
    UpdateActiveText(String),
    SaveActiveFile,
    SetEditorViewMode {
        path: Utf8PathBuf,
        mode: crate::editor_session::EditorViewMode,
    },
    CycleTabs {
        reverse: bool,
    },
    RenamePath {
        path: Utf8PathBuf,
        new_name: String,
    },
    CreateFile {
        parent: Utf8PathBuf,
        name: String,
    },
    CreateDirectory {
        parent: Utf8PathBuf,
        name: String,
    },
    DeletePath(Utf8PathBuf),
    MovePaths {
        paths: Vec<Utf8PathBuf>,
        new_parent: Utf8PathBuf,
    },
    NudgeLayoutFixtures {
        fixture_ids: Vec<FixtureId>,
        dx: f64,
        dy: f64,
    },
    DuplicateLayoutFixture {
        fixture_id: FixtureId,
    },
    DeleteLayoutFixture {
        fixture_id: FixtureId,
    },
    CreateInlineLayoutFixture {
        x: f64,
        y: f64,
    },
    ConfirmLayoutFixtureName {
        name: String,
    },
    StartImportLayoutFixture {
        selected_file: PathBuf,
        x: f64,
        y: f64,
    },
    ConfirmImportLayoutFixture {
        object_key: String,
    },
    CancelImportLayoutFixture,
    CancelLayoutFixtureName,
    AdjustFixtureBulb {
        object_key: String,
        delta: f64,
    },
    SelectFixtureDefinition {
        object_key: String,
    },
    NudgeFixtureGeometryHandles {
        object_key: String,
        handles: Vec<usize>,
        dx: f64,
        dy: f64,
    },
    DuplicateFixtureDefinition {
        object_key: String,
    },
    DeleteFixtureDefinition {
        object_key: String,
    },
    OpenSequence(Utf8PathBuf),
    SelectPreviewFixture(Option<FixtureId>),
    SelectPreviewRig(PreviewRigKind),
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
