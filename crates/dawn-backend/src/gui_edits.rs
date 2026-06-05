use dawn_language::{
    document::{
        LayoutDocument, SequenceDocument, SequenceDocumentEdit, SequenceEffectMoveDocumentEdit,
        SequenceEffectResizeDocumentEdit, SequenceMarkMoveDocumentEdit,
        SequenceMarkPasteDocumentEdit, SequenceMarkRefDocumentEdit,
    },
    model::{Authored, DawnFile, DawnObject, FixtureId, Geometry, Sequence},
};

use crate::types::{
    FixtureGuiEdit, LayoutGuiEdit, SequenceClipboard, SequenceGuiEdit, SequenceMarkRef,
    SequenceResizeEdge, SequenceSelection, SequenceSelectionEdit, SequenceSelectionEditResult,
};

const MIN_EFFECT_DURATION_SECONDS: f64 = 0.000000001;

#[derive(Debug, Clone)]
pub(crate) struct SequenceSelectionEditOutcome {
    pub(crate) document_edit: Option<SequenceDocumentEdit>,
    pub(crate) result: SequenceSelectionEditResult,
}

pub(crate) fn sequence_document_edit_from_gui(edit: SequenceGuiEdit) -> SequenceDocumentEdit {
    match edit {
        SequenceGuiEdit::Document(edit) => edit,
    }
}

pub(crate) fn apply_layout_gui_edit(
    document: &mut LayoutDocument,
    edit: LayoutGuiEdit,
) -> Result<(), String> {
    match edit {
        LayoutGuiEdit::UpdatePlacementTransform { id, transform } => {
            let id = FixtureId(id);
            let placement = document
                .fixtures
                .iter_mut()
                .find(|fixture| fixture.id == id)
                .ok_or_else(|| format!("fixture placement `{id}` was not found"))?;
            placement.transform = transform;
        }
    }
    Ok(())
}

pub(crate) fn apply_fixture_gui_edit(
    document: &mut dawn_language::document::FixtureDocument,
    edit: FixtureGuiEdit,
) -> Result<(), String> {
    match edit {
        FixtureGuiEdit::UpdateBulbDiameter {
            object_key,
            bulb_diameter_meters,
        } => {
            let fixture = document
                .fixtures
                .iter_mut()
                .find(|fixture| fixture.object_key == object_key)
                .ok_or_else(|| format!("fixture `{object_key}` was not found"))?;
            fixture.bulb_diameter =
                dawn_language::model::DistanceSpan::try_from_meters_f64_truncated(
                    bulb_diameter_meters,
                )
                .map_err(str::to_string)?;
        }
        FixtureGuiEdit::MovePoint {
            object_key,
            point_index,
            point,
        } => {
            let fixture = document
                .fixtures
                .iter_mut()
                .find(|fixture| fixture.object_key == object_key)
                .ok_or_else(|| format!("fixture `{object_key}` was not found"))?;
            let Geometry::Points { points } = &mut fixture.geometry else {
                return Err("only point geometry can be edited in this milestone".to_string());
            };
            let target = points
                .get_mut(point_index as usize)
                .ok_or_else(|| format!("point `{point_index}` was not found"))?;
            *target = point;
        }
    }
    Ok(())
}

pub(crate) fn parse_authored_sequence(
    text: &str,
    object_key: &str,
) -> Result<Sequence<Authored>, String> {
    let file = serde_yaml::from_str::<DawnFile>(text).map_err(|error| error.to_string())?;
    let object = file
        .get(object_key)
        .ok_or_else(|| format!("sequence object `{object_key}` was not found"))?;
    let DawnObject::Sequence(sequence) = object else {
        return Err(format!("object `{object_key}` is not a sequence"));
    };
    Ok(sequence.clone())
}

pub(crate) fn plan_sequence_selection_edit(
    edit: SequenceSelectionEdit,
    sequence_clipboard: &mut Option<SequenceClipboard>,
    sequence: &Sequence<Authored>,
    document: &SequenceDocument,
) -> Result<SequenceSelectionEditOutcome, String> {
    let document_edit = match edit {
        SequenceSelectionEdit::Copy { selection } => {
            let copied_count =
                copy_sequence_selection(sequence_clipboard, sequence, document, &selection);
            return Ok(SequenceSelectionEditOutcome {
                document_edit: None,
                result: SequenceSelectionEditResult {
                    selection: Some(selection),
                    copied_count,
                    skipped_count: 0,
                },
            });
        }
        SequenceSelectionEdit::Cut { selection } => {
            let copied_count =
                copy_sequence_selection(sequence_clipboard, sequence, document, &selection);
            SequenceSelectionEditOutcome {
                document_edit: Some(sequence_delete_edit(selection.clone())),
                result: SequenceSelectionEditResult {
                    selection: Some(selection_empty_like(&selection)),
                    copied_count,
                    skipped_count: 0,
                },
            }
        }
        SequenceSelectionEdit::Delete { selection } => SequenceSelectionEditOutcome {
            document_edit: Some(sequence_delete_edit(selection.clone())),
            result: SequenceSelectionEditResult {
                selection: Some(selection_empty_like(&selection)),
                copied_count: 0,
                skipped_count: 0,
            },
        },
        SequenceSelectionEdit::Paste { anchor } => {
            let (edit, selection, skipped_count) =
                sequence_paste_edit(sequence_clipboard, document, anchor)?;
            SequenceSelectionEditOutcome {
                document_edit: Some(edit),
                result: SequenceSelectionEditResult {
                    copied_count: selection_count(&selection),
                    selection: Some(selection),
                    skipped_count,
                },
            }
        }
        SequenceSelectionEdit::MoveEffects {
            ids,
            time_delta_seconds,
            lane_delta,
        } => SequenceSelectionEditOutcome {
            document_edit: Some(SequenceDocumentEdit::MoveEffects {
                edits: effect_move_edits(document, ids.clone(), time_delta_seconds, lane_delta),
            }),
            result: SequenceSelectionEditResult {
                selection: Some(SequenceSelection::Effects { ids }),
                copied_count: 0,
                skipped_count: 0,
            },
        },
        SequenceSelectionEdit::ResizeEffects {
            ids,
            edge,
            time_delta_seconds,
        } => SequenceSelectionEditOutcome {
            document_edit: Some(SequenceDocumentEdit::ResizeEffects {
                edits: effect_resize_edits(document, ids.clone(), edge, time_delta_seconds),
            }),
            result: SequenceSelectionEditResult {
                selection: Some(SequenceSelection::Effects { ids }),
                copied_count: 0,
                skipped_count: 0,
            },
        },
        SequenceSelectionEdit::MoveMarks {
            marks,
            time_delta_seconds,
        } => SequenceSelectionEditOutcome {
            document_edit: Some(SequenceDocumentEdit::MoveMarks {
                edits: mark_move_edits(document, marks.clone(), time_delta_seconds),
            }),
            result: SequenceSelectionEditResult {
                selection: Some(SequenceSelection::Marks { marks }),
                copied_count: 0,
                skipped_count: 0,
            },
        },
    };

    Ok(document_edit)
}

fn sequence_delete_edit(selection: SequenceSelection) -> SequenceDocumentEdit {
    match selection {
        SequenceSelection::Effects { ids } => SequenceDocumentEdit::DeleteEffects { ids },
        SequenceSelection::Marks { marks } => SequenceDocumentEdit::DeleteMarks {
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

fn selection_empty_like(selection: &SequenceSelection) -> SequenceSelection {
    match selection {
        SequenceSelection::Effects { .. } => SequenceSelection::Effects { ids: Vec::new() },
        SequenceSelection::Marks { .. } => SequenceSelection::Marks { marks: Vec::new() },
    }
}

fn selection_count(selection: &SequenceSelection) -> u32 {
    match selection {
        SequenceSelection::Effects { ids } => ids.len().min(u32::MAX as usize) as u32,
        SequenceSelection::Marks { marks } => marks.len().min(u32::MAX as usize) as u32,
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
    edge: SequenceResizeEdge,
    time_delta_seconds: f64,
) -> Vec<SequenceEffectResizeDocumentEdit> {
    ids.into_iter()
        .filter_map(|id| {
            let effect = document
                .effects
                .iter()
                .find(|candidate| candidate.id == id)?;
            let (start_seconds, duration_seconds) = match edge {
                SequenceResizeEdge::Left => {
                    let end_seconds = effect.start_seconds + effect.duration_seconds;
                    let start_seconds = (effect.start_seconds + time_delta_seconds)
                        .clamp(0.0, end_seconds - MIN_EFFECT_DURATION_SECONDS);
                    (start_seconds, end_seconds - start_seconds)
                }
                SequenceResizeEdge::Right => {
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
    marks: Vec<SequenceMarkRef>,
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
    selection: &SequenceSelection,
) -> u32 {
    match selection {
        SequenceSelection::Effects { ids } => {
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
        SequenceSelection::Marks { marks } => {
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
    anchor: crate::types::SequencePasteAnchor,
) -> Result<(SequenceDocumentEdit, SequenceSelection, u32), String> {
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
                SequenceSelection::Effects { ids },
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
                    refs.push(SequenceMarkRef {
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
                SequenceSelection::Marks { marks: refs },
                skipped,
            ))
        }
        None => Err("sequence clipboard is empty".to_string()),
    }
}
