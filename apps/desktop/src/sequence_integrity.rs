use std::collections::{BTreeSet, HashSet};

use dawn_language::dsl::Type;
use dawn_language::effect::{CurveSource, EffectParamValue, EffectTarget};
use dawn_language::operator::effect_param_matches_type;
use dawn_language::sequence::{
    AutomationMapping, AutomationTarget, CompositionGraphNodeKind, MarkCollectionKey, SequenceId,
};
use dawn_language::setup::LayoutTarget;
use dawn_project_io::ProjectSession;

use crate::gui::GuiMutationError;

pub(crate) fn validate_sequence_integrity(
    session: &ProjectSession,
    id: &SequenceId,
) -> Result<(), GuiMutationError> {
    let sequence = session
        .project
        .sequences
        .get(id)
        .ok_or_else(|| GuiMutationError::Invalid("Sequence was not found.".to_string()))?;
    let unique = |values: Vec<String>, label: &str| {
        let count = values.len();
        let unique = values.into_iter().collect::<BTreeSet<_>>().len();
        (count == unique)
            .then_some(())
            .ok_or_else(|| GuiMutationError::Invalid(format!("Sequence {label} must be unique.")))
    };
    unique(
        sequence
            .layers
            .iter()
            .map(|layer| layer.id.0.to_string())
            .collect(),
        "layer ids",
    )?;
    unique(
        sequence
            .effects
            .iter()
            .map(|effect| effect.id.0.to_string())
            .collect(),
        "effect ids",
    )?;
    unique(
        sequence
            .mark_collections
            .iter()
            .map(|collection| collection.key.name.clone())
            .collect(),
        "mark collection keys",
    )?;
    unique(
        sequence
            .automation_clips
            .iter()
            .map(|clip| clip.id.0.to_string())
            .collect(),
        "automation clip ids",
    )?;
    let layer_ids = sequence
        .layers
        .iter()
        .map(|layer| layer.id.clone())
        .collect::<HashSet<_>>();
    let mark_keys = sequence
        .mark_collections
        .iter()
        .map(|collection| collection.key.clone())
        .collect::<HashSet<_>>();
    let layout = session
        .project
        .setups
        .get(&session.project.root.setup)
        .and_then(|setup| session.project.layouts.get(&setup.layout));
    for effect in &sequence.effects {
        if !layer_ids.contains(&effect.layer_id) {
            return Err(GuiMutationError::Invalid(format!(
                "Effect {} references a missing layer.",
                effect.id.0
            )));
        }
        if !layout
            .ok_or_else(|| GuiMutationError::Invalid("Active layout is missing.".to_string()))?
            .target_order
            .iter()
            .any(|target| effect_target_matches_layout(&effect.target, target))
        {
            return Err(GuiMutationError::Invalid(format!(
                "Effect {} references a missing layout target.",
                effect.id.0
            )));
        }
        let definition = session
            .project
            .definitions
            .effects
            .get(&effect.definition)
            .ok_or_else(|| {
                GuiMutationError::Invalid("Effect definition is missing.".to_string())
            })?;
        for declaration in definition.compiled.params() {
            match effect.param_overrides.get(&declaration.name) {
                Some(value) if effect_param_matches_type(value, &declaration.ty) => {
                    validate_param_references(session, value, &mark_keys)?;
                }
                Some(_) => {
                    return Err(GuiMutationError::Invalid(format!(
                        "Effect {} parameter `{}` has the wrong type.",
                        effect.id.0,
                        declaration.name.as_str()
                    )));
                }
                None if declaration.default.is_none() => {
                    return Err(GuiMutationError::Invalid(format!(
                        "Effect {} is missing required parameter `{}`.",
                        effect.id.0,
                        declaration.name.as_str()
                    )));
                }
                None => {}
            }
        }
        if effect.param_overrides.keys().any(|name| {
            !definition
                .compiled
                .params()
                .iter()
                .any(|param| &param.name == name)
        }) {
            return Err(GuiMutationError::Invalid(format!(
                "Effect {} contains an undeclared parameter.",
                effect.id.0
            )));
        }
    }
    for clip in &sequence.automation_clips {
        if !clip.start.as_seconds_f64().is_finite()
            || !clip.duration.as_seconds_f64().is_finite()
            || clip.duration.as_seconds_f64() <= 0.0
        {
            return Err(GuiMutationError::Invalid(
                "Automation clip timing must be finite and positive.".to_string(),
            ));
        }
        for binding in &clip.bindings {
            let ty = match &binding.target {
                AutomationTarget::EffectParam { effect_id, param } => {
                    let effect = sequence
                        .effects
                        .iter()
                        .find(|effect| &effect.id == effect_id)
                        .ok_or_else(|| {
                            GuiMutationError::Invalid("Automation effect is missing.".to_string())
                        })?;
                    session
                        .project
                        .definitions
                        .effects
                        .get(&effect.definition)
                        .and_then(|definition| {
                            definition
                                .compiled
                                .params()
                                .iter()
                                .find(|declaration| &declaration.name == param)
                        })
                        .map(|declaration| &declaration.ty)
                        .ok_or_else(|| {
                            GuiMutationError::Invalid(
                                "Automation parameter is missing.".to_string(),
                            )
                        })?
                }
                AutomationTarget::CompositionNodeParam { node_id, param } => {
                    let operator = sequence
                        .composition_graph
                        .nodes
                        .iter()
                        .find(|node| &node.id == node_id)
                        .and_then(|node| match &node.kind {
                            CompositionGraphNodeKind::Operator(operator) => Some(operator),
                            _ => None,
                        })
                        .ok_or_else(|| {
                            GuiMutationError::Invalid(
                                "Automation graph node is missing.".to_string(),
                            )
                        })?;
                    session
                        .project
                        .definitions
                        .operators
                        .resolve(&operator.operator)
                        .and_then(|definition| {
                            definition
                                .params
                                .iter()
                                .find(|declaration| &declaration.name == param)
                        })
                        .map(|declaration| &declaration.ty)
                        .ok_or_else(|| {
                            GuiMutationError::Invalid(
                                "Automation parameter is missing.".to_string(),
                            )
                        })?
                }
            };
            if !automation_mapping_matches_type(&binding.mapping, ty) {
                return Err(GuiMutationError::Invalid(
                    "Automation mapping does not match its target parameter.".to_string(),
                ));
            }
        }
    }
    Ok(())
}

fn validate_param_references(
    session: &ProjectSession,
    value: &EffectParamValue,
    mark_keys: &HashSet<MarkCollectionKey>,
) -> Result<(), GuiMutationError> {
    match value {
        EffectParamValue::Marks(key) if !mark_keys.contains(key) => Err(GuiMutationError::Invalid(
            "Effect parameter references a missing mark collection.".to_string(),
        )),
        EffectParamValue::Curve(CurveSource::Reference(id))
            if !session
                .project
                .definitions
                .curves
                .definitions
                .contains_key(id) =>
        {
            Err(GuiMutationError::Invalid(
                "Effect parameter references a missing curve.".to_string(),
            ))
        }
        EffectParamValue::Array(values) => {
            for value in values {
                validate_param_references(session, value, mark_keys)?;
            }
            Ok(())
        }
        _ => Ok(()),
    }
}

fn automation_mapping_matches_type(mapping: &AutomationMapping, ty: &Type) -> bool {
    match (mapping, ty) {
        (AutomationMapping::Float { min, max }, Type::Float) => {
            min.is_finite() && max.is_finite() && min <= max
        }
        (AutomationMapping::Int { min, max }, Type::Int) => min <= max,
        (AutomationMapping::Bool, Type::Bool) => true,
        (AutomationMapping::Enum { values }, Type::Enum(options)) => {
            !values.is_empty() && values.iter().all(|value| options.contains(value))
        }
        (AutomationMapping::FloatCurve { min, max }, Type::Curve(inner)) => {
            matches!(inner.as_ref(), Type::Float)
                && min.is_finite()
                && max.is_finite()
                && min <= max
        }
        _ => false,
    }
}

fn effect_target_matches_layout(target: &EffectTarget, candidate: &LayoutTarget) -> bool {
    matches!(
        (target, candidate),
        (EffectTarget::Fixture(left), LayoutTarget::Fixture(right)) if left == right
    ) || matches!(
        (target, candidate),
        (EffectTarget::Group(left), LayoutTarget::Group(right)) if left == right
    )
}
