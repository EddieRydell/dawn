use std::fmt;
use std::time::SystemTime;

use dawn_language::path::Utf8PathBuf;
use serde::{Deserialize, Serialize};

use crate::editor::{BufferExternalState, EditorViewMode, FileVersion};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct Revision(u64);

impl Revision {
    pub const INITIAL: Self = Self(0);

    pub fn new(value: u64) -> Self {
        Self(value)
    }

    pub fn next(self) -> Self {
        Self(self.0.saturating_add(1))
    }

    pub fn get(self) -> u64 {
        self.0
    }
}

impl Default for Revision {
    fn default() -> Self {
        Self::INITIAL
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct RequestId(u64);

impl RequestId {
    pub fn new(value: u64) -> Self {
        Self(value)
    }

    pub fn get(self) -> u64 {
        self.0
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct BufferId {
    pub path: Utf8PathBuf,
}

impl From<Utf8PathBuf> for BufferId {
    fn from(path: Utf8PathBuf) -> Self {
        Self { path }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SequenceId {
    pub path: Utf8PathBuf,
    pub object_key: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ServiceName {
    DocumentStore,
    ProjectIndex,
    SequenceEdit,
    PreviewEngine,
    AudioEngine,
    Autosave,
    FileWatcher,
    LiveOutput,
    LayoutPrefs,
    ReadModel,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum RuntimeErrorKind {
    NoProject,
    NotFound,
    Conflict,
    StaleRevision,
    Backpressure,
    InvalidCommand,
    Io,
    Fatal,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum RuntimeStatus {
    NoProjectOpen,
    Saved,
    Message { message: String },
}

impl RuntimeStatus {
    pub fn message(message: impl Into<String>) -> Self {
        match message.into().as_str() {
            "No project open" => Self::NoProjectOpen,
            "Saved" => Self::Saved,
            message => Self::Message {
                message: message.to_string(),
            },
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RuntimeError {
    pub service: ServiceName,
    pub kind: RuntimeErrorKind,
    pub message: String,
}

impl RuntimeError {
    pub fn new(service: ServiceName, kind: RuntimeErrorKind, message: impl Into<String>) -> Self {
        Self {
            service,
            kind,
            message: message.into(),
        }
    }

    pub fn stale(service: ServiceName, expected: Revision, actual: Revision) -> Self {
        Self::new(
            service,
            RuntimeErrorKind::StaleRevision,
            format!(
                "stale revision: expected {}, current {}",
                expected.get(),
                actual.get()
            ),
        )
    }
}

impl fmt::Display for RuntimeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "{:?} {:?}: {}",
            self.service, self.kind, self.message
        )
    }
}

impl std::error::Error for RuntimeError {}

pub type RuntimeResult<T> = Result<T, RuntimeError>;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RuntimeCommandOutcome {
    pub changed_slices: Vec<RuntimeSlice>,
    pub effects: Vec<RuntimeEffect>,
}

impl RuntimeCommandOutcome {
    pub fn all_slices() -> Self {
        Self {
            changed_slices: RuntimeSlice::all(),
            effects: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RuntimeSlice {
    Workspace,
    Editor,
    ActiveDocument,
    Diagnostics,
    Preview,
    LiveOutput,
    Status,
    Prefs,
}

impl RuntimeSlice {
    pub fn all() -> Vec<Self> {
        vec![
            Self::Workspace,
            Self::Editor,
            Self::ActiveDocument,
            Self::Diagnostics,
            Self::Preview,
            Self::LiveOutput,
            Self::Status,
            Self::Prefs,
        ]
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum RuntimeEffect {
    SyncWatcherRoot { project_root: Option<String> },
    ClearNativeAudio,
    OpenPreviewWindow { focus: bool },
    ClosePreviewWindow,
    SyncPreviewTransport { pixel_count: usize },
    SendLiveOutputFrame,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CommandAck {
    pub request_id: RequestId,
    pub service: ServiceName,
    pub target_revision: Option<Revision>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum TaskStatus {
    Queued,
    Running,
    Complete,
    Failed,
    Cancelled,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TaskRecord {
    pub request_id: RequestId,
    pub service: ServiceName,
    pub label: String,
    pub status: TaskStatus,
    pub message: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Event {
    ProjectOpened {
        root: String,
        revision: Revision,
    },
    BufferOpened {
        path: Utf8PathBuf,
        revision: Revision,
        text: String,
        disk_version: Option<FileVersion>,
        external_state: BufferExternalState,
        view_mode: EditorViewMode,
    },
    ActiveBufferChanged {
        path: Utf8PathBuf,
        revision: Revision,
    },
    BufferClosed {
        path: Utf8PathBuf,
        active_file: Option<Utf8PathBuf>,
        revision: Revision,
    },
    BufferViewModeChanged {
        path: Utf8PathBuf,
        mode: EditorViewMode,
        revision: Revision,
    },
    BufferUpdated {
        path: Utf8PathBuf,
        revision: Revision,
        dirty: bool,
        disk_version: Option<FileVersion>,
        external_state: BufferExternalState,
    },
    BufferTextUpdated {
        path: Utf8PathBuf,
        revision: Revision,
        text: String,
    },
    BufferConflict {
        path: Utf8PathBuf,
        clean_revision: Revision,
        disk_version: Option<FileVersion>,
        external_state: BufferExternalState,
    },
    BufferPathReconciled {
        old_path: Utf8PathBuf,
        new_path: Utf8PathBuf,
        revision: Revision,
    },
    AnalysisUpdated {
        revision: Revision,
        diagnostic_count: usize,
    },
    PreviewQueued {
        sequence: SequenceId,
        request_revision: Revision,
    },
    PreviewFramePublished {
        sequence: SequenceId,
        request_revision: Revision,
        frame_revision: Revision,
    },
    AudioReadinessChanged {
        sequence: SequenceId,
        revision: Revision,
        ready: bool,
    },
    AutosaveTagged {
        path: Utf8PathBuf,
        tag: SelfWriteTag,
        revision: Revision,
    },
    CommandFailed {
        service: ServiceName,
        kind: RuntimeErrorKind,
        message: String,
    },
    CommandCompleted {
        service: ServiceName,
    },
    TaskChanged(TaskRecord),
    Fatal {
        service: ServiceName,
        message: String,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EventEnvelope {
    pub request_id: Option<RequestId>,
    pub service: ServiceName,
    pub sequence: u64,
    pub created_at: SystemTime,
    pub event: Event,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SelfWriteTag {
    pub path: Utf8PathBuf,
    pub revision: Revision,
    pub nonce: u64,
}
