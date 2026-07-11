use super::*;

pub(super) fn current_curve_param_value(
    session: &ProjectSession,
    resolved: &ResolvedGuiObject,
    effect_id: u32,
    name: &str,
) -> Result<SequenceEffectParamValue, GuiMutationError> {
    let sequence_id = SequenceId(SourceIdentity::new(
        resolved.identity.document().to_path_buf(),
        resolved.identity.object().to_string(),
    ));
    let sequence = session
        .project
        .sequences
        .get(&sequence_id)
        .ok_or_else(|| GuiMutationError::Invalid("Sequence was not found.".to_string()))?;
    let effect = sequence
        .effects
        .iter()
        .find(|effect| effect.id.0 == effect_id)
        .ok_or_else(|| GuiMutationError::Invalid("Effect was not found.".to_string()))?;

    if let Some(value) = effect
        .param_overrides
        .iter()
        .find_map(|(key, value)| (key.as_str() == name).then_some(value))
    {
        return match value {
            EffectParamValue::Curve(_) => Ok(effect_param_value(session, value)),
            _ => Err(GuiMutationError::Invalid(
                "Param is not a curve param.".to_string(),
            )),
        };
    }

    let definition = session
        .project
        .definitions
        .effects
        .get(&effect.definition)
        .ok_or_else(|| GuiMutationError::Invalid("Effect definition was not found.".to_string()))?;
    let param = definition
        .compiled
        .params()
        .iter()
        .find(|param| param.name.as_str() == name)
        .ok_or_else(|| GuiMutationError::Invalid("Effect param was not found.".to_string()))?;
    match &param.ty {
        Type::Curve(inner) => Ok(param
            .default
            .as_ref()
            .and_then(default_param_value)
            .unwrap_or_else(|| match inner.as_ref() {
                Type::Color => SequenceEffectParamValue::ColorCurve { points: Vec::new() },
                _ => SequenceEffectParamValue::FloatCurve { points: Vec::new() },
            })),
        _ => Err(GuiMutationError::Invalid(
            "Param is not a curve param.".to_string(),
        )),
    }
}

pub(super) fn current_graph_curve_param_value(
    session: &ProjectSession,
    resolved: &ResolvedGuiObject,
    node_id: &str,
    name: &str,
) -> Result<SequenceEffectParamValue, GuiMutationError> {
    let sequence_id = SequenceId(SourceIdentity::new(
        resolved.identity.document().to_path_buf(),
        resolved.identity.object().to_string(),
    ));
    let sequence = session
        .project
        .sequences
        .get(&sequence_id)
        .ok_or_else(|| GuiMutationError::Invalid("Sequence was not found.".to_string()))?;
    let node_id = parse_graph_node_id(node_id)?;
    let node = sequence
        .composition_graph
        .nodes
        .iter()
        .find(|node| node.id == node_id)
        .ok_or_else(|| GuiMutationError::Invalid("Graph node was not found.".to_string()))?;
    let CompositionGraphNodeKind::Operator(operator) = &node.kind else {
        return Err(GuiMutationError::Invalid(
            "Graph node is not an operator.".to_string(),
        ));
    };
    if let Some(value) = operator
        .params
        .iter()
        .find_map(|(key, value)| (key.as_str() == name).then_some(value))
    {
        return match value {
            EffectParamValue::Curve(_) => Ok(effect_param_value(session, value)),
            _ => Err(GuiMutationError::Invalid(
                "Param is not a curve param.".to_string(),
            )),
        };
    }
    let definition = session
        .project
        .definitions
        .operators
        .resolve(&operator.operator)
        .ok_or_else(|| {
            GuiMutationError::Invalid("Operator definition was not found.".to_string())
        })?;
    let param = definition
        .params
        .iter()
        .find(|param| param.name.as_str() == name)
        .ok_or_else(|| GuiMutationError::Invalid("Operator param was not found.".to_string()))?;
    match &param.ty {
        Type::Curve(inner) => Ok(param
            .default
            .as_ref()
            .and_then(default_param_value)
            .unwrap_or_else(|| match inner.as_ref() {
                Type::Color => SequenceEffectParamValue::ColorCurve { points: Vec::new() },
                _ => SequenceEffectParamValue::FloatCurve { points: Vec::new() },
            })),
        _ => Err(GuiMutationError::Invalid(
            "Param is not a curve param.".to_string(),
        )),
    }
}

pub(super) fn required_operator_param_value(
    ty: Type,
    sequence: &dawn_language::sequence::Sequence,
) -> Result<EffectParamValue, GuiMutationError> {
    if ty == Type::Marks {
        return sequence
            .mark_collections
            .first()
            .map(|collection| EffectParamValue::Marks(collection.key.clone()))
            .ok_or_else(|| {
                GuiMutationError::Invalid(
                    "A required marks parameter needs a mark collection.".to_string(),
                )
            });
    }
    default_effect_param_value(&ty).ok_or_else(|| {
        GuiMutationError::Invalid(
            "A valid required operator parameter could not be created.".to_string(),
        )
    })
}

pub(super) fn copy_sequence_selection(
    session: &ProjectSession,
    sequence_id: &SequenceId,
    selection: &SequenceSelection,
) -> Result<(Option<SequenceClipboard>, u32, u32), GuiMutationError> {
    match selection {
        SequenceSelection::Effects { ids } => {
            let sequence =
                session.project.sequences.get(sequence_id).ok_or_else(|| {
                    GuiMutationError::Invalid("Sequence was not found.".to_string())
                })?;
            let mut copied = Vec::new();
            let mut skipped = 0u32;
            for id in ids {
                let Some(effect) = sequence.effects.iter().find(|effect| effect.id.0 == *id) else {
                    skipped = skipped.saturating_add(1);
                    continue;
                };
                copied.push(ClipboardEffect {
                    effect: effect.clone(),
                    start_seconds: effect.start.as_seconds_f64(),
                    lane_index: effect_lane_index(session, &effect.target),
                });
            }
            let copied_count = copied.len() as u32;
            Ok((
                (!copied.is_empty()).then_some(SequenceClipboard::Effects(copied)),
                copied_count,
                skipped,
            ))
        }
        SequenceSelection::Marks { marks } => {
            let sequence =
                session.project.sequences.get(sequence_id).ok_or_else(|| {
                    GuiMutationError::Invalid("Sequence was not found.".to_string())
                })?;
            let mut copied = Vec::new();
            let mut skipped = 0u32;
            for mark in marks {
                let Some(time_seconds) = mark_time_seconds(sequence, mark) else {
                    skipped = skipped.saturating_add(1);
                    continue;
                };
                copied.push(ClipboardMark {
                    collection_key: mark.collection_key.clone(),
                    time_seconds,
                });
            }
            let copied_count = copied.len() as u32;
            Ok((
                (!copied.is_empty()).then_some(SequenceClipboard::Marks(copied)),
                copied_count,
                skipped,
            ))
        }
    }
}

pub(super) fn delete_sequence_selection(
    session: &mut ProjectSession,
    sequence_id: &SequenceId,
    selection: &SequenceSelection,
) -> Result<(), GuiMutationError> {
    let sequence = sequence_mut(session, sequence_id)?;
    match selection {
        SequenceSelection::Effects { ids } => {
            sequence
                .effects
                .retain(|effect| !ids.contains(&effect.id.0));
            for clip in &mut sequence.automation_clips {
                clip.bindings.retain(|binding| {
                    binding
                        .effect_param()
                        .is_none_or(|(effect_id, _)| !ids.contains(&effect_id.0))
                });
            }
            sequence
                .automation_clips
                .retain(|clip| !clip.bindings.is_empty());
        }
        SequenceSelection::Marks { marks } => {
            for (collection_key, indexes) in mark_indexes_by_collection(marks) {
                for index in indexes.into_iter().rev() {
                    let collection = mark_collection_mut(sequence, &collection_key)?;
                    if index < collection.marks.len() {
                        collection.marks.remove(index);
                    }
                }
            }
        }
    }
    Ok(())
}

pub(super) fn paste_sequence_clipboard(
    session: &mut ProjectSession,
    sequence_id: &SequenceId,
    anchor: SequencePasteAnchor,
    clipboard: Option<&SequenceClipboard>,
) -> Result<SequenceSelectionMutation, GuiMutationError> {
    let Some(clipboard) = clipboard else {
        return Ok(SequenceSelectionMutation {
            selection: None,
            copied_count: 0,
            skipped_count: 0,
        });
    };
    let lane_count = sequence_lane_count(session);
    let lane_targets = (0..lane_count)
        .map(|lane| target_for_lane(session, lane))
        .collect::<Vec<_>>();
    match clipboard {
        SequenceClipboard::Effects(effects) => {
            let min_start = effects
                .iter()
                .map(|effect| effect.start_seconds)
                .fold(f64::INFINITY, f64::min);
            let min_lane = effects
                .iter()
                .map(|effect| effect.lane_index)
                .min()
                .unwrap_or_default();
            let sequence = sequence_mut(session, sequence_id)?;
            let mut next_id = sequence
                .effects
                .iter()
                .map(|effect| effect.id.0)
                .max()
                .unwrap_or(0)
                .saturating_add(1);
            let mut pasted_ids = Vec::with_capacity(effects.len());
            for effect in effects {
                let mut value = effect.effect.clone();
                let target_lane = anchored_lane(
                    anchor.lane_index as usize,
                    effect.lane_index,
                    min_lane,
                    lane_count,
                );
                value.id = EffectInstId(next_id);
                value.start =
                    dawn_time((anchor.time_seconds + effect.start_seconds - min_start).max(0.0));
                if let Some(Some(target)) = lane_targets.get(target_lane) {
                    value.target = target.clone();
                }
                sequence.effects.push(value);
                pasted_ids.push(next_id);
                next_id = next_id.saturating_add(1);
            }
            Ok(SequenceSelectionMutation {
                selection: Some(SequenceSelection::Effects { ids: pasted_ids }),
                copied_count: effects.len() as u32,
                skipped_count: 0,
            })
        }
        SequenceClipboard::Marks(marks) => {
            let min_time = marks
                .iter()
                .map(|mark| mark.time_seconds)
                .fold(f64::INFINITY, f64::min);
            let mut pasted = Vec::new();
            let mut skipped = 0u32;
            let sequence = sequence_mut(session, sequence_id)?;
            for mark in marks {
                let collection = match mark_collection_mut(sequence, &mark.collection_key) {
                    Ok(collection) => collection,
                    Err(_) => {
                        skipped = skipped.saturating_add(1);
                        continue;
                    }
                };
                let time_seconds = (anchor.time_seconds + mark.time_seconds - min_time).max(0.0);
                collection.marks.push(dawn_time(time_seconds));
                collection.marks.sort_by_key(|time| time.0);
                let index = collection
                    .marks
                    .iter()
                    .position(|value| (value.as_seconds_f64() - time_seconds).abs() < f64::EPSILON)
                    .unwrap_or_else(|| collection.marks.len().saturating_sub(1));
                pasted.push(SequenceMarkRef {
                    collection_key: mark.collection_key.clone(),
                    index: index as u32,
                });
            }
            Ok(SequenceSelectionMutation {
                selection: Some(SequenceSelection::Marks { marks: pasted }),
                copied_count: marks.len() as u32,
                skipped_count: skipped,
            })
        }
    }
}

pub(super) fn move_effect_selection(
    session: &mut ProjectSession,
    sequence_id: &SequenceId,
    ids: &[u32],
    time_delta_seconds: f64,
    lane_delta: i32,
) -> Result<Vec<u32>, GuiMutationError> {
    let effect_updates = effect_selection_updates(session, sequence_id, ids, |session, effect| {
        let lane = shifted_lane(
            effect_lane_index(session, &effect.target),
            lane_delta,
            sequence_lane_count(session),
        );
        (
            effect.start.as_seconds_f64() + time_delta_seconds,
            effect.duration.as_seconds_f64(),
            lane,
        )
    })?;
    apply_effect_updates(session, sequence_id, effect_updates)
}

pub(super) fn resize_effect_selection(
    session: &mut ProjectSession,
    sequence_id: &SequenceId,
    ids: &[u32],
    edge: SequenceResizeEdge,
    time_delta_seconds: f64,
) -> Result<(), GuiMutationError> {
    let effect_updates = effect_selection_updates(session, sequence_id, ids, |session, effect| {
        let start_seconds = effect.start.as_seconds_f64();
        let duration_seconds = effect.duration.as_seconds_f64();
        let lane = effect_lane_index(session, &effect.target);
        match edge {
            SequenceResizeEdge::Left => (
                start_seconds + time_delta_seconds,
                duration_seconds - time_delta_seconds,
                lane,
            ),
            SequenceResizeEdge::Right => {
                (start_seconds, duration_seconds + time_delta_seconds, lane)
            }
        }
    })?;
    apply_effect_updates(session, sequence_id, effect_updates)?;
    Ok(())
}

pub(super) fn move_mark_selection(
    session: &mut ProjectSession,
    sequence_id: &SequenceId,
    marks: &[SequenceMarkRef],
    time_delta_seconds: f64,
) -> Result<Vec<SequenceMarkRef>, GuiMutationError> {
    let sequence = sequence_mut(session, sequence_id)?;
    let mut moved = Vec::new();
    for (collection_key, indexes) in mark_indexes_by_collection(marks) {
        let mut moved_times = Vec::new();
        for index in indexes {
            let collection = mark_collection_mut(sequence, &collection_key)?;
            if let Some(value) = collection.marks.get_mut(index) {
                let time_seconds = (value.as_seconds_f64() + time_delta_seconds).max(0.0);
                *value = dawn_time(time_seconds);
                moved_times.push(time_seconds);
            }
        }
        let collection = mark_collection_mut(sequence, &collection_key)?;
        collection.marks.sort_by_key(|time| time.0);
        for time_seconds in moved_times {
            if let Some(index) = collection
                .marks
                .iter()
                .position(|value| (value.as_seconds_f64() - time_seconds).abs() < f64::EPSILON)
            {
                moved.push(SequenceMarkRef {
                    collection_key: collection_key.clone(),
                    index: index as u32,
                });
            }
        }
    }
    Ok(moved)
}

pub(super) struct EffectUpdate {
    id: u32,
    start_seconds: f64,
    duration_seconds: f64,
    lane_index: usize,
}

pub(super) fn effect_selection_updates(
    session: &ProjectSession,
    sequence_id: &SequenceId,
    ids: &[u32],
    update: impl Fn(&ProjectSession, &dawn_language::effect::EffectInst) -> (f64, f64, usize),
) -> Result<Vec<EffectUpdate>, GuiMutationError> {
    let sequence = session
        .project
        .sequences
        .get(sequence_id)
        .ok_or_else(|| GuiMutationError::Invalid("Sequence was not found.".to_string()))?;
    Ok(sequence
        .effects
        .iter()
        .filter(|effect| ids.contains(&effect.id.0))
        .map(|effect| {
            let (start_seconds, duration_seconds, lane_index) = update(session, effect);
            EffectUpdate {
                id: effect.id.0,
                start_seconds,
                duration_seconds,
                lane_index,
            }
        })
        .collect())
}

pub(super) fn apply_effect_updates(
    session: &mut ProjectSession,
    sequence_id: &SequenceId,
    updates: Vec<EffectUpdate>,
) -> Result<Vec<u32>, GuiMutationError> {
    let targets = updates
        .iter()
        .map(|update| (update.id, target_for_lane(session, update.lane_index)))
        .collect::<Vec<_>>();
    let sequence = sequence_mut(session, sequence_id)?;
    let mut moved = Vec::new();
    for update in updates {
        let effect = effect_mut(sequence, update.id)?;
        effect.start = dawn_time(update.start_seconds.max(0.0));
        effect.duration = dawn_duration(update.duration_seconds.max(0.000000001));
        if let Some((_, Some(target))) = targets.iter().find(|(id, _)| *id == update.id) {
            effect.target = target.clone();
        }
        moved.push(update.id);
    }
    Ok(moved)
}

pub(super) fn mark_time_seconds(
    sequence: &dawn_language::sequence::Sequence,
    mark: &SequenceMarkRef,
) -> Option<f64> {
    sequence
        .mark_collections
        .iter()
        .find(|collection| collection.key.name == mark.collection_key)?
        .marks
        .get(mark.index as usize)
        .map(DawnTime::as_seconds_f64)
}

pub(super) fn mark_indexes_by_collection(
    marks: &[SequenceMarkRef],
) -> BTreeMap<String, Vec<usize>> {
    let mut grouped = BTreeMap::<String, Vec<usize>>::new();
    for mark in marks {
        grouped
            .entry(mark.collection_key.clone())
            .or_default()
            .push(mark.index as usize);
    }
    for indexes in grouped.values_mut() {
        indexes.sort_unstable();
        indexes.dedup();
    }
    grouped
}

pub(super) fn effect_lane_index(session: &ProjectSession, target: &EffectTarget) -> usize {
    effect_lane_index_resolved(session, target).unwrap_or_default()
}

pub(super) fn effect_lane_index_resolved(
    session: &ProjectSession,
    target: &EffectTarget,
) -> Option<usize> {
    let layout_id = active_layout_id(session)?;
    let layout = session.project.layouts.get(&layout_id)?;
    layout
        .target_order
        .iter()
        .position(|candidate| effect_target_matches_layout(target, candidate))
}

pub(super) fn effect_target_matches_layout(
    target: &EffectTarget,
    candidate: &DomainLayoutTarget,
) -> bool {
    matches!(
        (target, candidate),
        (EffectTarget::Fixture(left), DomainLayoutTarget::Fixture(right)) if left == right
    ) || matches!(
        (target, candidate),
        (EffectTarget::Group(left), DomainLayoutTarget::Group(right)) if left == right
    )
}

pub(super) fn sequence_lane_count(session: &ProjectSession) -> usize {
    active_layout_id(session)
        .and_then(|layout_id| session.project.layouts.get(&layout_id))
        .map(|layout| layout.target_order.len())
        .unwrap_or_default()
}

pub(super) fn target_for_lane(session: &ProjectSession, lane_index: usize) -> Option<EffectTarget> {
    let layout_id = active_layout_id(session)?;
    let layout = session.project.layouts.get(&layout_id)?;
    layout
        .target_order
        .get(lane_index)
        .map(effect_target_from_layout)
}

pub(super) fn effect_target_from_layout(target: &DomainLayoutTarget) -> EffectTarget {
    match target {
        DomainLayoutTarget::Fixture(id) => EffectTarget::Fixture(FixtureInstanceId(id.0)),
        DomainLayoutTarget::Group(id) => EffectTarget::Group(FixtureGroupId(id.0)),
    }
}

pub(super) fn shifted_lane(lane_index: usize, lane_delta: i32, lane_count: usize) -> usize {
    if lane_count == 0 {
        return 0;
    }
    (lane_index as i32 + lane_delta).clamp(0, lane_count.saturating_sub(1) as i32) as usize
}

pub(super) fn anchored_lane(
    anchor_lane: usize,
    lane_index: usize,
    min_lane: usize,
    lane_count: usize,
) -> usize {
    if lane_count == 0 {
        return lane_index;
    }
    (anchor_lane + lane_index.saturating_sub(min_lane)).min(lane_count.saturating_sub(1))
}

pub(super) fn mark_param_names(
    session: &ProjectSession,
    script: &EffectScriptReference,
) -> Result<Vec<String>, GuiMutationError> {
    let id = EffectDefinitionId(SourceIdentity::new(
        Utf8PathBuf::from(&script.path),
        script.effect_name.clone(),
    ));
    let definition = session
        .project
        .definitions
        .effects
        .get(&id)
        .ok_or_else(|| GuiMutationError::Invalid("Effect script was not found.".to_string()))?;
    Ok(definition
        .compiled
        .params()
        .iter()
        .filter(|param| matches!(param.ty, Type::Marks))
        .map(|param| param.name.as_str().to_string())
        .collect())
}
