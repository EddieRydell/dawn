use std::path::PathBuf;

use dawn_project::document::LayoutTargetDocument;
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
    BeginDeferredPersistenceHold,
    EndDeferredPersistenceHold,
    FlushDeferredPersistence {
        revision: u64,
    },
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
    SelectSequenceEffect {
        id: Option<u32>,
    },
    AddSequenceEffect {
        script_path: String,
        target: LayoutTargetDocument,
        start_ms: u64,
    },
    DuplicateSequenceEffect {
        id: u32,
    },
    DeleteSequenceEffect {
        id: u32,
    },
    MoveSequenceEffect {
        id: u32,
        start_ms: u64,
        target: Option<LayoutTargetDocument>,
    },
    ResizeSequenceEffect {
        id: u32,
        start_ms: u64,
        duration_ms: u64,
    },
    RetargetSequenceEffect {
        id: u32,
        target: LayoutTargetDocument,
    },
    SetSequencePlayhead {
        time_ms: u64,
    },
    OpenSequence(Utf8PathBuf),
    SelectPreviewFixture(Option<FixtureId>),
    SelectPreviewRig(PreviewRigKind),
    OpenPreviewWindow,
    PreviewWindowClosed,
    SetPreviewWindowBounds {
        x: f64,
        y: f64,
        width: f64,
        height: f64,
    },
    ToggleSoloSelected,
    Play,
    Pause,
    Stop,
    TickPlayback {
        delta_ms: u64,
    },
    About,
    Seek(f64),
    ToggleProjectTree,
    ToggleInspector,
    SetInspectorTab(crate::layout_persistence::InspectorTab),
    ResetLayout,
}
