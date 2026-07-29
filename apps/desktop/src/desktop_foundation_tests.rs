#[cfg(test)]
mod tests {
    use std::fs;

    use camino::{Utf8Path, Utf8PathBuf};
    use dawn_project_io::load_package;

    use crate::dto::{DocumentViewId, GuiDocument, GuiDocumentRequest, WorkspacePathChangeRequest};

    fn starter() -> dawn_project_io::ProjectSession {
        let workspace = Utf8Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .and_then(Utf8Path::parent)
            .unwrap();
        load_package(&workspace.join("examples/starter"))
            .unwrap()
            .session
    }

    fn starter_copy() -> (tempfile::TempDir, Utf8PathBuf) {
        let workspace = Utf8Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .and_then(Utf8Path::parent)
            .unwrap();
        let temporary = tempfile::tempdir().unwrap();
        let root = Utf8Path::from_path(temporary.path())
            .unwrap()
            .join("starter");
        copy_tree(&workspace.join("examples/starter"), &root);
        (temporary, root)
    }

    fn copy_tree(source: &Utf8Path, destination: &Utf8Path) {
        fs::create_dir_all(destination).unwrap();
        for entry in fs::read_dir(source).unwrap() {
            let entry = entry.unwrap();
            let source_path = Utf8PathBuf::from_path_buf(entry.path()).unwrap();
            let destination_path = destination.join(entry.file_name().into_string().unwrap());
            if entry.file_type().unwrap().is_dir() {
                copy_tree(&source_path, &destination_path);
            } else {
                fs::copy(source_path, destination_path).unwrap();
            }
        }
    }

    #[test]
    fn setup_projection_contains_all_five_sections() {
        let session = starter();
        let request = GuiDocumentRequest {
            path: "setups/main.setup.dawn".to_string(),
            view: DocumentViewId::Setup,
            object_key: Some("main".to_string()),
        };
        let GuiDocument::Setup { document } =
            crate::gui::project_gui_document(Some(&session), &request)
        else {
            panic!("setup projection was blocked");
        };
        assert!(!document.elements.is_empty());
        assert!(!document.preview_links.is_empty());
        assert!(!document.patch_nodes.is_empty());
        assert!(!document.controllers.is_empty());
        assert_eq!(document.fixture_profiles.len(), 0);
    }

    #[test]
    fn show_render_service_returns_shared_show_frame() {
        let session = starter();
        let sequence = session.project.root.sequences.first().unwrap();
        let mut service = crate::show_render::ShowRenderService::new();
        service
            .prepare(&session.project, &session.project.root.setup, sequence)
            .unwrap();
        let audio = crate::dto::AudioTransportSnapshot {
            state: crate::dto::AudioTransportState::Paused,
            source: None,
            generation: 4,
            position_seconds: 0.0,
            home_seconds: 0.0,
            duration_seconds: 60.0,
            last_error: None,
        };
        let frame = service.render_current_sequence_frame(&audio).unwrap();
        assert_eq!(frame.audio_generation, 4);
        assert!(!frame.frame.elements.is_empty());
        assert!(!frame.frame.controller_frames.is_empty());
    }

    #[test]
    fn path_change_rejects_stale_revision() {
        let (_temporary, root) = starter_copy();
        let state = crate::state::DesktopState::new();
        let snapshot = state.open_project_path(root.as_str());
        let error = state
            .plan_workspace_path_change(WorkspacePathChangeRequest {
                source: "effects/impact-burst.effect.dawn".to_string(),
                destination: "effects/impact.effect.dawn".to_string(),
                project_revision: snapshot.project_revision.saturating_sub(1),
            })
            .unwrap_err();
        assert!(error.contains("project changed"));
    }

    #[test]
    fn structural_path_change_rejects_dirty_open_text() {
        let (_temporary, root) = starter_copy();
        let state = crate::state::DesktopState::new();
        state.open_project_path(root.as_str());
        state.open_file_path("sequences/layer_test.sequence.dawn");
        state.update_active_text("dirty text".to_string());
        let revision = state.snapshot().project_revision;
        let error = state
            .apply_workspace_path_change(WorkspacePathChangeRequest {
                source: "effects/impact-burst.effect.dawn".to_string(),
                destination: "effects/impact.effect.dawn".to_string(),
                project_revision: revision,
            })
            .unwrap_err();
        assert!(error.contains("saved"));
        assert!(root.join("effects/impact-burst.effect.dawn").is_file());
    }
}
