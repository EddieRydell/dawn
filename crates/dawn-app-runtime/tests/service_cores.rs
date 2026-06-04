use std::time::SystemTime;

use dawn_app_runtime::read_model::ReadModelCore;
use dawn_app_runtime::runtime::contracts::{
    BufferExternalState, DiskVersion, Event, EventEnvelope, Revision, RuntimeErrorKind, SequenceId,
    ServiceName,
};
use dawn_app_runtime::services::autosave::{AutosaveCommand, AutosaveCore};
use dawn_app_runtime::services::document_store::{DocumentStoreCommand, DocumentStoreCore};
use dawn_app_runtime::services::file_watcher::{FileWatcherCommand, FileWatcherCore};
use dawn_app_runtime::services::preview_engine::{PreviewEngineCommand, PreviewEngineCore};
use dawn_language::path::Utf8PathBuf;

#[test]
fn document_store_rejects_stale_text_edits() {
    let path = Utf8PathBuf::from("sequences/example.sequence.dawn");
    let mut core = DocumentStoreCore::default();
    core.handle(DocumentStoreCommand::OpenProject {
        root: "C:/project".to_string(),
    })
    .expect("project opens");
    core.handle(DocumentStoreCommand::OpenBuffer {
        path: path.clone(),
        text: "first".to_string(),
        disk_version: Some(disk_version(5, 1)),
    })
    .expect("buffer opens");
    core.handle(DocumentStoreCommand::UpdateBufferText {
        path: path.clone(),
        expected_revision: Revision::new(2),
        text: "second".to_string(),
    })
    .expect("first edit succeeds");

    let error = core
        .handle(DocumentStoreCommand::UpdateBufferText {
            path,
            expected_revision: Revision::new(2),
            text: "third".to_string(),
        })
        .expect_err("stale edit is rejected");

    assert_eq!(error.kind, RuntimeErrorKind::StaleRevision);
}

#[test]
fn document_store_marks_dirty_external_change_as_conflict() {
    let path = Utf8PathBuf::from("sequences/example.sequence.dawn");
    let mut core = DocumentStoreCore::default();
    core.handle(DocumentStoreCommand::OpenBuffer {
        path: path.clone(),
        text: "saved".to_string(),
        disk_version: Some(disk_version(5, 1)),
    })
    .expect("buffer opens");
    core.handle(DocumentStoreCommand::UpdateBufferText {
        path: path.clone(),
        expected_revision: Revision::new(1),
        text: "dirty".to_string(),
    })
    .expect("edit succeeds");

    let events = core
        .handle(DocumentStoreCommand::ExternalDiskChanged {
            path: path.clone(),
            disk_version: disk_version(4, 2),
            text: "disk".to_string(),
        })
        .expect("external event handled");

    assert!(matches!(events.as_slice(), [Event::BufferConflict { .. }]));
    let buffer = core.buffer(&path).expect("buffer remains open");
    assert_eq!(buffer.external_state, BufferExternalState::ChangedOnDisk);
    assert_eq!(buffer.disk_version, Some(disk_version(4, 2)));
}

#[test]
fn document_store_tracks_reload_keep_delete_and_path_reconciliation() {
    let path = Utf8PathBuf::from("sequences/example.sequence.dawn");
    let moved_path = Utf8PathBuf::from("sequences/moved.sequence.dawn");
    let mut core = DocumentStoreCore::default();
    core.handle(DocumentStoreCommand::OpenBuffer {
        path: path.clone(),
        text: "saved".to_string(),
        disk_version: Some(disk_version(5, 1)),
    })
    .expect("buffer opens");
    core.handle(DocumentStoreCommand::UpdateBufferText {
        path: path.clone(),
        expected_revision: Revision::new(1),
        text: "dirty".to_string(),
    })
    .expect("edit succeeds");
    core.handle(DocumentStoreCommand::ExternalDiskDeleted { path: path.clone() })
        .expect("delete is tracked");
    assert_eq!(
        core.buffer(&path)
            .expect("buffer remains open")
            .external_state,
        BufferExternalState::DeletedOnDisk
    );

    core.handle(DocumentStoreCommand::KeepBuffer { path: path.clone() })
        .expect("keep clears external state");
    assert_eq!(
        core.buffer(&path)
            .expect("buffer remains open")
            .external_state,
        BufferExternalState::Current
    );

    core.handle(DocumentStoreCommand::ReloadBufferFromDisk {
        path: path.clone(),
        text: "disk".to_string(),
        disk_version: disk_version(4, 3),
    })
    .expect("reload replaces buffer text");
    let buffer = core.buffer(&path).expect("buffer remains open");
    assert_eq!(buffer.text, "disk");
    assert!(!buffer.dirty());
    assert_eq!(buffer.disk_version, Some(disk_version(4, 3)));

    core.handle(DocumentStoreCommand::ReconcileMovedPath {
        old_path: path.clone(),
        new_path: moved_path.clone(),
    })
    .expect("move reconciles path");
    assert!(core.buffer(&path).is_none());
    assert!(core.buffer(&moved_path).is_some());
    assert_eq!(core.active_file(), Some(&moved_path));

    core.handle(DocumentStoreCommand::ReconcileDeletedPath { path: moved_path })
        .expect("delete reconciliation closes buffer");
    assert!(core.buffers().next().is_none());
}

#[test]
fn autosave_tags_and_file_watcher_ignores_self_writes() {
    let path = Utf8PathBuf::from("sequences/example.sequence.dawn");
    let mut autosave = AutosaveCore::default();
    let events = autosave
        .handle(AutosaveCommand::TagSelfWrite {
            path: path.clone(),
            revision: Revision::new(4),
        })
        .expect("tag emitted");
    let Event::AutosaveTagged { tag, .. } = events[0].clone() else {
        panic!("expected autosave tag");
    };
    assert!(autosave.contains_tag(&tag));

    let mut watcher = FileWatcherCore::default();
    let watcher_events = watcher
        .handle(FileWatcherCommand::DiskChanged {
            path,
            disk_version: disk_version(4, 4),
            matching_self_write: Some(tag),
        })
        .expect("watcher handles event");

    assert!(watcher_events.is_empty());
}

#[test]
fn preview_engine_publishes_only_latest_request() {
    let sequence = SequenceId {
        path: Utf8PathBuf::from("sequences/example.sequence.dawn"),
        object_key: "main".to_string(),
    };
    let mut core = PreviewEngineCore::default();
    core.handle(PreviewEngineCommand::QueueRender {
        sequence: sequence.clone(),
        request_revision: Revision::new(1),
    })
    .expect("first queued");
    core.handle(PreviewEngineCommand::QueueRender {
        sequence: sequence.clone(),
        request_revision: Revision::new(2),
    })
    .expect("second queued");

    let stale_events = core
        .handle(PreviewEngineCommand::PublishFrame {
            sequence: sequence.clone(),
            request_revision: Revision::new(1),
        })
        .expect("stale frame ignored");
    let latest_events = core
        .handle(PreviewEngineCommand::PublishFrame {
            sequence,
            request_revision: Revision::new(2),
        })
        .expect("latest frame published");

    assert!(stale_events.is_empty());
    assert!(matches!(
        latest_events.as_slice(),
        [Event::PreviewFramePublished { .. }]
    ));
}

#[test]
fn read_model_applies_editor_and_preview_events() {
    let path = Utf8PathBuf::from("sequences/example.sequence.dawn");
    let sequence = SequenceId {
        path: path.clone(),
        object_key: "main".to_string(),
    };
    let mut core = ReadModelCore::default();
    apply(
        &mut core,
        Event::BufferOpened {
            path: path.clone(),
            revision: Revision::new(1),
            text: "saved".to_string(),
            disk_version: Some(disk_version(5, 1)),
            external_state: dawn_app_runtime::runtime::contracts::BufferExternalState::Current,
            view_mode: dawn_app_runtime::runtime::contracts::ViewMode::Text,
        },
    );
    apply(
        &mut core,
        Event::BufferUpdated {
            path,
            revision: Revision::new(2),
            dirty: true,
            disk_version: Some(disk_version(5, 1)),
            external_state: dawn_app_runtime::runtime::contracts::BufferExternalState::Current,
        },
    );
    apply(
        &mut core,
        Event::PreviewQueued {
            sequence: sequence.clone(),
            request_revision: Revision::new(2),
        },
    );
    apply(
        &mut core,
        Event::PreviewFramePublished {
            sequence,
            request_revision: Revision::new(2),
            frame_revision: Revision::new(1),
        },
    );

    assert_eq!(core.models().editor.buffers.len(), 1);
    assert!(!core.models().preview.stale);
    assert!(!core.models().preview.updating);
}

fn disk_version(len: u64, content_hash: u64) -> DiskVersion {
    DiskVersion {
        len,
        modified_millis: None,
        content_hash,
    }
}

fn apply(core: &mut ReadModelCore, event: Event) {
    core.apply(&EventEnvelope {
        request_id: None,
        service: ServiceName::ReadModel,
        sequence: 0,
        created_at: SystemTime::now(),
        event,
    })
    .expect("event applies");
}
