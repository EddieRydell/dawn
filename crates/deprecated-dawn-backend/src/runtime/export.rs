use dawn_language::document::{DocumentViewId, SequenceDocument};

use crate::output::fseq_export::{export_fseq_file, FseqExportOptions};
use crate::runtime::contracts::{RuntimeNotice, RuntimeStatus};

use super::AppBackend;

impl AppBackend {
    pub(super) fn active_sequence_fseq_default_name_command(&self) -> Result<String, String> {
        let (_, _, default_name) = self.active_sequence_export_source()?;
        Ok(default_name)
    }

    pub(super) fn export_active_sequence_fseq_command(
        &mut self,
        output_path: &std::path::Path,
        step_ms: u8,
    ) -> Result<(), String> {
        let (analysis, document, _) = self.active_sequence_export_source()?;
        let report = export_fseq_file(
            &analysis,
            &document,
            output_path,
            FseqExportOptions {
                step_ms,
                ..FseqExportOptions::default()
            },
        )
        .map_err(|error| error.to_string())?;
        self.status = RuntimeStatus::notice(RuntimeNotice::ExportedFseq {
            frame_count: report.frame_count,
            channel_count: report.channel_count,
        });
        Ok(())
    }

    fn active_sequence_export_source(
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
}
