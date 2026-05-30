use std::collections::HashMap;
use std::error::Error;
use std::fmt;

use indexmap::IndexMap;

use crate::model::*;
use crate::path::{resolve_import_path, Utf8PathBuf};

#[derive(Debug, Clone)]
pub struct ResolvedImport {
    pub source_path: Utf8PathBuf,
    pub object: DawnObject<Authored>,
}

#[derive(Debug, Clone)]
pub enum LowerError {
    MissingProject {
        key: String,
    },
    WrongObjectKind {
        key: String,
        expected: ObjectKind,
        actual: ObjectKind,
    },
    WrongImportedObjectKind {
        import: String,
        expected: ObjectKind,
        actual: ObjectKind,
    },
    Import {
        import: String,
        message: String,
    },
    DuplicateFixtureId {
        id: FixtureId,
    },
    EmptyFixtureName,
    DuplicateFixtureName {
        name: String,
    },
    UnknownFixture {
        id: FixtureId,
    },
    DuplicateControllerName {
        name: String,
    },
    UnknownController {
        name: String,
    },
    DuplicateGroupName {
        name: String,
    },
    UnknownGroup {
        name: String,
    },
    DuplicateLayoutTargetOrderEntry {
        kind: LayoutTargetKind,
        name: String,
    },
    MissingLayoutTargetOrderEntry {
        kind: LayoutTargetKind,
        name: String,
    },
    UnknownLayoutTargetOrderEntry {
        kind: LayoutTargetKind,
        name: String,
    },
    DuplicateSequenceEffectId {
        id: u32,
    },
    UnknownSequenceEffect {
        id: u32,
    },
    AutomationCurveType {
        id: u32,
        actual: CurveValueType,
    },
}

impl fmt::Display for LowerError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingProject { key } => {
                write!(formatter, "project object `{key}` was not found")
            }
            Self::WrongObjectKind {
                key,
                expected,
                actual,
            } => write!(
                formatter,
                "object `{key}` must be a {expected}, but found a {actual}"
            ),
            Self::WrongImportedObjectKind {
                import,
                expected,
                actual,
            } => write!(
                formatter,
                "import `{import}` must resolve to a {expected}, but found a {actual}"
            ),
            Self::Import { import, message } => {
                write!(formatter, "failed to resolve import `{import}`: {message}")
            }
            Self::DuplicateFixtureId { id } => write!(formatter, "duplicate fixture id `{id}`"),
            Self::EmptyFixtureName => write!(formatter, "fixture name cannot be empty"),
            Self::DuplicateFixtureName { name } => {
                write!(formatter, "duplicate fixture name `{name}`")
            }
            Self::UnknownFixture { id } => write!(formatter, "unknown fixture `{id}`"),
            Self::DuplicateControllerName { name } => {
                write!(formatter, "duplicate controller `{name}`")
            }
            Self::UnknownController { name } => write!(formatter, "unknown controller `{name}`"),
            Self::DuplicateGroupName { name } => write!(formatter, "duplicate group `{name}`"),
            Self::UnknownGroup { name } => write!(formatter, "unknown group `{name}`"),
            Self::DuplicateLayoutTargetOrderEntry { kind, name } => {
                write!(
                    formatter,
                    "duplicate layout target order entry `{:?}:{name}`",
                    kind
                )
            }
            Self::MissingLayoutTargetOrderEntry { kind, name } => {
                write!(
                    formatter,
                    "missing layout target order entry `{:?}:{name}`",
                    kind
                )
            }
            Self::UnknownLayoutTargetOrderEntry { kind, name } => {
                write!(
                    formatter,
                    "unknown layout target order entry `{:?}:{name}`",
                    kind
                )
            }
            Self::DuplicateSequenceEffectId { id } => {
                write!(formatter, "duplicate sequence effect `{id}`")
            }
            Self::UnknownSequenceEffect { id } => {
                write!(formatter, "unknown sequence effect `{id}`")
            }
            Self::AutomationCurveType { id, actual } => {
                write!(
                    formatter,
                    "automation clip `{id}` requires a float curve, but found a {:?} curve",
                    actual
                )
            }
        }
    }
}

impl Error for LowerError {}

pub fn lower_project(
    file: &DawnFile,
    project_key: &str,
    source_path: &Utf8PathBuf,
    mut resolver: impl FnMut(&Utf8PathBuf, &ImportRef, ObjectKind) -> Result<ResolvedImport, LowerError>,
) -> Result<ResolvedProject, LowerError> {
    let object = file
        .get(project_key)
        .ok_or_else(|| LowerError::MissingProject {
            key: project_key.to_string(),
        })?;
    let DawnObject::Project(project) = object else {
        return Err(LowerError::WrongObjectKind {
            key: project_key.to_string(),
            expected: ObjectKind::Project,
            actual: object.kind(),
        });
    };
    lower_project_object(project, source_path, &mut resolver)
}

fn lower_project_object(
    project: &Project<Authored>,
    source_path: &Utf8PathBuf,
    resolver: &mut impl FnMut(
        &Utf8PathBuf,
        &ImportRef,
        ObjectKind,
    ) -> Result<ResolvedImport, LowerError>,
) -> Result<Project<Resolved>, LowerError> {
    let (display, display_source) =
        resolve_display(&project.display, source_path, ObjectKind::Display, resolver)?;
    let (layout, layout_source) = resolve_layout(
        &display.layout,
        &display_source,
        ObjectKind::Layout,
        resolver,
    )?;

    let mut sequences = Vec::with_capacity(project.sequences.len());
    for sequence in &project.sequences {
        let (sequence, sequence_source) =
            resolve_sequence(sequence, source_path, ObjectKind::Sequence, resolver)?;
        sequences.push(lower_sequence(
            &sequence,
            &sequence_source,
            &layout,
            &layout_source,
            resolver,
        )?);
    }

    Ok(Project {
        name: project.name.clone(),
        display: lower_display(&display, &display_source, resolver)?,
        sequences,
    })
}

fn lower_display(
    display: &Display<Authored>,
    source_path: &Utf8PathBuf,
    resolver: &mut impl FnMut(
        &Utf8PathBuf,
        &ImportRef,
        ObjectKind,
    ) -> Result<ResolvedImport, LowerError>,
) -> Result<Display<Resolved>, LowerError> {
    let mut controllers = Vec::with_capacity(display.controllers.len());
    for controller in &display.controllers {
        let (controller, _) = resolve_controller(controller, source_path, resolver)?;
        controllers.push(controller);
    }
    let controller_indices = controller_indices(&controllers)?;

    let (layout, layout_source) =
        resolve_layout(&display.layout, source_path, ObjectKind::Layout, resolver)?;
    let layout = lower_layout(&layout, &layout_source, resolver)?;
    let fixture_indices = fixture_indices(&layout.fixtures)?;

    let (patch, _) = resolve_patch(&display.patch, source_path, ObjectKind::Patch, resolver)?;
    let patch = lower_patch(&patch, &fixture_indices, &controller_indices)?;

    Ok(Display {
        name: display.name.clone(),
        controllers,
        patch,
        layout,
    })
}

pub(crate) fn lower_layout(
    layout: &Layout<Authored>,
    source_path: &Utf8PathBuf,
    resolver: &mut impl FnMut(
        &Utf8PathBuf,
        &ImportRef,
        ObjectKind,
    ) -> Result<ResolvedImport, LowerError>,
) -> Result<Layout<Resolved>, LowerError> {
    let mut fixtures = Vec::with_capacity(layout.fixtures.len());
    for placement in &layout.fixtures {
        let (fixture, _) = resolve_fixture(&placement.fixture, source_path, resolver)?;
        fixtures.push(FixturePlacement {
            id: placement.id,
            name: placement.name.clone(),
            fixture,
            transform: placement.transform,
        });
    }

    let fixture_indices = fixture_indices(&fixtures)?;
    let mut groups = Vec::with_capacity(layout.groups.len());
    let mut group_names = HashMap::with_capacity(layout.groups.len());
    for (group_index, group) in layout.groups.iter().enumerate() {
        if group_names
            .insert(group.name.clone(), GroupIndex(group_index))
            .is_some()
        {
            return Err(LowerError::DuplicateGroupName {
                name: group.name.clone(),
            });
        }

        let mut members = Vec::with_capacity(group.members.len());
        for member in &group.members {
            let Some(index) = fixture_indices.get(member).copied() else {
                return Err(LowerError::UnknownFixture { id: *member });
            };
            members.push(index);
        }
        groups.push(Group {
            name: group.name.clone(),
            members,
        });
    }

    Ok(Layout {
        name: layout.name.clone(),
        units: layout.units,
        target_order: validate_layout_target_order(layout, &fixtures, &groups)?,
        fixtures,
        groups,
    })
}

fn validate_layout_target_order(
    layout: &Layout<Authored>,
    fixtures: &[FixturePlacement<Resolved>],
    groups: &[Group<Resolved>],
) -> Result<Vec<LayoutTargetRef>, LowerError> {
    let expected = groups
        .iter()
        .map(|group| LayoutTargetRef {
            kind: LayoutTargetKind::Group,
            name: group.name.clone(),
        })
        .chain(fixtures.iter().map(|fixture| LayoutTargetRef {
            kind: LayoutTargetKind::Fixture,
            name: fixture.name.clone(),
        }))
        .collect::<Vec<_>>();
    let expected_set = expected
        .iter()
        .cloned()
        .collect::<std::collections::HashSet<_>>();
    let mut seen = std::collections::HashSet::with_capacity(layout.target_order.len());
    for target in &layout.target_order {
        if !seen.insert(target.clone()) {
            return Err(LowerError::DuplicateLayoutTargetOrderEntry {
                kind: target.kind,
                name: target.name.clone(),
            });
        }
        if !expected_set.contains(target) {
            return Err(LowerError::UnknownLayoutTargetOrderEntry {
                kind: target.kind,
                name: target.name.clone(),
            });
        }
    }
    for target in expected {
        if !seen.contains(&target) {
            return Err(LowerError::MissingLayoutTargetOrderEntry {
                kind: target.kind,
                name: target.name,
            });
        }
    }
    Ok(layout.target_order.clone())
}

fn lower_patch(
    patch: &Patch<Authored>,
    fixtures: &HashMap<FixtureId, FixtureIndex>,
    controllers: &HashMap<String, ControllerIndex>,
) -> Result<Patch<Resolved>, LowerError> {
    let mut routes = Vec::with_capacity(patch.routes.len());
    for route in &patch.routes {
        let fixture = fixtures
            .get(&route.fixture)
            .copied()
            .ok_or(LowerError::UnknownFixture { id: route.fixture })?;
        let controller = controllers
            .get(route.controller.as_str())
            .copied()
            .ok_or_else(|| LowerError::UnknownController {
                name: route.controller.as_str().to_string(),
            })?;
        routes.push(Route {
            fixture,
            controller,
            universe: route.universe,
            start: route.start,
        });
    }

    Ok(Patch { routes })
}

fn lower_sequence(
    sequence: &Sequence<Authored>,
    sequence_source_path: &Utf8PathBuf,
    layout: &Layout<Authored>,
    layout_source_path: &Utf8PathBuf,
    resolver: &mut impl FnMut(
        &Utf8PathBuf,
        &ImportRef,
        ObjectKind,
    ) -> Result<ResolvedImport, LowerError>,
) -> Result<Sequence<Resolved>, LowerError> {
    let resolved_layout = lower_layout(layout, layout_source_path, resolver)?;
    let fixtures = fixture_indices(&resolved_layout.fixtures)?;
    let groups = group_indices(&resolved_layout.groups)?;

    let mut effect_indices = HashMap::with_capacity(sequence.effects.len());
    let mut effects = Vec::with_capacity(sequence.effects.len());
    for (effect_index, effect) in sequence.effects.iter().enumerate() {
        if effect_indices
            .insert(effect.id, SequenceEffectIndex(effect_index))
            .is_some()
        {
            return Err(LowerError::DuplicateSequenceEffectId { id: effect.id });
        }
        effects.push(lower_sequence_effect(
            effect,
            &fixtures,
            &groups,
            sequence_source_path,
            resolver,
        )?);
    }

    let mut automation_clips = Vec::with_capacity(sequence.automation_clips.len());
    for clip in &sequence.automation_clips {
        let mut targets = Vec::with_capacity(clip.targets.len());
        for target in &clip.targets {
            let Some(index) = effect_indices.get(target).copied() else {
                return Err(LowerError::UnknownSequenceEffect { id: *target });
            };
            targets.push(index);
        }
        let curve = resolve_curve(&clip.curve, sequence_source_path, resolver)?;
        if curve.value_type != CurveValueType::Float {
            return Err(LowerError::AutomationCurveType {
                id: clip.id,
                actual: curve.value_type,
            });
        }
        automation_clips.push(AutomationClip {
            id: clip.id,
            start: clip.start.clone(),
            duration: clip.duration.clone(),
            curve,
            targets,
        });
    }

    Ok(Sequence {
        duration: sequence.duration.clone(),
        frame_rate: sequence.frame_rate,
        audio: sequence
            .audio
            .as_ref()
            .map(|audio| resolve_path(sequence_source_path, audio.path(), audio.raw()))
            .transpose()?,
        mark_collections: sequence.mark_collections.clone(),
        effects,
        automation_clips,
    })
}

fn lower_sequence_effect(
    effect: &SequenceEffect<Authored>,
    fixtures: &HashMap<FixtureId, FixtureIndex>,
    groups: &HashMap<String, GroupIndex>,
    source_path: &Utf8PathBuf,
    resolver: &mut impl FnMut(
        &Utf8PathBuf,
        &ImportRef,
        ObjectKind,
    ) -> Result<ResolvedImport, LowerError>,
) -> Result<SequenceEffect<Resolved>, LowerError> {
    let target = match &effect.target {
        EffectTarget::Group { name: group } => {
            let name = group.as_str();
            let Some(index) = groups.get(name).copied() else {
                return Err(LowerError::UnknownGroup {
                    name: name.to_string(),
                });
            };
            EffectTarget::Group { name: index }
        }
        EffectTarget::Fixture { id: fixture } => {
            let Some(index) = fixtures.get(fixture).copied() else {
                return Err(LowerError::UnknownFixture { id: *fixture });
            };
            EffectTarget::Fixture { id: index }
        }
    };

    let mut params = IndexMap::with_capacity(effect.params.len());
    for (name, param) in &effect.params {
        params.insert(
            name.clone(),
            lower_effect_param(param, source_path, resolver)?,
        );
    }

    Ok(SequenceEffect {
        id: effect.id,
        start: effect.start.clone(),
        duration: effect.duration.clone(),
        target,
        scope: effect.scope,
        params,
        script: match &effect.script {
            InlineOrImport::Inline(script) => ScriptSource::Inline(script.clone()),
            InlineOrImport::Import { import } => {
                ScriptSource::External(resolve_path(source_path, import.path(), import.raw())?)
            }
        },
    })
}

fn lower_effect_param(
    param: &EffectParam<Authored>,
    source_path: &Utf8PathBuf,
    resolver: &mut impl FnMut(
        &Utf8PathBuf,
        &ImportRef,
        ObjectKind,
    ) -> Result<ResolvedImport, LowerError>,
) -> Result<EffectParam<Resolved>, LowerError> {
    Ok(match param {
        EffectParam::Integer { value } => EffectParam::Integer { value: *value },
        EffectParam::Float { value } => EffectParam::Float { value: *value },
        EffectParam::Boolean { value } => EffectParam::Boolean { value: *value },
        EffectParam::Enum { value } => EffectParam::Enum {
            value: value.clone(),
        },
        EffectParam::Flags { value } => EffectParam::Flags {
            value: value.clone(),
        },
        EffectParam::Color { value } => EffectParam::Color { value: *value },
        EffectParam::Curve { curve } => EffectParam::Curve {
            curve: resolve_curve(curve, source_path, resolver)?,
        },
        EffectParam::Marks { key } => EffectParam::Marks { key: key.clone() },
    })
}

fn fixture_indices(
    fixtures: &[FixturePlacement<Resolved>],
) -> Result<HashMap<FixtureId, FixtureIndex>, LowerError> {
    let mut indices = HashMap::with_capacity(fixtures.len());
    let mut names = HashMap::with_capacity(fixtures.len());
    for (index, fixture) in fixtures.iter().enumerate() {
        if indices.insert(fixture.id, FixtureIndex(index)).is_some() {
            return Err(LowerError::DuplicateFixtureId { id: fixture.id });
        }
        let name = fixture.name.trim();
        if name.is_empty() {
            return Err(LowerError::EmptyFixtureName);
        }
        if names
            .insert(name.to_string(), FixtureIndex(index))
            .is_some()
        {
            return Err(LowerError::DuplicateFixtureName {
                name: name.to_string(),
            });
        }
    }
    Ok(indices)
}

fn controller_indices(
    controllers: &[Controller],
) -> Result<HashMap<String, ControllerIndex>, LowerError> {
    let mut indices = HashMap::with_capacity(controllers.len());
    for (index, controller) in controllers.iter().enumerate() {
        if indices
            .insert(controller.name.clone(), ControllerIndex(index))
            .is_some()
        {
            return Err(LowerError::DuplicateControllerName {
                name: controller.name.clone(),
            });
        }
    }
    Ok(indices)
}

fn group_indices(groups: &[Group<Resolved>]) -> Result<HashMap<String, GroupIndex>, LowerError> {
    let mut indices = HashMap::with_capacity(groups.len());
    for (index, group) in groups.iter().enumerate() {
        if indices
            .insert(group.name.clone(), GroupIndex(index))
            .is_some()
        {
            return Err(LowerError::DuplicateGroupName {
                name: group.name.clone(),
            });
        }
    }
    Ok(indices)
}

fn resolve_path(
    source_path: &Utf8PathBuf,
    import_path: &Utf8PathBuf,
    raw: &str,
) -> Result<Utf8PathBuf, LowerError> {
    let _ = raw;
    Ok(resolve_import_path(source_path, import_path))
}

fn resolve_import(
    source_path: &Utf8PathBuf,
    import: &ImportRef,
    expected: ObjectKind,
    resolver: &mut impl FnMut(
        &Utf8PathBuf,
        &ImportRef,
        ObjectKind,
    ) -> Result<ResolvedImport, LowerError>,
) -> Result<ResolvedImport, LowerError> {
    let resolved = resolver(source_path, import, expected)?;
    if resolved.object.kind() != expected {
        return Err(LowerError::WrongImportedObjectKind {
            import: import.raw().to_string(),
            expected,
            actual: resolved.object.kind(),
        });
    }
    Ok(resolved)
}
pub(crate) fn select_imported_object(
    file: &DawnFile,
    import: &ImportRef,
) -> Result<DawnObject<Authored>, LowerError> {
    if let Some(object) = import.object() {
        return file
            .get(object.as_str())
            .cloned()
            .ok_or_else(|| LowerError::Import {
                import: import.raw().to_string(),
                message: format!("object `{}` was not found", object.as_str()),
            });
    }

    if file.len() == 1 {
        if let Some(object) = file.values().next() {
            return Ok(object.clone());
        }
    }

    Err(LowerError::Import {
        import: import.raw().to_string(),
        message: "import must name an object when the target file has zero or multiple objects"
            .to_string(),
    })
}
fn resolve_display(
    value: &InlineOrImport<Display<Authored>>,
    source_path: &Utf8PathBuf,
    expected: ObjectKind,
    resolver: &mut impl FnMut(
        &Utf8PathBuf,
        &ImportRef,
        ObjectKind,
    ) -> Result<ResolvedImport, LowerError>,
) -> Result<(Display<Authored>, Utf8PathBuf), LowerError> {
    match value {
        InlineOrImport::Inline(display) => Ok((display.clone(), source_path.clone())),
        InlineOrImport::Import { import } => {
            let resolved = resolve_import(source_path, import, expected, resolver)?;
            let DawnObject::Display(display) = resolved.object else {
                unreachable!("resolved import kind was checked");
            };
            Ok((display, resolved.source_path))
        }
    }
}

fn resolve_controller(
    value: &InlineOrImport<Controller>,
    source_path: &Utf8PathBuf,
    resolver: &mut impl FnMut(
        &Utf8PathBuf,
        &ImportRef,
        ObjectKind,
    ) -> Result<ResolvedImport, LowerError>,
) -> Result<(Controller, Utf8PathBuf), LowerError> {
    match value {
        InlineOrImport::Inline(controller) => Ok((controller.clone(), source_path.clone())),
        InlineOrImport::Import { import } => {
            let resolved = resolve_import(source_path, import, ObjectKind::Controller, resolver)?;
            let DawnObject::Controller(controller) = resolved.object else {
                unreachable!("resolved import kind was checked");
            };
            Ok((controller, resolved.source_path))
        }
    }
}

fn resolve_layout(
    value: &InlineOrImport<Layout<Authored>>,
    source_path: &Utf8PathBuf,
    expected: ObjectKind,
    resolver: &mut impl FnMut(
        &Utf8PathBuf,
        &ImportRef,
        ObjectKind,
    ) -> Result<ResolvedImport, LowerError>,
) -> Result<(Layout<Authored>, Utf8PathBuf), LowerError> {
    match value {
        InlineOrImport::Inline(layout) => Ok((layout.clone(), source_path.clone())),
        InlineOrImport::Import { import } => {
            let resolved = resolve_import(source_path, import, expected, resolver)?;
            let DawnObject::Layout(layout) = resolved.object else {
                unreachable!("resolved import kind was checked");
            };
            Ok((layout, resolved.source_path))
        }
    }
}

fn resolve_fixture(
    value: &InlineOrImport<Fixture>,
    source_path: &Utf8PathBuf,
    resolver: &mut impl FnMut(
        &Utf8PathBuf,
        &ImportRef,
        ObjectKind,
    ) -> Result<ResolvedImport, LowerError>,
) -> Result<(Fixture, Utf8PathBuf), LowerError> {
    match value {
        InlineOrImport::Inline(fixture) => Ok((fixture.clone(), source_path.clone())),
        InlineOrImport::Import { import } => {
            let resolved = resolve_import(source_path, import, ObjectKind::Fixture, resolver)?;
            let DawnObject::Fixture(fixture) = resolved.object else {
                unreachable!("resolved import kind was checked");
            };
            Ok((fixture, resolved.source_path))
        }
    }
}

fn resolve_patch(
    value: &InlineOrImport<Patch<Authored>>,
    source_path: &Utf8PathBuf,
    expected: ObjectKind,
    resolver: &mut impl FnMut(
        &Utf8PathBuf,
        &ImportRef,
        ObjectKind,
    ) -> Result<ResolvedImport, LowerError>,
) -> Result<(Patch<Authored>, Utf8PathBuf), LowerError> {
    match value {
        InlineOrImport::Inline(patch) => Ok((patch.clone(), source_path.clone())),
        InlineOrImport::Import { import } => {
            let resolved = resolve_import(source_path, import, expected, resolver)?;
            let DawnObject::Patch(patch) = resolved.object else {
                unreachable!("resolved import kind was checked");
            };
            Ok((patch, resolved.source_path))
        }
    }
}

fn resolve_sequence(
    value: &InlineOrImport<Sequence<Authored>>,
    source_path: &Utf8PathBuf,
    expected: ObjectKind,
    resolver: &mut impl FnMut(
        &Utf8PathBuf,
        &ImportRef,
        ObjectKind,
    ) -> Result<ResolvedImport, LowerError>,
) -> Result<(Sequence<Authored>, Utf8PathBuf), LowerError> {
    match value {
        InlineOrImport::Inline(sequence) => Ok((sequence.clone(), source_path.clone())),
        InlineOrImport::Import { import } => {
            let resolved = resolve_import(source_path, import, expected, resolver)?;
            let DawnObject::Sequence(sequence) = resolved.object else {
                unreachable!("resolved import kind was checked");
            };
            Ok((sequence, resolved.source_path))
        }
    }
}

fn resolve_curve(
    value: &InlineOrImport<Curve>,
    source_path: &Utf8PathBuf,
    resolver: &mut impl FnMut(
        &Utf8PathBuf,
        &ImportRef,
        ObjectKind,
    ) -> Result<ResolvedImport, LowerError>,
) -> Result<Curve, LowerError> {
    match value {
        InlineOrImport::Inline(curve) => Ok(curve.clone()),
        InlineOrImport::Import { import } => {
            let resolved = resolve_import(source_path, import, ObjectKind::Curve, resolver)?;
            let DawnObject::Curve(curve) = resolved.object else {
                unreachable!("resolved import kind was checked");
            };
            Ok(curve)
        }
    }
}
