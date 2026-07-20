#[cfg(test)]
mod tests {
    use camino::Utf8Path;
    use dawn_project_io::load_package;

    use crate::dto::{DocumentViewId, GuiDocument, GuiDocumentRequest};

    fn starter() -> dawn_project_io::ProjectSession {
        let workspace = Utf8Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .and_then(Utf8Path::parent)
            .unwrap();
        load_package(&workspace.join("examples/starter"))
            .unwrap()
            .session
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
}
