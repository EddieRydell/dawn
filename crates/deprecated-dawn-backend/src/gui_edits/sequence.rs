use dawn_language::document::SequenceDocumentEdit;

use crate::gui_edits::types::SequenceGuiEdit;

pub fn sequence_document_edit_from_gui(edit: SequenceGuiEdit) -> SequenceDocumentEdit {
    match edit {
        SequenceGuiEdit::SetAudio { import } => SequenceDocumentEdit::SetAudio { import },
        SequenceGuiEdit::AddEffect {
            script,
            target,
            scope,
            start_seconds,
            mark_collection_key,
        } => SequenceDocumentEdit::AddEffect {
            script,
            target,
            scope,
            start_seconds,
            mark_collection_key,
        },
        SequenceGuiEdit::MoveEffect {
            id,
            start_seconds,
            target,
        } => SequenceDocumentEdit::MoveEffect {
            id,
            start_seconds,
            target,
        },
        SequenceGuiEdit::ResizeEffect {
            id,
            start_seconds,
            duration_seconds,
        } => SequenceDocumentEdit::ResizeEffect {
            id,
            start_seconds,
            duration_seconds,
        },
        SequenceGuiEdit::ChangeEffectScript { id, script } => {
            SequenceDocumentEdit::ChangeEffectScript { id, script }
        }
        SequenceGuiEdit::DeleteEffect { id } => SequenceDocumentEdit::DeleteEffect { id },
        SequenceGuiEdit::RetargetEffect { id, target } => {
            SequenceDocumentEdit::RetargetEffect { id, target }
        }
        SequenceGuiEdit::SetEffectScope { id, scope } => {
            SequenceDocumentEdit::SetEffectScope { id, scope }
        }
        SequenceGuiEdit::UpdateEffectParam { id, name, value } => {
            SequenceDocumentEdit::UpdateEffectParam { id, name, value }
        }
        SequenceGuiEdit::LinkEffectCurveParam {
            id,
            name,
            curve_path,
            object_key,
        } => SequenceDocumentEdit::LinkEffectCurveParam {
            id,
            name,
            curve_path,
            object_key,
        },
        SequenceGuiEdit::UnlinkEffectCurveParam { id, name } => {
            SequenceDocumentEdit::UnlinkEffectCurveParam { id, name }
        }
        SequenceGuiEdit::CreateMarkCollection { key, name, color } => {
            SequenceDocumentEdit::CreateMarkCollection { key, name, color }
        }
        SequenceGuiEdit::RenameMarkCollection { key, name } => {
            SequenceDocumentEdit::RenameMarkCollection { key, name }
        }
        SequenceGuiEdit::DeleteMarkCollection { key } => {
            SequenceDocumentEdit::DeleteMarkCollection { key }
        }
        SequenceGuiEdit::SetMarkCollectionColor { key, color } => {
            SequenceDocumentEdit::SetMarkCollectionColor { key, color }
        }
        SequenceGuiEdit::AddMark {
            collection_key,
            time_seconds,
        } => SequenceDocumentEdit::AddMark {
            collection_key,
            time_seconds,
        },
        SequenceGuiEdit::MoveMark {
            collection_key,
            index,
            time_seconds,
        } => SequenceDocumentEdit::MoveMark {
            collection_key,
            index: index as usize,
            time_seconds,
        },
        SequenceGuiEdit::DeleteMark {
            collection_key,
            index,
        } => SequenceDocumentEdit::DeleteMark {
            collection_key,
            index: index as usize,
        },
    }
}
