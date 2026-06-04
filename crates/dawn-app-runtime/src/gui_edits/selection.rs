use dawn_language::document::{
    SequenceDocument, SequenceDocumentEdit, SequenceEffectMoveDocumentEdit,
    SequenceEffectResizeDocumentEdit, SequenceMarkMoveDocumentEdit, SequenceMarkPasteDocumentEdit,
    SequenceMarkRefDocumentEdit,
};
use dawn_language::model::{Authored, Sequence, SequenceEffect};

use crate::dto::{
    SequenceMarkRefDto, SequencePasteAnchorDto, SequenceResizeEdgeDto, SequenceSelectionDto,
    SequenceSelectionEditDto, SequenceSelectionEditResultDto,
};

const MIN_EFFECT_DURATION_SECONDS: f64 = 0.000000001;

#[derive(Debug, Clone)]
pub enum SequenceClipboard {
    Effects(Vec<SequenceEffect<Authored>>),
    Marks(Vec<SequenceMarkPasteDocumentEdit>),
}

pub struct SequenceSelectionEditOutcome {
    pub document_edit: Option<SequenceDocumentEdit>,
    pub result: SequenceSelectionEditResultDto,
    pub status_message: String,
}

pub fn plan_sequence_selection_edit(
    edit: SequenceSelectionEditDto,
    sequence_clipboard: &mut Option<SequenceClipboard>,
    sequence: &Sequence<Authored>,
    document: &SequenceDocument,
) -> Result<SequenceSelectionEditOutcome, String> {
    let resulting_selection;
    let mut copied_count = 0;
    let mut skipped_count = 0;

    let document_edit = match edit {
        SequenceSelectionEditDto::Copy { selection } => {
            copied_count =
                copy_sequence_selection(sequence_clipboard, sequence, document, &selection);
            return Ok(SequenceSelectionEditOutcome {
                document_edit: None,
                result: SequenceSelectionEditResultDto {
                    selection: Some(selection),
                    copied_count,
                    skipped_count,
                },
                status_message: format!("Copied {copied_count}"),
            });
        }
        SequenceSelectionEditDto::Cut { selection } => {
            copied_count =
                copy_sequence_selection(sequence_clipboard, sequence, document, &selection);
            let edit = sequence_delete_edit(selection.clone());
            resulting_selection = Some(selection_empty_like(&selection));
            SequenceSelectionEditOutcome {
                document_edit: Some(edit),
                result: SequenceSelectionEditResultDto {
                    selection: resulting_selection,
                    copied_count,
                    skipped_count,
                },
                status_message: format!("Cut {copied_count}"),
            }
        }
        SequenceSelectionEditDto::Delete { selection } => {
            let edit = sequence_delete_edit(selection.clone());
            resulting_selection = Some(selection_empty_like(&selection));
            SequenceSelectionEditOutcome {
                document_edit: Some(edit),
                result: SequenceSelectionEditResultDto {
                    selection: resulting_selection,
                    copied_count,
                    skipped_count,
                },
                status_message: "Deleted selection".to_string(),
            }
        }
        SequenceSelectionEditDto::Paste { anchor } => {
            let (edit, selection, skipped) =
                sequence_paste_edit(sequence_clipboard, document, anchor)?;
            skipped_count = skipped;
            copied_count = selection_count(&selection);
            let status_message = if skipped_count == 0 {
                format!("Pasted {copied_count}")
            } else {
                format!("Pasted {copied_count}, skipped {skipped_count}")
            };
            SequenceSelectionEditOutcome {
                document_edit: Some(edit),
                result: SequenceSelectionEditResultDto {
                    selection: Some(selection),
                    copied_count,
                    skipped_count,
                },
                status_message,
            }
        }
        SequenceSelectionEditDto::MoveEffects {
            ids,
            time_delta_seconds,
            lane_delta,
        } => {
            let edits = effect_move_edits(document, ids.clone(), time_delta_seconds, lane_delta);
            SequenceSelectionEditOutcome {
                document_edit: Some(SequenceDocumentEdit::MoveEffects { edits }),
                result: SequenceSelectionEditResultDto {
                    selection: Some(SequenceSelectionDto::Effects { ids }),
                    copied_count,
                    skipped_count,
                },
                status_message: "Moved selection".to_string(),
            }
        }
        SequenceSelectionEditDto::ResizeEffects {
            ids,
            edge,
            time_delta_seconds,
        } => {
            let edits = effect_resize_edits(document, ids.clone(), edge, time_delta_seconds);
            SequenceSelectionEditOutcome {
                document_edit: Some(SequenceDocumentEdit::ResizeEffects { edits }),
                result: SequenceSelectionEditResultDto {
                    selection: Some(SequenceSelectionDto::Effects { ids }),
                    copied_count,
                    skipped_count,
                },
                status_message: "Resized selection".to_string(),
            }
        }
        SequenceSelectionEditDto::MoveMarks {
            marks,
            time_delta_seconds,
        } => {
            let edits = mark_move_edits(document, marks.clone(), time_delta_seconds);
            SequenceSelectionEditOutcome {
                document_edit: Some(SequenceDocumentEdit::MoveMarks { edits }),
                result: SequenceSelectionEditResultDto {
                    selection: Some(SequenceSelectionDto::Marks { marks }),
                    copied_count,
                    skipped_count,
                },
                status_message: "Moved selection".to_string(),
            }
        }
    };

    Ok(document_edit)
}

fn sequence_delete_edit(selection: SequenceSelectionDto) -> SequenceDocumentEdit {
    match selection {
        SequenceSelectionDto::Effects { ids } => SequenceDocumentEdit::DeleteEffects { ids },
        SequenceSelectionDto::Marks { marks } => SequenceDocumentEdit::DeleteMarks {
            marks: marks
                .into_iter()
                .map(|mark| SequenceMarkRefDocumentEdit {
                    collection_key: mark.collection_key,
                    index: mark.index as usize,
                })
                .collect(),
        },
    }
}

fn selection_empty_like(selection: &SequenceSelectionDto) -> SequenceSelectionDto {
    match selection {
        SequenceSelectionDto::Effects { .. } => SequenceSelectionDto::Effects { ids: Vec::new() },
        SequenceSelectionDto::Marks { .. } => SequenceSelectionDto::Marks { marks: Vec::new() },
    }
}

fn selection_count(selection: &SequenceSelectionDto) -> u32 {
    match selection {
        SequenceSelectionDto::Effects { ids } => ids.len().min(u32::MAX as usize) as u32,
        SequenceSelectionDto::Marks { marks } => marks.len().min(u32::MAX as usize) as u32,
    }
}

fn effect_move_edits(
    document: &SequenceDocument,
    ids: Vec<u32>,
    time_delta_seconds: f64,
    lane_delta: i32,
) -> Vec<SequenceEffectMoveDocumentEdit> {
    ids.into_iter()
        .filter_map(|id| {
            let effect = document
                .effects
                .iter()
                .find(|candidate| candidate.id == id)?;
            let current_lane = document
                .lanes
                .iter()
                .position(|lane| lane.target == effect.target)
                .unwrap_or(0);
            let lane_index = (current_lane as i32 + lane_delta)
                .clamp(0, document.lanes.len().saturating_sub(1) as i32)
                as usize;
            Some(SequenceEffectMoveDocumentEdit {
                id,
                start_seconds: (effect.start_seconds + time_delta_seconds).clamp(
                    0.0,
                    (document.duration_seconds - effect.duration_seconds).max(0.0),
                ),
                target: document
                    .lanes
                    .get(lane_index)
                    .map(|lane| lane.target.clone()),
            })
        })
        .collect()
}

fn effect_resize_edits(
    document: &SequenceDocument,
    ids: Vec<u32>,
    edge: SequenceResizeEdgeDto,
    time_delta_seconds: f64,
) -> Vec<SequenceEffectResizeDocumentEdit> {
    ids.into_iter()
        .filter_map(|id| {
            let effect = document
                .effects
                .iter()
                .find(|candidate| candidate.id == id)?;
            let (start_seconds, duration_seconds) = match edge {
                SequenceResizeEdgeDto::Left => {
                    let end_seconds = effect.start_seconds + effect.duration_seconds;
                    let start_seconds = (effect.start_seconds + time_delta_seconds)
                        .clamp(0.0, end_seconds - MIN_EFFECT_DURATION_SECONDS);
                    (start_seconds, end_seconds - start_seconds)
                }
                SequenceResizeEdgeDto::Right => {
                    let duration_seconds = (effect.duration_seconds + time_delta_seconds).clamp(
                        MIN_EFFECT_DURATION_SECONDS,
                        document.duration_seconds - effect.start_seconds,
                    );
                    (effect.start_seconds, duration_seconds)
                }
            };
            Some(SequenceEffectResizeDocumentEdit {
                id,
                start_seconds,
                duration_seconds,
            })
        })
        .collect()
}

fn mark_move_edits(
    document: &SequenceDocument,
    marks: Vec<SequenceMarkRefDto>,
    time_delta_seconds: f64,
) -> Vec<SequenceMarkMoveDocumentEdit> {
    marks
        .into_iter()
        .filter_map(|mark| {
            let collection = document
                .mark_collections
                .iter()
                .find(|collection| collection.key == mark.collection_key)?;
            let time_seconds = collection.marks_seconds.get(mark.index as usize)?;
            Some(SequenceMarkMoveDocumentEdit {
                collection_key: mark.collection_key,
                index: mark.index as usize,
                time_seconds: (time_seconds + time_delta_seconds)
                    .clamp(0.0, document.duration_seconds),
            })
        })
        .collect()
}

fn copy_sequence_selection(
    sequence_clipboard: &mut Option<SequenceClipboard>,
    sequence: &Sequence<Authored>,
    document: &SequenceDocument,
    selection: &SequenceSelectionDto,
) -> u32 {
    match selection {
        SequenceSelectionDto::Effects { ids } => {
            let effects = ids
                .iter()
                .filter_map(|id| {
                    sequence
                        .effects
                        .iter()
                        .find(|effect| effect.id == *id)
                        .cloned()
                })
                .collect::<Vec<_>>();
            let count = effects.len().min(u32::MAX as usize) as u32;
            *sequence_clipboard = Some(SequenceClipboard::Effects(effects));
            count
        }
        SequenceSelectionDto::Marks { marks } => {
            let copied = marks
                .iter()
                .filter_map(|mark| {
                    document
                        .mark_collections
                        .iter()
                        .find(|collection| collection.key == mark.collection_key)
                        .and_then(|collection| collection.marks_seconds.get(mark.index as usize))
                        .map(|time_seconds| SequenceMarkPasteDocumentEdit {
                            collection_key: mark.collection_key.clone(),
                            time_seconds: *time_seconds,
                        })
                })
                .collect::<Vec<_>>();
            let count = copied.len().min(u32::MAX as usize) as u32;
            *sequence_clipboard = Some(SequenceClipboard::Marks(copied));
            count
        }
    }
}

fn sequence_paste_edit(
    sequence_clipboard: &Option<SequenceClipboard>,
    document: &SequenceDocument,
    anchor: SequencePasteAnchorDto,
) -> Result<(SequenceDocumentEdit, SequenceSelectionDto, u32), String> {
    match sequence_clipboard.clone() {
        Some(SequenceClipboard::Effects(effects)) => {
            let first_id = document
                .effects
                .iter()
                .map(|effect| effect.id)
                .max()
                .unwrap_or(0)
                + 1;
            let ids = (0..effects.len())
                .map(|offset| first_id + offset as u32)
                .collect::<Vec<_>>();
            Ok((
                SequenceDocumentEdit::PasteEffects {
                    effects,
                    lane_index: anchor.lane_index.map(|value| value as usize),
                    time_seconds: anchor.time_seconds,
                },
                SequenceSelectionDto::Effects { ids },
                0,
            ))
        }
        Some(SequenceClipboard::Marks(marks)) => {
            let existing = document
                .mark_collections
                .iter()
                .map(|collection| (collection.key.clone(), collection.marks_seconds.len()))
                .collect::<std::collections::HashMap<_, _>>();
            let mut refs = Vec::new();
            for mark in &marks {
                if let Some(index) = existing.get(&mark.collection_key) {
                    refs.push(SequenceMarkRefDto {
                        collection_key: mark.collection_key.clone(),
                        index: *index as u32,
                    });
                }
            }
            let skipped = marks
                .len()
                .saturating_sub(refs.len())
                .min(u32::MAX as usize) as u32;
            Ok((
                SequenceDocumentEdit::PasteMarks {
                    marks,
                    time_seconds: anchor.time_seconds,
                },
                SequenceSelectionDto::Marks { marks: refs },
                skipped,
            ))
        }
        None => Err("sequence clipboard is empty".to_string()),
    }
}
