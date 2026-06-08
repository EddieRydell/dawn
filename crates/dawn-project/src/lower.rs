use std::collections::{HashMap, HashSet};
use std::error::Error;
use std::fmt;

use indexmap::IndexMap;

use crate::effect_script::{
    compile as compile_effect_script, compile_module as compile_effect_script_module,
    CompiledEffect,
};
use crate::model::*;
use crate::path::{resolve_import_path, Utf8PathBuf};

#[derive(Debug, Clone)]
pub struct ResolvedImport {
    pub source_path: Utf8PathBuf,
    pub symbol: String,
    pub object: DawnObject<Authored>,
}

#[derive(Debug, Clone)]
pub struct ResolvedEffectImport {
    pub source_path: Utf8PathBuf,
    pub effect_name: String,
    pub source: String,
}

pub trait SymbolResolver {
    fn resolve_object(
        &mut self,
        source_path: &Utf8PathBuf,
        reference: &SymbolRef,
        expected: ObjectKind,
    ) -> Result<ResolvedImport, LowerError>;

    fn resolve_effect(
        &mut self,
        source_path: &Utf8PathBuf,
        reference: &SymbolRef,
    ) -> Result<ResolvedEffectImport, LowerError>;
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
        reference: String,
        expected: ObjectKind,
        actual: ObjectKind,
    },
    Import {
        reference: String,
        message: String,
    },
    EffectCompile {
        reference: String,
        message: String,
    },
    DuplicateFixtureId {
        id: FixtureId,
    },
    UnknownFixture {
        id: FixtureId,
    },
    DuplicateGroupId {
        id: GroupInstantiationId,
    },
    UnknownGroup {
        id: GroupInstantiationId,
    },
    DisplayDoesNotUseController {
        reference: String,
    },
    DuplicateLayoutTargetOrderEntry {
        kind: LayoutTargetKind,
        id: u32,
    },
    MissingLayoutTargetOrderEntry {
        kind: LayoutTargetKind,
        id: u32,
    },
    UnknownLayoutTargetOrderEntry {
        kind: LayoutTargetKind,
        id: u32,
    },
    DuplicateSequenceEffectId {
        id: u32,
    },
    UnknownSequenceEffect {
        id: u32,
    },
    DuplicateAutomationClipId {
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
                reference,
                expected,
                actual,
            } => write!(
                formatter,
                "reference `{reference}` must resolve to a {expected}, but found a {actual}"
            ),
            Self::Import { reference, message } => {
                write!(
                    formatter,
                    "failed to resolve reference `{reference}`: {message}"
                )
            }
            Self::EffectCompile { reference, message } => {
                write!(
                    formatter,
                    "failed to compile effect `{reference}`: {message}"
                )
            }
            Self::DuplicateFixtureId { id } => write!(formatter, "duplicate fixture id `{id}`"),
            Self::UnknownFixture { id } => write!(formatter, "unknown fixture `{id}`"),
            Self::DuplicateGroupId { id } => write!(formatter, "duplicate group id `{id}`"),
            Self::UnknownGroup { id } => write!(formatter, "unknown group `{id}`"),
            Self::DisplayDoesNotUseController { reference } => {
                write!(formatter, "display does not use controller `{reference}`")
            }
            Self::DuplicateLayoutTargetOrderEntry { kind, id } => {
                write!(
                    formatter,
                    "duplicate layout target order entry `{:?}:{id}`",
                    kind
                )
            }
            Self::MissingLayoutTargetOrderEntry { kind, id } => {
                write!(
                    formatter,
                    "missing layout target order entry `{:?}:{id}`",
                    kind
                )
            }
            Self::UnknownLayoutTargetOrderEntry { kind, id } => {
                write!(
                    formatter,
                    "unknown layout target order entry `{:?}:{id}`",
                    kind
                )
            }
            Self::DuplicateSequenceEffectId { id } => {
                write!(formatter, "duplicate sequence effect `{id}`")
            }
            Self::UnknownSequenceEffect { id } => {
                write!(formatter, "unknown sequence effect `{id}`")
            }
            Self::DuplicateAutomationClipId { id } => {
                write!(formatter, "duplicate automation clip `{id}`")
            }
            Self::AutomationCurveType { id, actual } => write!(
                formatter,
                "automation clip `{id}` requires a float curve, but found a {:?} curve",
                actual
            ),
        }
    }
}

impl Error for LowerError {}

struct LowerCtx<'a, R: SymbolResolver> {
    resolver: &'a mut R,
    stores: ResolvedStores,
}

impl<'a, R: SymbolResolver> LowerCtx<'a, R> {
    fn new(resolver: &'a mut R) -> Self {
        Self {
            resolver,
            stores: ResolvedStores::default(),
        }
    }

    fn resolve_import(
        &mut self,
        source_path: &Utf8PathBuf,
        reference: &SymbolRef,
        expected: ObjectKind,
    ) -> Result<ResolvedImport, LowerError> {
        let resolved = self
            .resolver
            .resolve_object(source_path, reference, expected)?;
        if resolved.object.kind() != expected {
            return Err(LowerError::WrongImportedObjectKind {
                reference: reference.raw().to_string(),
                expected,
                actual: resolved.object.kind(),
            });
        }
        Ok(resolved)
    }

    fn display_key(path: Utf8PathBuf, symbol: String) -> DisplayDefinitionKey {
        DisplayDefinitionKey::new(path, symbol)
    }

    fn sequence_key(path: Utf8PathBuf, symbol: String) -> SequenceDefinitionKey {
        SequenceDefinitionKey::new(path, symbol)
    }

    fn controller_key(path: Utf8PathBuf, symbol: String) -> ControllerDefinitionKey {
        ControllerDefinitionKey::new(path, symbol)
    }

    fn patch_key(path: Utf8PathBuf, symbol: String) -> PatchDefinitionKey {
        PatchDefinitionKey::new(path, symbol)
    }

    fn layout_key(path: Utf8PathBuf, symbol: String) -> LayoutDefinitionKey {
        LayoutDefinitionKey::new(path, symbol)
    }

    fn fixture_key(path: Utf8PathBuf, symbol: String) -> FixtureDefinitionKey {
        FixtureDefinitionKey::new(path, symbol)
    }

    fn curve_key(path: Utf8PathBuf, symbol: String) -> CurveDefinitionKey {
        CurveDefinitionKey::new(path, symbol)
    }

    fn effect_key(path: Utf8PathBuf, name: String) -> EffectDefinitionKey {
        EffectDefinitionKey::new(path, name)
    }

    fn lower_display_ref(
        &mut self,
        value: &InlineOrRef<Display<Authored>>,
        source_path: &Utf8PathBuf,
    ) -> Result<Display<Resolved>, LowerError> {
        match value {
            InlineOrRef::Inline(display) => self.lower_display(display, source_path),
            InlineOrRef::Ref(reference) => {
                let resolved = self.resolve_import(source_path, reference, ObjectKind::Display)?;
                let key = Self::display_key(resolved.source_path.clone(), resolved.symbol.clone());
                if let Some(existing) = self.stores.displays.get(&key) {
                    return Ok(existing.value.clone());
                }
                let DawnObject::Display(display) = resolved.object else {
                    unreachable!("resolved import kind was checked");
                };
                let lowered = self.lower_display(&display, &resolved.source_path)?;
                self.stores.displays.insert(
                    key,
                    ResolvedObject {
                        value: lowered.clone(),
                        provenance: ResolvedProvenance::Named {
                            path: resolved.source_path,
                            symbol: resolved.symbol,
                        },
                    },
                );
                Ok(lowered)
            }
        }
    }

    fn lower_sequence_ref(
        &mut self,
        value: &InlineOrRef<Sequence<Authored>>,
        source_path: &Utf8PathBuf,
        layout: &Layout<Resolved>,
    ) -> Result<Sequence<Resolved>, LowerError> {
        match value {
            InlineOrRef::Inline(sequence) => self.lower_sequence(sequence, source_path, layout),
            InlineOrRef::Ref(reference) => {
                let resolved = self.resolve_import(source_path, reference, ObjectKind::Sequence)?;
                let key = Self::sequence_key(resolved.source_path.clone(), resolved.symbol.clone());
                if let Some(existing) = self.stores.sequences.get(&key) {
                    return Ok(existing.value.clone());
                }
                let DawnObject::Sequence(sequence) = resolved.object else {
                    unreachable!("resolved import kind was checked");
                };
                let lowered = self.lower_sequence(&sequence, &resolved.source_path, layout)?;
                self.stores.sequences.insert(
                    key,
                    ResolvedObject {
                        value: lowered.clone(),
                        provenance: ResolvedProvenance::Named {
                            path: resolved.source_path,
                            symbol: resolved.symbol,
                        },
                    },
                );
                Ok(lowered)
            }
        }
    }

    fn lower_controller_ref(
        &mut self,
        value: &InlineOrRef<Controller>,
        source_path: &Utf8PathBuf,
    ) -> Result<(Controller, Option<ControllerDefinitionKey>), LowerError> {
        match value {
            InlineOrRef::Inline(controller) => Ok((controller.clone(), None)),
            InlineOrRef::Ref(reference) => {
                let resolved =
                    self.resolve_import(source_path, reference, ObjectKind::Controller)?;
                let key =
                    Self::controller_key(resolved.source_path.clone(), resolved.symbol.clone());
                if let Some(existing) = self.stores.controllers.get(&key) {
                    return Ok((existing.value.clone(), Some(key)));
                }
                let DawnObject::Controller(controller) = resolved.object else {
                    unreachable!("resolved import kind was checked");
                };
                self.stores.controllers.insert(
                    key.clone(),
                    ResolvedObject {
                        value: controller.clone(),
                        provenance: ResolvedProvenance::Named {
                            path: resolved.source_path,
                            symbol: resolved.symbol,
                        },
                    },
                );
                Ok((controller, Some(key)))
            }
        }
    }

    fn lower_controller_reference(
        &mut self,
        reference: &SymbolRef,
        source_path: &Utf8PathBuf,
    ) -> Result<ControllerDefinitionKey, LowerError> {
        let resolved = self.resolve_import(source_path, reference, ObjectKind::Controller)?;
        let key = Self::controller_key(resolved.source_path.clone(), resolved.symbol.clone());
        if !self.stores.controllers.contains_key(&key) {
            let DawnObject::Controller(controller) = resolved.object else {
                unreachable!("resolved import kind was checked");
            };
            self.stores.controllers.insert(
                key.clone(),
                ResolvedObject {
                    value: controller,
                    provenance: ResolvedProvenance::Named {
                        path: resolved.source_path,
                        symbol: resolved.symbol,
                    },
                },
            );
        }
        Ok(key)
    }

    fn lower_patch_ref(
        &mut self,
        value: &InlineOrRef<Patch<Authored>>,
        source_path: &Utf8PathBuf,
        fixtures: &HashMap<FixtureId, FixturePlacement<Resolved>>,
        display_controllers: &HashSet<ControllerDefinitionKey>,
    ) -> Result<Patch<Resolved>, LowerError> {
        match value {
            InlineOrRef::Inline(patch) => {
                self.lower_patch(patch, source_path, fixtures, display_controllers)
            }
            InlineOrRef::Ref(reference) => {
                let resolved = self.resolve_import(source_path, reference, ObjectKind::Patch)?;
                let key = Self::patch_key(resolved.source_path.clone(), resolved.symbol.clone());
                if let Some(existing) = self.stores.patches.get(&key) {
                    return Ok(existing.value.clone());
                }
                let DawnObject::Patch(patch) = resolved.object else {
                    unreachable!("resolved import kind was checked");
                };
                let lowered =
                    self.lower_patch(&patch, &resolved.source_path, fixtures, display_controllers)?;
                self.stores.patches.insert(
                    key,
                    ResolvedObject {
                        value: lowered.clone(),
                        provenance: ResolvedProvenance::Named {
                            path: resolved.source_path,
                            symbol: resolved.symbol,
                        },
                    },
                );
                Ok(lowered)
            }
        }
    }

    fn lower_layout_ref(
        &mut self,
        value: &InlineOrRef<Layout<Authored>>,
        source_path: &Utf8PathBuf,
    ) -> Result<Layout<Resolved>, LowerError> {
        match value {
            InlineOrRef::Inline(layout) => self.lower_layout(layout, source_path),
            InlineOrRef::Ref(reference) => {
                let resolved = self.resolve_import(source_path, reference, ObjectKind::Layout)?;
                let key = Self::layout_key(resolved.source_path.clone(), resolved.symbol.clone());
                if let Some(existing) = self.stores.layouts.get(&key) {
                    return Ok(existing.value.clone());
                }
                let DawnObject::Layout(layout) = resolved.object else {
                    unreachable!("resolved import kind was checked");
                };
                let lowered = self.lower_layout(&layout, &resolved.source_path)?;
                self.stores.layouts.insert(
                    key,
                    ResolvedObject {
                        value: lowered.clone(),
                        provenance: ResolvedProvenance::Named {
                            path: resolved.source_path,
                            symbol: resolved.symbol,
                        },
                    },
                );
                Ok(lowered)
            }
        }
    }

    fn lower_fixture_definition_ref(
        &mut self,
        value: &InlineOrRef<Fixture>,
        source_path: &Utf8PathBuf,
    ) -> Result<Fixture, LowerError> {
        match value {
            InlineOrRef::Inline(fixture) => Ok(fixture.clone()),
            InlineOrRef::Ref(reference) => {
                let resolved = self.resolve_import(source_path, reference, ObjectKind::Fixture)?;
                let key = Self::fixture_key(resolved.source_path.clone(), resolved.symbol.clone());
                if let Some(existing) = self.stores.fixture_definitions.get(&key) {
                    return Ok(existing.value.clone());
                }
                let DawnObject::Fixture(fixture) = resolved.object else {
                    unreachable!("resolved import kind was checked");
                };
                self.stores.fixture_definitions.insert(
                    key,
                    ResolvedObject {
                        value: fixture.clone(),
                        provenance: ResolvedProvenance::Named {
                            path: resolved.source_path,
                            symbol: resolved.symbol,
                        },
                    },
                );
                Ok(fixture)
            }
        }
    }

    fn lower_curve_ref(
        &mut self,
        value: &InlineOrRef<Curve>,
        source_path: &Utf8PathBuf,
    ) -> Result<Curve, LowerError> {
        match value {
            InlineOrRef::Inline(curve) => Ok(curve.clone()),
            InlineOrRef::Ref(reference) => {
                let resolved = self.resolve_import(source_path, reference, ObjectKind::Curve)?;
                let key = Self::curve_key(resolved.source_path.clone(), resolved.symbol.clone());
                if let Some(existing) = self.stores.curves.get(&key) {
                    return Ok(existing.value.clone());
                }
                let DawnObject::Curve(curve) = resolved.object else {
                    unreachable!("resolved import kind was checked");
                };
                self.stores.curves.insert(
                    key,
                    ResolvedObject {
                        value: curve.clone(),
                        provenance: ResolvedProvenance::Named {
                            path: resolved.source_path,
                            symbol: resolved.symbol,
                        },
                    },
                );
                Ok(curve)
            }
        }
    }

    fn lower_display(
        &mut self,
        display: &Display<Authored>,
        source_path: &Utf8PathBuf,
    ) -> Result<Display<Resolved>, LowerError> {
        let mut controllers = Vec::with_capacity(display.controllers.len());
        let mut controller_keys = HashSet::with_capacity(display.controllers.len());
        for controller in &display.controllers {
            let (controller, key) = self.lower_controller_ref(controller, source_path)?;
            if let Some(key) = key {
                controller_keys.insert(key);
            }
            controllers.push(controller);
        }

        let layout = self.lower_layout_ref(&display.layout, source_path)?;
        let fixture_lookup = layout
            .fixtures
            .iter()
            .map(|fixture| (fixture.id, fixture.clone()))
            .collect::<HashMap<_, _>>();
        let patch = self.lower_patch_ref(
            &display.patch,
            source_path,
            &fixture_lookup,
            &controller_keys,
        )?;

        Ok(Display {
            controllers,
            patch,
            layout,
        })
    }

    fn lower_layout(
        &mut self,
        layout: &Layout<Authored>,
        source_path: &Utf8PathBuf,
    ) -> Result<Layout<Resolved>, LowerError> {
        let mut fixtures = Vec::with_capacity(layout.fixtures.len());
        let mut fixture_ids = HashSet::with_capacity(layout.fixtures.len());
        for placement in &layout.fixtures {
            if !fixture_ids.insert(placement.id) {
                return Err(LowerError::DuplicateFixtureId { id: placement.id });
            }
            fixtures.push(FixturePlacement {
                id: placement.id,
                name: placement.name.clone(),
                fixture: self.lower_fixture_definition_ref(&placement.fixture, source_path)?,
                transform: placement.transform,
            });
        }

        let mut groups = Vec::with_capacity(layout.groups.len());
        let mut group_ids = HashSet::with_capacity(layout.groups.len());
        for group in &layout.groups {
            if !group_ids.insert(group.id) {
                return Err(LowerError::DuplicateGroupId { id: group.id });
            }
            let mut members = Vec::with_capacity(group.members.len());
            for member in &group.members {
                if !fixture_ids.contains(member) {
                    return Err(LowerError::UnknownFixture { id: *member });
                }
                members.push(*member);
            }
            groups.push(Group {
                id: group.id,
                name: group.name.clone(),
                members,
            });
        }

        Ok(Layout {
            target_order: self.validate_layout_target_order(layout, &fixture_ids, &group_ids)?,
            fixtures,
            groups,
        })
    }

    fn validate_layout_target_order(
        &self,
        layout: &Layout<Authored>,
        fixtures: &HashSet<FixtureId>,
        groups: &HashSet<GroupInstantiationId>,
    ) -> Result<Vec<LayoutTargetRef>, LowerError> {
        let expected = groups
            .iter()
            .map(|id| LayoutTargetRef {
                kind: LayoutTargetKind::Group,
                id: id.0,
            })
            .chain(fixtures.iter().map(|id| LayoutTargetRef {
                kind: LayoutTargetKind::Fixture,
                id: id.0,
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
                    id: target.id,
                });
            }
            if !expected_set.contains(target) {
                return Err(LowerError::UnknownLayoutTargetOrderEntry {
                    kind: target.kind,
                    id: target.id,
                });
            }
        }
        for target in expected {
            if !seen.contains(&target) {
                return Err(LowerError::MissingLayoutTargetOrderEntry {
                    kind: target.kind,
                    id: target.id,
                });
            }
        }
        Ok(layout.target_order.clone())
    }

    fn lower_patch(
        &mut self,
        patch: &Patch<Authored>,
        source_path: &Utf8PathBuf,
        fixtures: &HashMap<FixtureId, FixturePlacement<Resolved>>,
        display_controllers: &HashSet<ControllerDefinitionKey>,
    ) -> Result<Patch<Resolved>, LowerError> {
        let mut routes = Vec::with_capacity(patch.routes.len());
        for route in &patch.routes {
            if !fixtures.contains_key(&route.fixture) {
                return Err(LowerError::UnknownFixture { id: route.fixture });
            }
            let controller = self.lower_controller_reference(&route.controller, source_path)?;
            if !display_controllers.contains(&controller) {
                return Err(LowerError::DisplayDoesNotUseController {
                    reference: route.controller.raw().to_string(),
                });
            }
            routes.push(Route {
                fixture: route.fixture,
                controller,
                universe: route.universe,
                start: route.start,
            });
        }
        Ok(Patch { routes })
    }

    fn lower_sequence(
        &mut self,
        sequence: &Sequence<Authored>,
        source_path: &Utf8PathBuf,
        layout: &Layout<Resolved>,
    ) -> Result<Sequence<Resolved>, LowerError> {
        let fixture_ids = layout
            .fixtures
            .iter()
            .map(|fixture| fixture.id)
            .collect::<HashSet<_>>();
        let group_ids = layout
            .groups
            .iter()
            .map(|group| group.id)
            .collect::<HashSet<_>>();

        let mut effect_ids = HashSet::with_capacity(sequence.effects.len());
        let mut effects = Vec::with_capacity(sequence.effects.len());
        for effect in &sequence.effects {
            if !effect_ids.insert(effect.id) {
                return Err(LowerError::DuplicateSequenceEffectId { id: effect.id });
            }
            effects.push(self.lower_sequence_effect(
                effect,
                &fixture_ids,
                &group_ids,
                source_path,
            )?);
        }

        let mut automation_clip_ids = HashSet::with_capacity(sequence.automation_clips.len());
        let mut automation_clips = Vec::with_capacity(sequence.automation_clips.len());
        for clip in &sequence.automation_clips {
            if !automation_clip_ids.insert(clip.id) {
                return Err(LowerError::DuplicateAutomationClipId { id: clip.id });
            }
            let mut targets = Vec::with_capacity(clip.targets.len());
            for target in &clip.targets {
                if !effect_ids.contains(target) {
                    return Err(LowerError::UnknownSequenceEffect { id: *target });
                }
                targets.push(SequenceEffectId(*target));
            }
            let curve: CurveUse<Resolved> = CurveUse {
                id: clip.curve.id,
                curve: self.lower_curve_ref(&clip.curve.curve, source_path)?,
            };
            if curve.curve.value_type != CurveValueType::Float {
                return Err(LowerError::AutomationCurveType {
                    id: clip.id,
                    actual: curve.curve.value_type,
                });
            }
            automation_clips.push(AutomationClip {
                id: clip.id,
                start: clip.start,
                duration: clip.duration,
                curve,
                targets,
            });
        }

        Ok(Sequence {
            duration: sequence.duration,
            frame_rate: sequence.frame_rate,
            audio: sequence
                .audio
                .as_ref()
                .map(|audio| resolve_path(source_path, audio.path(), audio.raw()))
                .transpose()?,
            mark_collections: sequence.mark_collections.clone(),
            effects,
            automation_clips,
        })
    }

    fn lower_sequence_effect(
        &mut self,
        effect: &SequenceEffect<Authored>,
        fixtures: &HashSet<FixtureId>,
        groups: &HashSet<GroupInstantiationId>,
        source_path: &Utf8PathBuf,
    ) -> Result<SequenceEffect<Resolved>, LowerError> {
        let target = match &effect.target {
            EffectTarget::Group { id: group } => {
                if !groups.contains(group) {
                    return Err(LowerError::UnknownGroup { id: *group });
                }
                EffectTarget::Group { id: *group }
            }
            EffectTarget::Fixture { id: fixture } => {
                if !fixtures.contains(fixture) {
                    return Err(LowerError::UnknownFixture { id: *fixture });
                }
                EffectTarget::Fixture { id: *fixture }
            }
        };

        let mut params = IndexMap::with_capacity(effect.params.len());
        for (name, param) in &effect.params {
            params.insert(name.clone(), self.lower_effect_param(param, source_path)?);
        }

        Ok(SequenceEffect {
            id: SequenceEffectId(effect.id),
            start: effect.start,
            duration: effect.duration,
            target,
            scope: effect.scope,
            params,
            script: self.lower_effect_definition_ref(&effect.script, source_path, effect.id)?,
        })
    }

    fn lower_effect_definition_ref(
        &mut self,
        script: &InlineScriptOrRef,
        source_path: &Utf8PathBuf,
        effect_id: u32,
    ) -> Result<ScriptSource, LowerError> {
        match script {
            InlineScriptOrRef::Inline { inline } => {
                compile_effect_script(inline).map_err(|diagnostics| LowerError::EffectCompile {
                    reference: format!("effect {effect_id}"),
                    message: first_script_diagnostic(diagnostics),
                })?;
                Ok(ScriptSource::Inline(inline.clone()))
            }
            InlineScriptOrRef::Ref(reference) => {
                let resolved = self.resolver.resolve_effect(source_path, reference)?;
                let compiled =
                    compile_effect_script_module(&resolved.source).map_err(|diagnostics| {
                        LowerError::EffectCompile {
                            reference: reference.raw().to_string(),
                            message: first_script_diagnostic(diagnostics),
                        }
                    })?;
                let compiled =
                    select_compiled_effect(compiled, &resolved.effect_name, reference.raw())?;
                let key =
                    Self::effect_key(resolved.source_path.clone(), resolved.effect_name.clone());
                self.stores
                    .effect_definitions
                    .entry(key)
                    .or_insert_with(|| ResolvedObject {
                        value: EffectDefinition {
                            source: EffectDefinitionSource::External {
                                path: resolved.source_path.clone(),
                                effect_name: resolved.effect_name.clone(),
                            },
                            schema: compiled.params.clone(),
                            kind: compiled.kind,
                            compiled,
                        },
                        provenance: ResolvedProvenance::ExternalEffect {
                            path: resolved.source_path.clone(),
                            effect_name: resolved.effect_name.clone(),
                        },
                    });
                Ok(ScriptSource::External(EffectScriptId::new(
                    resolved.source_path,
                    resolved.effect_name,
                )))
            }
        }
    }

    fn lower_effect_param(
        &mut self,
        param: &EffectParam<Authored>,
        source_path: &Utf8PathBuf,
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
                curve: CurveUse {
                    id: curve.id,
                    curve: self.lower_curve_ref(&curve.curve, source_path)?,
                },
            },
            EffectParam::Array {
                element_type,
                values,
            } => EffectParam::Array {
                element_type: *element_type,
                values: values
                    .iter()
                    .map(|value| self.lower_effect_param_array_value(value, source_path))
                    .collect::<Result<Vec<_>, _>>()?,
            },
            EffectParam::Marks { key } => EffectParam::Marks { key: key.clone() },
        })
    }

    fn lower_effect_param_array_value(
        &mut self,
        value: &EffectParamArrayValue<Authored>,
        source_path: &Utf8PathBuf,
    ) -> Result<EffectParamArrayValue<Resolved>, LowerError> {
        Ok(match value {
            EffectParamArrayValue::Integer(value) => EffectParamArrayValue::Integer(*value),
            EffectParamArrayValue::Float(value) => EffectParamArrayValue::Float(*value),
            EffectParamArrayValue::Boolean(value) => EffectParamArrayValue::Boolean(*value),
            EffectParamArrayValue::Color(value) => EffectParamArrayValue::Color(*value),
            EffectParamArrayValue::Curve(curve) => EffectParamArrayValue::Curve(CurveUse {
                id: curve.id,
                curve: self.lower_curve_ref(&curve.curve, source_path)?,
            }),
        })
    }
}

pub fn lower_project(
    file: &DawnFile,
    project_key: &str,
    source_path: &Utf8PathBuf,
    resolver: &mut impl SymbolResolver,
) -> Result<DawnProject, LowerError> {
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

    let mut ctx = LowerCtx::new(resolver);
    let display = ctx.lower_display_ref(&project.display, source_path)?;
    let mut sequences = Vec::with_capacity(project.sequences.len());
    for sequence in &project.sequences {
        sequences.push(ctx.lower_sequence_ref(sequence, source_path, &display.layout)?);
    }

    Ok(Project {
        display,
        sequences,
        stores: ctx.stores,
    })
}

pub(crate) fn select_referenced_object(
    file: &DawnFile,
    reference: &SymbolRef,
) -> Result<DawnObject<Authored>, LowerError> {
    file.get(reference.name().as_str())
        .cloned()
        .ok_or_else(|| LowerError::Import {
            reference: reference.raw().to_string(),
            message: format!("object `{}` was not found", reference.name().as_str()),
        })
}

fn resolve_path(
    source_path: &Utf8PathBuf,
    import_path: &Utf8PathBuf,
    raw: &str,
) -> Result<Utf8PathBuf, LowerError> {
    let _ = raw;
    Ok(resolve_import_path(source_path, import_path))
}

fn first_script_diagnostic(diagnostics: Vec<crate::effect_script::ScriptDiagnostic>) -> String {
    diagnostics
        .first()
        .map(|diagnostic| diagnostic.message.clone())
        .unwrap_or_else(|| "effect script did not compile".to_string())
}

fn select_compiled_effect(
    mut compiled: Vec<CompiledEffect>,
    effect_name: &str,
    reference: &str,
) -> Result<CompiledEffect, LowerError> {
    let Some(index) = compiled
        .iter()
        .position(|compiled| compiled.name == effect_name)
    else {
        return Err(LowerError::EffectCompile {
            reference: reference.to_string(),
            message: format!("compiled module did not contain effect `{effect_name}`"),
        });
    };
    Ok(compiled.remove(index))
}
