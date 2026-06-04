use dawn_language::document::SequenceDocumentEdit;

use crate::dto::SequenceGuiEditDto;

pub fn sequence_document_edit_from_gui(edit: SequenceGuiEditDto) -> SequenceDocumentEdit {
    match edit {
        SequenceGuiEditDto::SetAudio { import } => SequenceDocumentEdit::SetAudio { import },
        SequenceGuiEditDto::AddEffect {
            script,
            target,
            scope,
            start_seconds,
            mark_collection_key,
        } => SequenceDocumentEdit::AddEffect {
            script: script.into(),
            target: target.into(),
            scope: scope.into(),
            start_seconds,
            mark_collection_key,
        },
        SequenceGuiEditDto::MoveEffect {
            id,
            start_seconds,
            target,
        } => SequenceDocumentEdit::MoveEffect {
            id,
            start_seconds,
            target: target.map(Into::into),
        },
        SequenceGuiEditDto::ResizeEffect {
            id,
            start_seconds,
            duration_seconds,
        } => SequenceDocumentEdit::ResizeEffect {
            id,
            start_seconds,
            duration_seconds,
        },
        SequenceGuiEditDto::ChangeEffectScript { id, script } => {
            SequenceDocumentEdit::ChangeEffectScript {
                id,
                script: script.into(),
            }
        }
        SequenceGuiEditDto::DeleteEffect { id } => SequenceDocumentEdit::DeleteEffect { id },
        SequenceGuiEditDto::RetargetEffect { id, target } => SequenceDocumentEdit::RetargetEffect {
            id,
            target: target.into(),
        },
        SequenceGuiEditDto::SetEffectScope { id, scope } => SequenceDocumentEdit::SetEffectScope {
            id,
            scope: scope.into(),
        },
        SequenceGuiEditDto::UpdateEffectParam { id, name, value } => {
            SequenceDocumentEdit::UpdateEffectParam {
                id,
                name,
                value: value.into(),
            }
        }
        SequenceGuiEditDto::LinkEffectCurveParam {
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
        SequenceGuiEditDto::UnlinkEffectCurveParam { id, name } => {
            SequenceDocumentEdit::UnlinkEffectCurveParam { id, name }
        }
        SequenceGuiEditDto::CreateMarkCollection { key, name, color } => {
            SequenceDocumentEdit::CreateMarkCollection { key, name, color }
        }
        SequenceGuiEditDto::RenameMarkCollection { key, name } => {
            SequenceDocumentEdit::RenameMarkCollection { key, name }
        }
        SequenceGuiEditDto::DeleteMarkCollection { key } => {
            SequenceDocumentEdit::DeleteMarkCollection { key }
        }
        SequenceGuiEditDto::SetMarkCollectionColor { key, color } => {
            SequenceDocumentEdit::SetMarkCollectionColor { key, color }
        }
        SequenceGuiEditDto::AddMark {
            collection_key,
            time_seconds,
        } => SequenceDocumentEdit::AddMark {
            collection_key,
            time_seconds,
        },
        SequenceGuiEditDto::MoveMark {
            collection_key,
            index,
            time_seconds,
        } => SequenceDocumentEdit::MoveMark {
            collection_key,
            index: index as usize,
            time_seconds,
        },
        SequenceGuiEditDto::DeleteMark {
            collection_key,
            index,
        } => SequenceDocumentEdit::DeleteMark {
            collection_key,
            index: index as usize,
        },
    }
}
