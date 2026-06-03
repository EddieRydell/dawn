use std::thread;
use std::time::{Duration, Instant};

use dawn_app_runtime::contracts::{Revision, RuntimeErrorKind, ServiceName};
use dawn_app_runtime::coordinator::AppCoordinator;
use dawn_app_runtime::services::document_store::DocumentStoreCommand;
use dawn_project::path::Utf8PathBuf;

#[test]
fn coordinator_assigns_monotonic_request_ids() {
    let mut coordinator = AppCoordinator::new();

    let first = coordinator
        .submit_document_store(DocumentStoreCommand::OpenProject {
            root: "C:/project".to_string(),
        })
        .expect("first command accepted");
    let second = coordinator
        .submit_document_store(DocumentStoreCommand::OpenProject {
            root: "C:/project-2".to_string(),
        })
        .expect("second command accepted");

    assert_eq!(first.request_id.get(), 1);
    assert_eq!(second.request_id.get(), 2);
    assert_eq!(first.target_revision, None);
    coordinator.shutdown().expect("shutdown joins workers");
}

#[test]
fn coordinator_drains_project_opened_into_read_model() {
    let mut coordinator = AppCoordinator::new();
    coordinator
        .submit_document_store(DocumentStoreCommand::OpenProject {
            root: "C:/project".to_string(),
        })
        .expect("open project accepted");

    drain_until(&mut coordinator, |coordinator| {
        coordinator.read_models().workspace.project_root.as_deref() == Some("C:/project")
    });

    assert_eq!(
        coordinator.read_models().workspace.revision,
        Revision::new(1)
    );
    coordinator.shutdown().expect("shutdown joins workers");
}

#[test]
fn coordinator_drains_buffer_revisions_into_editor_read_model() {
    let path = Utf8PathBuf::from("sequences/example.sequence.dawn");
    let mut coordinator = AppCoordinator::new();
    coordinator
        .submit_document_store(DocumentStoreCommand::OpenProject {
            root: "C:/project".to_string(),
        })
        .expect("open project accepted");
    coordinator
        .submit_document_store(DocumentStoreCommand::OpenBuffer {
            path: path.clone(),
            text: "first".to_string(),
            disk_revision: Revision::INITIAL,
        })
        .expect("open buffer accepted");
    let edit_ack = coordinator
        .submit_document_store(DocumentStoreCommand::UpdateBufferText {
            path: path.clone(),
            expected_revision: Revision::new(2),
            text: "second".to_string(),
        })
        .expect("edit accepted");

    assert_eq!(edit_ack.target_revision, Some(Revision::new(2)));
    drain_until(&mut coordinator, |coordinator| {
        coordinator
            .read_models()
            .editor
            .buffers
            .get(&path)
            .is_some_and(|buffer| buffer.revision == Revision::new(3) && buffer.dirty)
    });

    let buffer = coordinator
        .read_models()
        .editor
        .buffers
        .get(&path)
        .expect("buffer is published");
    assert_eq!(buffer.revision, Revision::new(3));
    assert!(buffer.dirty);
    coordinator.shutdown().expect("shutdown joins workers");
}

#[test]
fn coordinator_seeds_project_buffer_then_publishes_text_edit() {
    let path = Utf8PathBuf::from("sequences/example.sequence.dawn");
    let mut coordinator = AppCoordinator::new();
    coordinator
        .submit_document_store(DocumentStoreCommand::OpenProject {
            root: "C:/project".to_string(),
        })
        .expect("open project accepted");
    drain_until(&mut coordinator, |coordinator| {
        coordinator.read_models().workspace.project_root.as_deref() == Some("C:/project")
    });

    coordinator
        .submit_document_store(DocumentStoreCommand::OpenBuffer {
            path: path.clone(),
            text: "seed".to_string(),
            disk_revision: Revision::INITIAL,
        })
        .expect("open buffer accepted");
    drain_until(&mut coordinator, |coordinator| {
        coordinator
            .read_models()
            .editor
            .buffers
            .get(&path)
            .is_some_and(|buffer| buffer.revision == Revision::new(2) && !buffer.dirty)
    });

    let revision = coordinator
        .read_models()
        .editor
        .buffers
        .get(&path)
        .expect("buffer is seeded")
        .revision;
    coordinator
        .submit_document_store(DocumentStoreCommand::UpdateBufferText {
            path: path.clone(),
            expected_revision: revision,
            text: "edited".to_string(),
        })
        .expect("edit accepted");
    drain_until(&mut coordinator, |coordinator| {
        coordinator
            .read_models()
            .editor
            .buffers
            .get(&path)
            .is_some_and(|buffer| buffer.revision == revision.next() && buffer.dirty)
    });

    assert_eq!(
        coordinator.read_models().editor.active_file.as_ref(),
        Some(&path)
    );
    coordinator.shutdown().expect("shutdown joins workers");
}

#[test]
fn stale_buffer_edits_are_rejected_by_document_store_core() {
    let path = Utf8PathBuf::from("sequences/example.sequence.dawn");
    let mut coordinator = AppCoordinator::new();
    coordinator
        .submit_document_store(DocumentStoreCommand::OpenBuffer {
            path: path.clone(),
            text: "first".to_string(),
            disk_revision: Revision::INITIAL,
        })
        .expect("open buffer accepted");
    drain_until(&mut coordinator, |coordinator| {
        coordinator.read_models().editor.buffers.contains_key(&path)
    });

    let ack = coordinator
        .submit_document_store(DocumentStoreCommand::UpdateBufferText {
            path,
            expected_revision: Revision::INITIAL,
            text: "second".to_string(),
        })
        .expect("stale edit reaches service runner");

    assert_eq!(ack.target_revision, Some(Revision::INITIAL));
    drain_until(&mut coordinator, |coordinator| {
        coordinator.read_models().status.fatal_error.is_some()
    });

    assert_eq!(
        coordinator.read_models().status.fatal_error.as_deref(),
        Some("DocumentStore: stale revision: expected 0, current 1")
    );
    coordinator.shutdown().expect("shutdown joins workers");
}

#[test]
fn service_errors_are_reflected_as_fatal_status() {
    let mut coordinator = AppCoordinator::new();
    coordinator
        .submit_document_store(DocumentStoreCommand::SetActiveBuffer {
            path: Utf8PathBuf::from("missing.sequence.dawn"),
        })
        .expect("invalid command still reaches service runner");

    drain_until(&mut coordinator, |coordinator| {
        coordinator.read_models().status.fatal_error.is_some()
    });

    let fatal = coordinator
        .read_models()
        .status
        .fatal_error
        .as_deref()
        .expect("fatal status is published");
    assert!(fatal.starts_with("DocumentStore: buffer not open:"));
    coordinator.shutdown().expect("shutdown joins workers");
}

#[test]
fn coordinator_shutdown_joins_service_workers() {
    let mut coordinator = AppCoordinator::new();
    coordinator.shutdown().expect("shutdown joins workers");

    let error = coordinator
        .submit_document_store(DocumentStoreCommand::OpenProject {
            root: "C:/project".to_string(),
        })
        .expect_err("stopped coordinator rejects commands");

    assert_eq!(error.kind, RuntimeErrorKind::Fatal);
    assert_eq!(error.service, ServiceName::DocumentStore);
}

fn drain_until(coordinator: &mut AppCoordinator, done: impl Fn(&AppCoordinator) -> bool) {
    let deadline = Instant::now() + Duration::from_secs(2);
    while Instant::now() < deadline {
        coordinator.drain_events().expect("events drain");
        if done(coordinator) {
            return;
        }
        thread::sleep(Duration::from_millis(10));
    }
    coordinator.drain_events().expect("events drain");
    assert!(
        done(coordinator),
        "condition was not reached before timeout"
    );
}
