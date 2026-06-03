use std::time::SystemTime;

use dawn_app_runtime::contracts::{
    Event, EventEnvelope, Revision, RuntimeErrorKind, SequenceId, ServiceName,
};
use dawn_app_runtime::read_model::ReadModelCore;
use dawn_app_runtime::services::autosave::{AutosaveCommand, AutosaveCore};
use dawn_app_runtime::services::document_store::{DocumentStoreCommand, DocumentStoreCore};
use dawn_app_runtime::services::file_watcher::{FileWatcherCommand, FileWatcherCore};
use dawn_app_runtime::services::preview_engine::{PreviewEngineCommand, PreviewEngineCore};
use dawn_project::path::Utf8PathBuf;

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
        disk_revision: Revision::INITIAL,
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
        disk_revision: Revision::INITIAL,
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
            path,
            disk_revision: Revision::new(10),
            text: "disk".to_string(),
        })
        .expect("external event handled");

    assert!(matches!(events.as_slice(), [Event::BufferConflict { .. }]));
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
            disk_revision: Revision::new(4),
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
        },
    );
    apply(
        &mut core,
        Event::BufferUpdated {
            path,
            revision: Revision::new(2),
            dirty: true,
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
