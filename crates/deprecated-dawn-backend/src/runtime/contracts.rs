use dawn_language::path::Utf8PathBuf;
use serde::{Deserialize, Serialize};
use std::fmt;

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
pub struct RuntimeStatus {
    pub notice: RuntimeNotice,
    pub activities: Vec<RuntimeActivity>,
}

impl RuntimeStatus {
    pub fn no_project_open() -> Self {
        Self::notice(RuntimeNotice::NoProjectOpen)
    }

    pub fn notice(notice: RuntimeNotice) -> Self {
        Self {
            notice,
            activities: Vec::new(),
        }
    }

    pub fn error(message: impl Into<String>) -> Self {
        Self::notice(RuntimeNotice::Error {
            message: message.into(),
        })
    }

    pub fn set_notice(&mut self, notice: RuntimeNotice) {
        self.notice = notice;
    }

    pub fn set_activity(&mut self, activity: RuntimeActivity) {
        if !self.activities.contains(&activity) {
            self.activities.push(activity);
        }
    }

    pub fn clear_activity(&mut self, activity: RuntimeActivity) {
        self.activities.retain(|current| *current != activity);
    }

    pub fn is_no_project_open(&self) -> bool {
        self.notice == RuntimeNotice::NoProjectOpen && self.activities.is_empty()
    }

    pub fn is_saved(&self) -> bool {
        self.notice == RuntimeNotice::Saved && self.activities.is_empty()
    }

    pub fn label(&self) -> String {
        let mut parts = self
            .activities
            .iter()
            .map(RuntimeActivity::label)
            .collect::<Vec<_>>();
        parts.push(self.notice.label());
        parts.join(" - ")
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum RuntimeNotice {
    NoProjectOpen,
    Saved,
    Autosaved,
    Edited,
    Undo,
    Redo,
    ProjectOpened,
    ProjectRestored,
    ProjectChecked,
    FilesystemRefreshed,
    ReloadedFromDisk,
    KeptIdeChanges,
    PreviewPlaying,
    PreviewPaused,
    PreviewStopped,
    PreviewRewound,
    PreviewSeeked,
    PreviewComplete,
    PreviewReady,
    AudioMissing,
    ExportedFseq {
        frame_count: u32,
        channel_count: u32,
    },
    Selection {
        message: String,
    },
    Error {
        message: String,
    },
}

impl RuntimeNotice {
    pub fn label(&self) -> String {
        match self {
            Self::NoProjectOpen => "No project open".to_string(),
            Self::Saved => "Saved".to_string(),
            Self::Autosaved => "Autosaved".to_string(),
            Self::Edited => "Edited".to_string(),
            Self::Undo => "Undo".to_string(),
            Self::Redo => "Redo".to_string(),
            Self::ProjectOpened => "Project opened".to_string(),
            Self::ProjectRestored => "Project restored".to_string(),
            Self::ProjectChecked => "Project checked".to_string(),
            Self::FilesystemRefreshed => "Filesystem refreshed".to_string(),
            Self::ReloadedFromDisk => "Reloaded from disk".to_string(),
            Self::KeptIdeChanges => "Kept IDE changes".to_string(),
            Self::PreviewPlaying => "Preview playing".to_string(),
            Self::PreviewPaused => "Preview paused".to_string(),
            Self::PreviewStopped => "Preview stopped".to_string(),
            Self::PreviewRewound => "Preview rewound".to_string(),
            Self::PreviewSeeked => "Preview seeked".to_string(),
            Self::PreviewComplete => "Preview complete".to_string(),
            Self::PreviewReady => "Preview ready".to_string(),
            Self::AudioMissing => "Audio missing".to_string(),
            Self::ExportedFseq {
                frame_count,
                channel_count,
            } => format!("Exported FSEQ: {frame_count} frames, {channel_count} channels"),
            Self::Selection { message } | Self::Error { message } => message.clone(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RuntimeActivity {
    LoadingAudio,
    LoadingAudioToPlay,
}

impl RuntimeActivity {
    pub fn label(&self) -> String {
        match self {
            Self::LoadingAudio => "Loading audio".to_string(),
            Self::LoadingAudioToPlay => "Loading audio - will play".to_string(),
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
    Fatal {
        service: ServiceName,
        message: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SelfWriteTag {
    pub path: Utf8PathBuf,
    pub revision: Revision,
    pub nonce: u64,
}
