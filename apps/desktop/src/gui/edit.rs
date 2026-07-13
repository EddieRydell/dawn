pub(super) fn edit_layout(
    session: &mut ProjectSession,
    resolved: &ResolvedGuiObject,
    edit: LayoutGuiEdit,
) -> Result<(), GuiMutationError> {
    let layout_id = LayoutId(SourceIdentity::new(
        resolved.identity.document().to_path_buf(),
        resolved.identity.object().to_string(),
    ));
    let layout = session
        .project
        .layouts
        .get_mut(&layout_id)
        .ok_or_else(|| GuiMutationError::Invalid("Layout was not found.".to_string()))?;
    match edit {
        LayoutGuiEdit::UpdatePlacementTransform { id, transform } => {
            let fixture = layout
                .fixtures
                .iter_mut()
                .find(|fixture| fixture.id.0 == id)
                .ok_or_else(|| {
                    GuiMutationError::Invalid("Fixture placement was not found.".to_string())
                })?;
            fixture.position = domain_point3_meters(transform.position);
            fixture.rotation = rotation3_degrees(transform.rotation);
            fixture.scale = scale3(transform.scale);
            Ok(())
        }
    }
}

pub(super) fn edit_fixture(
    session: &mut ProjectSession,
    identity: &SourceIdentity,
    edit: FixtureGuiEdit,
) -> Result<(), GuiMutationError> {
    match edit {
        FixtureGuiEdit::UpdateBulbDiameter {
            object_key,
            bulb_diameter_meters,
        } => {
            let definition = fixture_definition_mut(session, identity.document(), &object_key)?;
            definition.bulb_radius = DistanceSpan::from_meters(bulb_diameter_meters / 2.0);
            Ok(())
        }
        FixtureGuiEdit::MovePoint {
            object_key,
            point_index,
            point,
        } => {
            let definition = fixture_definition_mut(session, identity.document(), &object_key)?;
            let DomainGeometry::Points { points } = &mut definition.geometry else {
                return Err(GuiMutationError::Invalid(
                    "Fixture geometry does not contain movable points.".to_string(),
                ));
            };
            let target = points.get_mut(point_index as usize).ok_or_else(|| {
                GuiMutationError::Invalid("Fixture point was not found.".to_string())
            })?;
            *target = domain_point3_meters(point);
            Ok(())
        }
    }
}

pub(super) fn edit_sequence(
    session: &mut ProjectSession,
    resolved: &ResolvedGuiObject,
    edit: SequenceGuiEdit,
) -> Result<(), GuiMutationError> {
    let add_effect_mark_params = match &edit {
        SequenceGuiEdit::AddEffect {
            effect: effect_reference,
            mark_collection_key: Some(_),
            ..
        } => mark_param_names(session, effect_reference)?,
        _ => Vec::new(),
    };
    let unlink_curve_value = match &edit {
        SequenceGuiEdit::UnlinkEffectCurve { id, name } => {
            Some(current_effect_curve_value(session, resolved, *id, name)?)
        }
        SequenceGuiEdit::UnlinkGraphOperatorCurve { node_id, name } => {
            Some(current_graph_curve_value(session, resolved, node_id, name)?)
        }
        _ => None,
    };
    let unlink_gradient_value = match &edit {
        SequenceGuiEdit::UnlinkEffectGradient { id, name } => {
            Some(current_effect_gradient_value(session, resolved, *id, name)?)
        }
        SequenceGuiEdit::UnlinkGraphOperatorGradient { node_id, name } => Some(
            current_graph_gradient_value(session, resolved, node_id, name)?,
        ),
        _ => None,
    };
    let sequence_id = SequenceId(SourceIdentity::new(
        resolved.identity.document().to_path_buf(),
        resolved.identity.object().to_string(),
    ));
    match edit {
        SequenceGuiEdit::SetDuration { duration_seconds } => {
            if !duration_seconds.is_finite() || duration_seconds <= 0.0 {
                return Err(GuiMutationError::Invalid(
                    "Sequence duration must be greater than zero.".to_string(),
                ));
            }
            sequence_mut(session, &sequence_id)?.duration =
                DawnDuration::from_seconds_f64(duration_seconds);
        }
        SequenceGuiEdit::SetAudio { import_path } => {
            let audio = match import_path {
                Some(import_path) => {
                    let document = resolved.identity.document().to_path_buf();
                    let id = register_sequence_audio_asset(session, &document, &import_path)?;
                    DomainSequenceAudio::Asset(id)
                }
                None => DomainSequenceAudio::None,
            };
            sequence_mut(session, &sequence_id)?.audio = audio;
        }
        SequenceGuiEdit::MoveEffect {
            id,
            start_seconds,
            target,
        } => {
            let parsed_target = target.map(layout_target_to_effect_target).transpose()?;
            let sequence = sequence_mut(session, &sequence_id)?;
            let effect = effect_mut(sequence, id)?;
            effect.start = DawnTime::from_seconds_f64(start_seconds.max(0.0));
            if let Some(target) = parsed_target {
                effect.target = target;
            }
        }
        SequenceGuiEdit::ResizeEffect {
            id,
            start_seconds,
            duration_seconds,
        } => {
            let sequence = sequence_mut(session, &sequence_id)?;
            let start = DawnTime::from_seconds_f64(start_seconds.max(0.0));
            let duration = DawnDuration::from_seconds_f64(duration_seconds.max(0.000000001));
            let effect = effect_mut(sequence, id)?;
            effect.start = start;
            effect.duration = duration;
        }
        SequenceGuiEdit::SetEffectScope { id, scope } => {
            let sequence = sequence_mut(session, &sequence_id)?;
            let scope = effect_scope(scope);
            effect_mut(sequence, id)?.scope = scope;
        }
        SequenceGuiEdit::RetargetEffect { id, target } => {
            let sequence = sequence_mut(session, &sequence_id)?;
            let target = layout_target_to_effect_target(target)?;
            effect_mut(sequence, id)?.target = target;
        }
        SequenceGuiEdit::DeleteEffect { id } => {
            let sequence = sequence_mut(session, &sequence_id)?;
            sequence.effects.retain(|effect| effect.id.0 != id);
            for clip in &mut sequence.automation_clips {
                clip.bindings.retain(|binding| {
                    binding
                        .effect_param()
                        .is_none_or(|(effect_id, _)| effect_id.0 != id)
                });
            }
            sequence
                .automation_clips
                .retain(|clip| !clip.bindings.is_empty());
        }
        SequenceGuiEdit::MoveMark {
            collection_key,
            index,
            time_seconds,
        } => {
            let collection =
                mark_collection_mut(sequence_mut(session, &sequence_id)?, &collection_key)?;
            let mark = collection
                .marks
                .get_mut(index as usize)
                .ok_or_else(|| GuiMutationError::Invalid("Mark was not found.".to_string()))?;
            *mark = DawnTime::from_seconds_f64(time_seconds.max(0.0));
            collection.marks.sort_by_key(|time| time.0);
        }
        SequenceGuiEdit::ReassignMarkCollection {
            collection_key,
            index,
            target_collection_key,
        } => {
            if collection_key != target_collection_key {
                let sequence = sequence_mut(session, &sequence_id)?;
                let mark = {
                    let collection = mark_collection_mut(sequence, &collection_key)?;
                    if (index as usize) >= collection.marks.len() {
                        return Err(GuiMutationError::Invalid("Mark was not found.".to_string()));
                    }
                    collection.marks.remove(index as usize)
                };
                let target_collection = mark_collection_mut(sequence, &target_collection_key)?;
                target_collection.marks.push(mark);
                target_collection.marks.sort_by_key(|time| time.0);
            }
        }
        SequenceGuiEdit::AddMark {
            collection_key,
            time_seconds,
        } => {
            let collection =
                mark_collection_mut(sequence_mut(session, &sequence_id)?, &collection_key)?;
            collection
                .marks
                .push(DawnTime::from_seconds_f64(time_seconds.max(0.0)));
            collection.marks.sort_by_key(|time| time.0);
        }
        SequenceGuiEdit::DeleteMark {
            collection_key,
            index,
        } => {
            let collection =
                mark_collection_mut(sequence_mut(session, &sequence_id)?, &collection_key)?;
            if (index as usize) < collection.marks.len() {
                collection.marks.remove(index as usize);
            }
        }
        SequenceGuiEdit::CreateMarkCollection { key, name, color } => {
            let sequence = sequence_mut(session, &sequence_id)?;
            if sequence
                .mark_collections
                .iter()
                .any(|collection| collection.key.name == key)
            {
                return Err(GuiMutationError::Invalid(
                    "Mark collection keys must be unique.".to_string(),
                ));
            }
            sequence.mark_collections.push(MarkCollection {
                key: MarkCollectionKey { name: key },
                name,
                display_color: parse_color(&color)?,
                marks: Vec::new(),
            });
        }
        SequenceGuiEdit::RenameMarkCollection { key, name } => {
            mark_collection_mut(sequence_mut(session, &sequence_id)?, &key)?.name = name;
        }
        SequenceGuiEdit::DeleteMarkCollection { key } => {
            let sequence = sequence_mut(session, &sequence_id)?;
            let is_referenced = sequence.effects.iter().any(|effect| {
                effect.param_overrides.values().any(|value| {
                    matches!(value, EffectParamValue::Marks(collection) if collection.name == key)
                })
            });
            if is_referenced {
                return Err(GuiMutationError::Invalid(
                    "Mark collection is still referenced by an effect.".to_string(),
                ));
            }
            sequence
                .mark_collections
                .retain(|collection| collection.key.name != key);
        }
        SequenceGuiEdit::SetMarkCollectionColor { key, color } => {
            mark_collection_mut(sequence_mut(session, &sequence_id)?, &key)?.display_color =
                parse_color(&color)?;
        }
        SequenceGuiEdit::UpdateEffectParam { id, name, value } => {
            let value = effect_param_value_from_gui(value)?;
            effect_mut(sequence_mut(session, &sequence_id)?, id)?
                .param_overrides
                .insert(identifier(&name)?, value);
        }
        SequenceGuiEdit::AddEffect {
            effect: effect_reference,
            target,
            scope,
            start_seconds,
            mark_collection_key,
        } => {
            let definition = effect_ref_from_gui(effect_reference)?;
            let Some(effect_definition) = session.project.definitions.effects.resolve(&definition)
            else {
                return Err(GuiMutationError::Invalid(
                    "Effect was not found.".to_string(),
                ));
            };
            let params = effect_definition.params.clone();
            if let EffectRef::Custom(definition) = &definition {
                ensure_document_can_reference_source(
                    session,
                    resolved.identity.document(),
                    SourceObjectKind::EffectDefinition,
                    &definition.0,
                )
                .map_err(|error| GuiMutationError::Blocked(error.to_string()))?;
            }
            let sequence = sequence_mut(session, &sequence_id)?;
            let layer_id = sequence
                .layers
                .first()
                .map(|layer| layer.id.clone())
                .ok_or_else(|| {
                    GuiMutationError::Invalid(
                        "An effect cannot be added to a sequence without a layer.".to_string(),
                    )
                })?;
            let next_id = sequence
                .effects
                .iter()
                .map(|effect| effect.id.0)
                .max()
                .unwrap_or(0)
                + 1;
            let mut param_overrides = IndexMap::new();
            if let Some(key) = mark_collection_key {
                for name in add_effect_mark_params {
                    param_overrides.insert(
                        identifier(&name)?,
                        EffectParamValue::Marks(MarkCollectionKey { name: key.clone() }),
                    );
                }
            }
            for param in params.iter().filter(|param| param.default.is_none()) {
                if param_overrides.contains_key(&param.name) {
                    continue;
                }
                let value = EffectParamValue::default_for_type(&param.ty).ok_or_else(|| {
                    GuiMutationError::Invalid(format!(
                        "Effect parameter `{}` requires an explicit value.",
                        param.name.as_str()
                    ))
                })?;
                param_overrides.insert(param.name.clone(), value);
            }
            sequence.effects.push(EffectInst {
                id: EffectInstId(next_id),
                layer_id,
                start: DawnTime::from_seconds_f64(start_seconds.max(0.0)),
                duration: DawnDuration::from_seconds_f64(1.0),
                target: layout_target_to_effect_target(target)?,
                scope: effect_scope(scope),
                definition,
                param_overrides,
            });
        }
        SequenceGuiEdit::CreateLayer { name, color } => {
            create_sequence_layer(session, &sequence_id, name, color, None, true)?;
        }
        SequenceGuiEdit::CreateLayerAt { name, color, x, y } => {
            create_sequence_layer(session, &sequence_id, name, color, Some((x, y)), false)?;
        }
        SequenceGuiEdit::RenameLayer { id, name } => {
            let layer = sequence_mut(session, &sequence_id)?
                .layers
                .iter_mut()
                .find(|layer| layer.id.0 == id)
                .ok_or_else(|| GuiMutationError::Invalid("Layer was not found.".to_string()))?;
            layer.name = name;
        }
        SequenceGuiEdit::SetLayerColor { id, color } => {
            let layer = sequence_mut(session, &sequence_id)?
                .layers
                .iter_mut()
                .find(|layer| layer.id.0 == id)
                .ok_or_else(|| GuiMutationError::Invalid("Layer was not found.".to_string()))?;
            layer.color = parse_color(&color)?;
        }
        SequenceGuiEdit::SetLayerEnabled { id, enabled } => {
            let layer = sequence_mut(session, &sequence_id)?
                .layers
                .iter_mut()
                .find(|layer| layer.id.0 == id)
                .ok_or_else(|| GuiMutationError::Invalid("Layer was not found.".to_string()))?;
            layer.enabled = enabled;
        }
        SequenceGuiEdit::DeleteLayer {
            id,
            migrate_to_layer_id,
        } => {
            if id == 0 {
                return Err(GuiMutationError::Invalid(
                    "Default layer cannot be deleted.".to_string(),
                ));
            }
            let sequence = sequence_mut(session, &sequence_id)?;
            if !sequence.layers.iter().any(|layer| layer.id.0 == id) {
                return Err(GuiMutationError::Invalid(
                    "Layer was not found.".to_string(),
                ));
            }
            if migrate_to_layer_id == id
                || !sequence
                    .layers
                    .iter()
                    .any(|layer| layer.id.0 == migrate_to_layer_id)
            {
                return Err(GuiMutationError::Invalid(
                    "Effect migration layer was not found.".to_string(),
                ));
            }
            for effect in &mut sequence.effects {
                if effect.layer_id.0 == id {
                    effect.layer_id = SequenceLayerId(migrate_to_layer_id);
                }
            }
            sequence.layers.retain(|layer| layer.id.0 != id);
            let layer_node_ids = sequence
                .composition_graph
                .nodes
                .iter()
                .filter_map(|node| match &node.kind {
                    CompositionGraphNodeKind::Layer { layer_id } if layer_id.0 == id => {
                        Some(node.id.clone())
                    }
                    _ => None,
                })
                .collect::<BTreeSet<_>>();
            sequence
                .composition_graph
                .nodes
                .retain(|node| !layer_node_ids.contains(&node.id));
            sequence.composition_graph.edges.retain(|edge| {
                !layer_node_ids.contains(&edge.from) && !layer_node_ids.contains(&edge.to)
            });
        }
        SequenceGuiEdit::SetEffectLayer { id, layer_id } => {
            let sequence = sequence_mut(session, &sequence_id)?;
            if !sequence.layers.iter().any(|layer| layer.id.0 == layer_id) {
                return Err(GuiMutationError::Invalid(
                    "Layer was not found.".to_string(),
                ));
            }
            let effect = sequence
                .effects
                .iter_mut()
                .find(|effect| effect.id.0 == id)
                .ok_or_else(|| GuiMutationError::Invalid("Effect was not found.".to_string()))?;
            effect.layer_id = SequenceLayerId(layer_id);
        }
        SequenceGuiEdit::ChangeEffectDefinition {
            id,
            effect: effect_reference,
        } => {
            let definition = effect_ref_from_gui(effect_reference)?;
            let Some(effect_definition) = session.project.definitions.effects.resolve(&definition)
            else {
                return Err(GuiMutationError::Invalid(
                    "Effect was not found.".to_string(),
                ));
            };
            let params = effect_definition.params.clone();
            let mut param_overrides = IndexMap::new();
            for param in params.iter().filter(|param| param.default.is_none()) {
                let value = EffectParamValue::default_for_type(&param.ty).ok_or_else(|| {
                    GuiMutationError::Invalid(format!(
                        "Effect parameter `{}` requires an explicit value before changing scripts.",
                        param.name.as_str()
                    ))
                })?;
                param_overrides.insert(param.name.clone(), value);
            }
            if let EffectRef::Custom(definition) = &definition {
                ensure_document_can_reference_source(
                    session,
                    resolved.identity.document(),
                    SourceObjectKind::EffectDefinition,
                    &definition.0,
                )
                .map_err(|error| GuiMutationError::Blocked(error.to_string()))?;
            }
            let sequence = sequence_mut(session, &sequence_id)?;
            let effect = effect_mut(sequence, id)?;
            effect.definition = definition;
            effect.param_overrides = param_overrides;
            for clip in &mut sequence.automation_clips {
                clip.bindings.retain(|binding| {
                    binding
                        .effect_param()
                        .is_none_or(|(effect_id, _)| effect_id.0 != id)
                });
            }
            sequence
                .automation_clips
                .retain(|clip| !clip.bindings.is_empty());
        }
        SequenceGuiEdit::LinkEffectCurve {
            id,
            name,
            source_path,
            object_key,
        } => {
            let value = linked_curve_value(session, resolved, source_path, object_key)?;
            effect_mut(sequence_mut(session, &sequence_id)?, id)?
                .param_overrides
                .insert(identifier(&name)?, value);
        }
        SequenceGuiEdit::UnlinkEffectCurve { id, name } => {
            let value = unlink_curve_value.ok_or_else(|| {
                GuiMutationError::Invalid("Curve param could not be resolved.".to_string())
            })?;
            effect_mut(sequence_mut(session, &sequence_id)?, id)?
                .param_overrides
                .insert(identifier(&name)?, effect_param_value_from_gui(value)?);
        }
        SequenceGuiEdit::LinkEffectGradient {
            id,
            name,
            source_path,
            object_key,
        } => {
            let value = linked_gradient_value(session, resolved, source_path, object_key)?;
            effect_mut(sequence_mut(session, &sequence_id)?, id)?
                .param_overrides
                .insert(identifier(&name)?, value);
        }
        SequenceGuiEdit::UnlinkEffectGradient { id, name } => {
            let value = unlink_gradient_value.ok_or_else(|| {
                GuiMutationError::Invalid("Gradient param could not be resolved.".to_string())
            })?;
            effect_mut(sequence_mut(session, &sequence_id)?, id)?
                .param_overrides
                .insert(identifier(&name)?, effect_param_value_from_gui(value)?);
        }
        SequenceGuiEdit::AddGraphOperatorNode { operator, x, y } => {
            let operator = graph_operator_from_gui(&operator)?;
            let definition = session
                .project
                .definitions
                .operators
                .resolve(&operator)
                .cloned()
                .ok_or_else(|| {
                    GuiMutationError::Invalid("Operator definition was not found.".to_string())
                })?;
            if let OperatorRef::Custom(id) = &operator {
                ensure_document_can_reference_source(
                    session,
                    resolved.identity.document(),
                    SourceObjectKind::OperatorDefinition,
                    &id.0,
                )
                .map_err(|error| GuiMutationError::Blocked(error.to_string()))?;
            }
            let sequence = sequence_mut(session, &sequence_id)?;
            let mut params = IndexMap::new();
            for declaration in &definition.params {
                if declaration.default.is_none() {
                    let value = required_operator_param_value(declaration.ty.clone(), sequence)?;
                    params.insert(declaration.name.clone(), value);
                }
            }
            let next_id = next_composition_node_id(sequence);
            sequence.composition_graph.nodes.push(CompositionGraphNode {
                id: CompositionGraphNodeId(next_id),
                position: GraphNodePosition { x, y },
                kind: CompositionGraphNodeKind::Operator(GraphOperatorNode { operator, params }),
            });
        }
        SequenceGuiEdit::MoveGraphNode { node_id, x, y } => {
            let sequence = sequence_mut(session, &sequence_id)?;
            let node_id = parse_graph_node_id(&node_id)?;
            let node = composition_graph_node_mut(sequence, &node_id)?;
            node.position = GraphNodePosition { x, y };
        }
        SequenceGuiEdit::DeleteGraphNode { node_id } => {
            let sequence = sequence_mut(session, &sequence_id)?;
            let node_id = parse_graph_node_id(&node_id)?;
            let node = sequence
                .composition_graph
                .nodes
                .iter()
                .find(|node| node.id == node_id)
                .ok_or_else(|| {
                    GuiMutationError::Invalid("Graph node was not found.".to_string())
                })?;
            match &node.kind {
                CompositionGraphNodeKind::Layer { .. } => {
                    return Err(GuiMutationError::Invalid(
                        "Delete the layer from the layer list.".to_string(),
                    ));
                }
                CompositionGraphNodeKind::Output => {
                    return Err(GuiMutationError::Invalid(
                        "Output node cannot be deleted.".to_string(),
                    ));
                }
                CompositionGraphNodeKind::Operator(_) => {}
            }
            sequence
                .composition_graph
                .nodes
                .retain(|node| node.id != node_id);
            sequence
                .composition_graph
                .edges
                .retain(|edge| edge.from != node_id && edge.to != node_id);
            for clip in &mut sequence.automation_clips {
                clip.bindings.retain(|binding| {
                    binding
                        .composition_node_param()
                        .is_none_or(|(binding_node_id, _)| binding_node_id != &node_id)
                });
            }
            sequence
                .automation_clips
                .retain(|clip| !clip.bindings.is_empty());
        }
        SequenceGuiEdit::ConnectGraphNodes {
            from_node,
            from_port,
            to_node,
            to_port,
        } => {
            if from_node == to_node {
                return Err(GuiMutationError::Invalid(
                    "Graph node cannot connect to itself.".to_string(),
                ));
            }
            let definitions = session.project.definitions.operators.clone();
            let sequence = sequence_mut(session, &sequence_id)?;
            let from = parse_graph_node_id(&from_node)?;
            let to = parse_graph_node_id(&to_node)?;
            ensure_graph_node_exists(sequence, &from)?;
            ensure_graph_node_exists(sequence, &to)?;
            if sequence.composition_graph.edges.iter().any(|edge| {
                edge.from == from
                    && edge.from_port.0 == from_port
                    && edge.to == to
                    && edge.to_port.0 == to_port
            }) {
                return Ok(());
            }
            let mut graph = sequence.composition_graph.clone();
            let single_input = graph
                .nodes
                .iter()
                .find(|node| node.id == to)
                .and_then(|node| graph_input_cardinality(&definitions, &node.kind, &to_port))
                == Some(OperatorPortCardinality::One);
            if single_input {
                graph
                    .edges
                    .retain(|edge| edge.to != to || edge.to_port.0 != to_port);
            }
            graph.edges.push(EffectGraphEdge {
                from,
                from_port: GraphPortId(from_port),
                to,
                to_port: GraphPortId(to_port),
            });
            validate_composition_graph(&graph, &definitions)
                .map_err(|error| GuiMutationError::Invalid(error.message))?;
            sequence.composition_graph = graph;
        }
        SequenceGuiEdit::DisconnectGraphNodes {
            from_node,
            from_port,
            to_node,
            to_port,
        } => {
            let sequence = sequence_mut(session, &sequence_id)?;
            let from = parse_graph_node_id(&from_node)?;
            let to = parse_graph_node_id(&to_node)?;
            sequence.composition_graph.edges.retain(|edge| {
                !(edge.from == from
                    && edge.from_port.0 == from_port
                    && edge.to == to
                    && edge.to_port.0 == to_port)
            });
        }
        SequenceGuiEdit::UpdateGraphOperatorParam {
            node_id,
            name,
            value,
        } => {
            let definitions = session.project.definitions.operators.clone();
            let sequence = sequence_mut(session, &sequence_id)?;
            let node_id = parse_graph_node_id(&node_id)?;
            let mut graph = sequence.composition_graph.clone();
            let node = graph
                .nodes
                .iter_mut()
                .find(|node| node.id == node_id)
                .ok_or_else(|| {
                    GuiMutationError::Invalid("Graph node was not found.".to_string())
                })?;
            let CompositionGraphNodeKind::Operator(operator) = &mut node.kind else {
                return Err(GuiMutationError::Invalid(
                    "Graph node is not an operator.".to_string(),
                ));
            };
            operator
                .params
                .insert(identifier(&name)?, effect_param_value_from_gui(value)?);
            validate_composition_graph(&graph, &definitions)
                .map_err(|error| GuiMutationError::Invalid(error.message))?;
            sequence.composition_graph = graph;
        }
        SequenceGuiEdit::LinkGraphOperatorCurve {
            node_id,
            name,
            source_path,
            object_key,
        } => {
            let value = linked_curve_value(session, resolved, source_path, object_key)?;
            let node_id = parse_graph_node_id(&node_id)?;
            let sequence = sequence_mut(session, &sequence_id)?;
            let node = composition_graph_node_mut(sequence, &node_id)?;
            let CompositionGraphNodeKind::Operator(operator) = &mut node.kind else {
                return Err(GuiMutationError::Invalid(
                    "Graph node is not an operator.".to_string(),
                ));
            };
            operator.params.insert(identifier(&name)?, value);
        }
        SequenceGuiEdit::UnlinkGraphOperatorCurve { node_id, name } => {
            let value = unlink_curve_value.ok_or_else(|| {
                GuiMutationError::Invalid("Curve param could not be resolved.".to_string())
            })?;
            let node_id = parse_graph_node_id(&node_id)?;
            let sequence = sequence_mut(session, &sequence_id)?;
            let node = composition_graph_node_mut(sequence, &node_id)?;
            let CompositionGraphNodeKind::Operator(operator) = &mut node.kind else {
                return Err(GuiMutationError::Invalid(
                    "Graph node is not an operator.".to_string(),
                ));
            };
            operator
                .params
                .insert(identifier(&name)?, effect_param_value_from_gui(value)?);
        }
        SequenceGuiEdit::LinkGraphOperatorGradient {
            node_id,
            name,
            source_path,
            object_key,
        } => {
            let value = linked_gradient_value(session, resolved, source_path, object_key)?;
            let node_id = parse_graph_node_id(&node_id)?;
            let sequence = sequence_mut(session, &sequence_id)?;
            let node = composition_graph_node_mut(sequence, &node_id)?;
            let CompositionGraphNodeKind::Operator(operator) = &mut node.kind else {
                return Err(GuiMutationError::Invalid(
                    "Graph node is not an operator.".to_string(),
                ));
            };
            operator.params.insert(identifier(&name)?, value);
        }
        SequenceGuiEdit::UnlinkGraphOperatorGradient { node_id, name } => {
            let value = unlink_gradient_value.ok_or_else(|| {
                GuiMutationError::Invalid("Gradient param could not be resolved.".to_string())
            })?;
            let node_id = parse_graph_node_id(&node_id)?;
            let sequence = sequence_mut(session, &sequence_id)?;
            let node = composition_graph_node_mut(sequence, &node_id)?;
            let CompositionGraphNodeKind::Operator(operator) = &mut node.kind else {
                return Err(GuiMutationError::Invalid(
                    "Graph node is not an operator.".to_string(),
                ));
            };
            operator
                .params
                .insert(identifier(&name)?, effect_param_value_from_gui(value)?);
        }
        SequenceGuiEdit::AddAutomationClip {
            start_seconds,
            duration_seconds,
            anchor_lane_index,
            lane_index,
        } => {
            let sequence = sequence_mut(session, &sequence_id)?;
            let next_id = sequence
                .automation_clips
                .iter()
                .map(|clip| clip.id.0)
                .max()
                .unwrap_or(0)
                + 1;
            sequence.automation_clips.push(AutomationClip {
                id: AutomationClipId(next_id),
                start: DawnTime::from_seconds_f64(start_seconds.max(0.0)),
                duration: DawnDuration::from_seconds_f64(duration_seconds.max(0.000000001)),
                anchor_lane_index,
                lane_index,
                curve: default_automation_curve(),
                bindings: Vec::new(),
            });
        }
        SequenceGuiEdit::CreateAndBindAutomationClip {
            effect_id,
            param,
            mapping,
        } => {
            let param = identifier(&param)?;
            let mapping = automation_mapping_from_gui(mapping)?;
            let (effect_start, effect_duration, anchor_lane_index) = {
                let sequence = session.project.sequences.get(&sequence_id).ok_or_else(|| {
                    GuiMutationError::Invalid("Sequence was not found.".to_string())
                })?;
                let effect = sequence
                    .effects
                    .iter()
                    .find(|effect| effect.id.0 == effect_id)
                    .ok_or_else(|| {
                        GuiMutationError::Invalid("Effect was not found.".to_string())
                    })?;
                let anchor_lane_index = effect_lane_index_resolved(session, &effect.target)
                    .ok_or_else(|| {
                        GuiMutationError::Invalid("Effect lane was not found.".to_string())
                    })?;
                (
                    effect.start.clone(),
                    effect.duration.clone(),
                    anchor_lane_index as u32,
                )
            };
            let sequence = sequence_mut(session, &sequence_id)?;
            for clip in &sequence.automation_clips {
                if clip.bindings.iter().any(|binding| {
                    binding
                        .effect_param()
                        .is_some_and(|(target_effect, target_param)| {
                            target_effect.0 == effect_id && target_param == &param
                        })
                }) {
                    return Err(GuiMutationError::Invalid(
                        "Param is already automated.".to_string(),
                    ));
                }
            }
            let next_id = sequence
                .automation_clips
                .iter()
                .map(|clip| clip.id.0)
                .max()
                .unwrap_or(0)
                + 1;
            sequence.automation_clips.push(AutomationClip {
                id: AutomationClipId(next_id),
                start: effect_start,
                duration: effect_duration,
                anchor_lane_index,
                lane_index: 0,
                curve: default_automation_curve(),
                bindings: vec![AutomationBinding {
                    target: AutomationTarget::EffectParam {
                        effect_id: EffectInstId(effect_id),
                        param,
                    },
                    mapping,
                }],
            });
        }
        SequenceGuiEdit::MoveAutomationClip {
            id,
            start_seconds,
            anchor_lane_index,
            lane_index,
        } => {
            let clip = automation_clip_mut(sequence_mut(session, &sequence_id)?, id)?;
            clip.start = DawnTime::from_seconds_f64(start_seconds.max(0.0));
            clip.anchor_lane_index = anchor_lane_index;
            clip.lane_index = lane_index;
        }
        SequenceGuiEdit::ResizeAutomationClip {
            id,
            start_seconds,
            duration_seconds,
        } => {
            let clip = automation_clip_mut(sequence_mut(session, &sequence_id)?, id)?;
            clip.start = DawnTime::from_seconds_f64(start_seconds.max(0.0));
            clip.duration = DawnDuration::from_seconds_f64(duration_seconds.max(0.000000001));
        }
        SequenceGuiEdit::UpdateAutomationCurve { id, curve } => {
            automation_clip_mut(sequence_mut(session, &sequence_id)?, id)?.curve =
                curve_from_points(curve);
        }
        SequenceGuiEdit::DeleteAutomationClip { id } => {
            sequence_mut(session, &sequence_id)?
                .automation_clips
                .retain(|clip| clip.id.0 != id);
        }
        SequenceGuiEdit::BindAutomationParam {
            clip_id,
            effect_id,
            param,
            mapping,
        } => {
            let param = identifier(&param)?;
            let sequence = sequence_mut(session, &sequence_id)?;
            for clip in &sequence.automation_clips {
                if clip.bindings.iter().any(|binding| {
                    binding
                        .effect_param()
                        .is_some_and(|(target_effect, target_param)| {
                            target_effect.0 == effect_id && target_param == &param
                        })
                }) {
                    return Err(GuiMutationError::Invalid(
                        "Param is already automated.".to_string(),
                    ));
                }
            }
            automation_clip_mut(sequence, clip_id)?
                .bindings
                .push(AutomationBinding {
                    target: AutomationTarget::EffectParam {
                        effect_id: EffectInstId(effect_id),
                        param,
                    },
                    mapping: automation_mapping_from_gui(mapping)?,
                });
        }
        SequenceGuiEdit::UnbindAutomationParam {
            clip_id,
            effect_id,
            param,
        } => {
            let param_id = identifier(&param)?;
            let sequence = sequence_mut(session, &sequence_id)?;
            let clip = sequence
                .automation_clips
                .iter()
                .find(|clip| clip.id.0 == clip_id)
                .cloned()
                .ok_or_else(|| {
                    GuiMutationError::Invalid("Automation clip was not found.".to_string())
                })?;
            let Some(binding) = clip
                .bindings
                .iter()
                .find(|binding| {
                    binding
                        .effect_param()
                        .is_some_and(|(target_effect, target_param)| {
                            target_effect.0 == effect_id && target_param == &param_id
                        })
                })
                .cloned()
            else {
                return Err(GuiMutationError::Invalid(
                    "Automation binding was not found.".to_string(),
                ));
            };
            let effect_start = sequence
                .effects
                .iter()
                .find(|effect| effect.id.0 == effect_id)
                .map(|effect| effect.start.as_seconds_f64())
                .ok_or_else(|| GuiMutationError::Invalid("Effect was not found.".to_string()))?;
            let value = automation_binding_value_at(&clip, &binding, effect_start)?;
            effect_mut(sequence, effect_id)?
                .param_overrides
                .insert(param_id.clone(), value);
            automation_clip_mut(sequence, clip_id)?
                .bindings
                .retain(|binding| {
                    !binding
                        .effect_param()
                        .is_some_and(|(target_effect, target_param)| {
                            target_effect.0 == effect_id && target_param == &param_id
                        })
                });
        }
    }
    Ok(())
}

fn linked_curve_value(
    session: &mut ProjectSession,
    resolved: &ResolvedGuiObject,
    source_path: String,
    object_key: String,
) -> Result<EffectParamValue, GuiMutationError> {
    let id = CurveId(SourceIdentity::new(
        Utf8PathBuf::from(source_path),
        object_key,
    ));
    if !session
        .project
        .definitions
        .curves
        .definitions
        .contains_key(&id)
    {
        return Err(GuiMutationError::Invalid(
            "Curve was not found.".to_string(),
        ));
    }
    ensure_document_can_reference_source(
        session,
        resolved.identity.document(),
        SourceObjectKind::Curve,
        &id.0,
    )
    .map_err(|error| GuiMutationError::Blocked(error.to_string()))?;
    Ok(EffectParamValue::Curve(CurveSource::Reference(id)))
}

fn linked_gradient_value(
    session: &mut ProjectSession,
    resolved: &ResolvedGuiObject,
    source_path: String,
    object_key: String,
) -> Result<EffectParamValue, GuiMutationError> {
    let id = GradientId(SourceIdentity::new(
        Utf8PathBuf::from(source_path),
        object_key,
    ));
    if !session
        .project
        .definitions
        .gradients
        .definitions
        .contains_key(&id)
    {
        return Err(GuiMutationError::Invalid(
            "Gradient was not found.".to_string(),
        ));
    }
    ensure_document_can_reference_source(
        session,
        resolved.identity.document(),
        SourceObjectKind::Gradient,
        &id.0,
    )
    .map_err(|error| GuiMutationError::Blocked(error.to_string()))?;
    Ok(EffectParamValue::Gradient(GradientSource::Reference(id)))
}
fn effect_ref_from_gui(reference: SequenceEffectReference) -> Result<EffectRef, GuiMutationError> {
    Ok(match reference {
        SequenceEffectReference::Builtin { effect } => EffectRef::Builtin(match effect {
            SequenceBuiltinEffect::Pulse => BuiltinEffect::Pulse,
            SequenceBuiltinEffect::Chase => BuiltinEffect::Chase,
            SequenceBuiltinEffect::Spin => BuiltinEffect::Spin,
            SequenceBuiltinEffect::MarkPulse => BuiltinEffect::MarkPulse,
            SequenceBuiltinEffect::MarkChase => BuiltinEffect::MarkChase,
        }),
        SequenceEffectReference::Custom { path, effect_name } => EffectRef::Custom(
            EffectDefinitionId(SourceIdentity::new(Utf8PathBuf::from(path), effect_name)),
        ),
    })
}

use std::collections::BTreeSet;

use camino::Utf8PathBuf;
use dawn_language::effect::{
    BuiltinEffect, CurveId, CurveSource, EffectDefinitionId, EffectInst, EffectInstId,
    EffectParamValue, EffectRef, GradientId, GradientSource,
};
use dawn_language::identity::SourceIdentity;
use dawn_language::operator::{
    GraphOperatorNode, OperatorPortCardinality, OperatorRef, validate_composition_graph,
};
use dawn_language::sequence::{
    AutomationBinding, AutomationClip, AutomationClipId, AutomationTarget, CompositionGraphNode,
    CompositionGraphNodeId, CompositionGraphNodeKind, EffectGraphEdge, GraphNodePosition,
    GraphPortId, MarkCollection, MarkCollectionKey, SequenceAudio as DomainSequenceAudio,
    SequenceId, SequenceLayerId,
};
use dawn_language::setup::{Geometry as DomainGeometry, LayoutId};
use dawn_language::values::{DawnDuration, DawnTime, DistanceSpan};
use dawn_project_io::{ProjectSession, SourceObjectKind, ensure_document_can_reference_source};
use indexmap::IndexMap;

use super::model::{
    automation_binding_value_at, automation_clip_mut, automation_mapping_from_gui,
    composition_graph_node_mut, create_sequence_layer, curve_from_points, default_automation_curve,
    domain_point3_meters, effect_mut, effect_param_value_from_gui, effect_scope,
    ensure_graph_node_exists, fixture_definition_mut, graph_input_cardinality,
    graph_operator_from_gui, identifier, layout_target_to_effect_target, mark_collection_mut,
    next_composition_node_id, parse_color, parse_graph_node_id, register_sequence_audio_asset,
    rotation3_degrees, scale3, sequence_mut,
};
use super::selection::{
    current_effect_curve_value, current_effect_gradient_value, current_graph_curve_value,
    current_graph_gradient_value, effect_lane_index_resolved, mark_param_names,
    required_operator_param_value,
};
use super::{GuiMutationError, ResolvedGuiObject};
use crate::dto::{
    FixtureGuiEdit, LayoutGuiEdit, SequenceBuiltinEffect, SequenceEffectReference, SequenceGuiEdit,
};
