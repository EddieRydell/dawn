use dawn_language::document::{DocumentViewId, SequenceDocument};
use dawn_language::path::Utf8PathBuf;

use super::CoordinatorState;

impl CoordinatorState {
    pub(crate) fn active_sequence_export_source(
        &self,
    ) -> Result<
        (
            dawn_language::analysis::ProjectAnalysis,
            SequenceDocument,
            String,
        ),
        String,
    > {
        let analysis = self
            .workspace
            .analysis()
            .ok_or_else(|| "project analysis is not available".to_string())?
            .clone();
        if analysis.has_errors() {
            return Err("project has analysis errors".to_string());
        }
        if self
            .active_gui_document()
            .as_ref()
            .is_some_and(|document| document.is_blocked())
        {
            return Err("active document is blocked by diagnostics".to_string());
        }
        let path = self
            .document_store
            .active_file()
            .cloned()
            .ok_or_else(|| "no active sequence file is selected".to_string())?;
        let overlays = self.document_store.dirty_overlays();
        let descriptor = self
            .workspace
            .inspect_document(path.clone(), overlays.clone())?;
        let object_key = descriptor
            .default_object_keys
            .get(&DocumentViewId::Sequence)
            .cloned()
            .ok_or_else(|| "active document is not a sequence".to_string())?;
        let document = self
            .workspace
            .sequence_document(path, &object_key, overlays)?;
        let default_name = format!("{}.fseq", document.object_key);
        Ok((analysis, document, default_name))
    }

    pub(crate) fn active_sequence_audio_context(
        &self,
    ) -> Result<(Option<String>, Utf8PathBuf), String> {
        let Some(sequence_path) = self.document_store.active_file().cloned() else {
            return Err("no active sequence file is selected".to_string());
        };
        if !self
            .active_gui_document()
            .as_ref()
            .is_some_and(|document| document.is_sequence())
        {
            return Err("active document is not a sequence".to_string());
        }
        Ok((self.workspace.project_root(), sequence_path))
    }

    pub(crate) fn effect_preview_request_source(
        &self,
        path: Utf8PathBuf,
        object_key: &str,
    ) -> Result<(dawn_language::analysis::ProjectAnalysis, SequenceDocument), String> {
        let analysis = self
            .workspace
            .analysis()
            .ok_or_else(|| "project analysis is not available".to_string())?
            .clone();
        let document = self.active_sequence_document_for_preview_request(&path, object_key)?;
        Ok((analysis, document))
    }
}
