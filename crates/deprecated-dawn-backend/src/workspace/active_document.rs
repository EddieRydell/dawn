use dawn_language::analysis::{ProjectDiagnostic, ProjectOverlay};
use dawn_language::document::{
    DocumentDescriptor, DocumentViewId, FixtureDocument, LayoutDocument, SequenceDocument,
};

use crate::editor::{BufferTab, EditorViewMode};
use crate::workspace::ProjectWorkspace;

#[derive(Debug, Clone)]
pub enum ActiveGuiDocument {
    Sequence(SequenceDocument),
    Layout(LayoutDocument),
    Fixture(FixtureDocument),
    Blocked {
        reason: String,
        diagnostics: Vec<ProjectDiagnostic>,
    },
}

impl ActiveGuiDocument {
    pub fn is_sequence(&self) -> bool {
        matches!(self, Self::Sequence(_))
    }

    pub fn is_blocked(&self) -> bool {
        matches!(self, Self::Blocked { .. })
    }
}

pub fn build_active_gui_document(
    workspace: &ProjectWorkspace,
    active_buffer: Option<&BufferTab>,
    diagnostics: &[ProjectDiagnostic],
    descriptor: Option<&DocumentDescriptor>,
    overlays: Vec<ProjectOverlay>,
) -> Option<ActiveGuiDocument> {
    let buffer = active_buffer?;
    if buffer.view_mode != EditorViewMode::Gui {
        return None;
    }
    if buffer.is_conflicted() {
        return Some(ActiveGuiDocument::Blocked {
            reason: "This document has external disk changes.".to_string(),
            diagnostics: Vec::new(),
        });
    }
    let diagnostics = diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.path == buffer.path)
        .cloned()
        .collect::<Vec<_>>();
    let Some(descriptor) = descriptor else {
        return Some(ActiveGuiDocument::Blocked {
            reason: "Text could not be parsed as a Dawn document.".to_string(),
            diagnostics,
        });
    };
    if let Some(object_key) = descriptor
        .default_object_keys
        .get(&DocumentViewId::Sequence)
    {
        return Some(
            match workspace.sequence_document(buffer.path.clone(), object_key, overlays) {
                Ok(document) => ActiveGuiDocument::Sequence(document),
                Err(error) => ActiveGuiDocument::Blocked {
                    reason: error,
                    diagnostics,
                },
            },
        );
    }
    if let Some(object_key) = descriptor.default_object_keys.get(&DocumentViewId::Layout) {
        return Some(
            match workspace.layout_document(buffer.path.clone(), object_key, overlays) {
                Ok(document) => ActiveGuiDocument::Layout(document),
                Err(error) => ActiveGuiDocument::Blocked {
                    reason: error,
                    diagnostics,
                },
            },
        );
    }
    if descriptor
        .default_object_keys
        .contains_key(&DocumentViewId::Fixture)
    {
        return Some(
            match workspace.fixture_document(buffer.path.clone(), None, overlays) {
                Ok(document) => ActiveGuiDocument::Fixture(document),
                Err(error) => ActiveGuiDocument::Blocked {
                    reason: error,
                    diagnostics,
                },
            },
        );
    }
    Some(ActiveGuiDocument::Blocked {
        reason: "This document has no GUI editor view.".to_string(),
        diagnostics,
    })
}
