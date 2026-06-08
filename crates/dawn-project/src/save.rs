use indexmap::IndexMap;

use crate::diagnostics::{DiagnosticSeverity, ProjectDiagnostic, ProjectDiagnosticKind, TextRange};
use crate::fs::WorkspaceFs;
use crate::model::*;
use crate::path::{canonicalize_path, resolve_import_path, Utf8PathBuf};

#[derive(Debug, Clone)]
pub struct ProjectSaveResult {
    pub written_files: Vec<Utf8PathBuf>,
    pub diagnostics: Vec<ProjectDiagnostic>,
}

pub fn save_project(fs: &WorkspaceFs, project: &DawnProject) -> ProjectSaveResult {
    let projected = project_source_text(project);
    let files = match projected {
        Ok(files) => files,
        Err(diagnostics) => {
            return ProjectSaveResult {
                written_files: Vec::new(),
                diagnostics,
            };
        }
    };

    let mut written_files = Vec::new();
    let mut diagnostics = Vec::new();
    for (path, text) in files {
        if let Err(error) = fs.write(&path, text) {
            diagnostics.push(diagnostic(
                path.clone(),
                format!("failed to write `{path}`: {error}"),
                ProjectDiagnosticKind::Io,
            ));
            continue;
        }
        written_files.push(path);
    }

    ProjectSaveResult {
        written_files,
        diagnostics,
    }
}

fn project_source_text(
    project: &DawnProject,
) -> Result<IndexMap<Utf8PathBuf, String>, Vec<ProjectDiagnostic>> {
    let mut ctx = SaveCtx {
        project,
        diagnostics: Vec::new(),
    };
    let Some(root_project) = project.stores.root_project.as_ref() else {
        return Err(vec![diagnostic(
            Utf8PathBuf::new(),
            "resolved project is missing root source metadata".to_string(),
            ProjectDiagnosticKind::Lower,
        )]);
    };
    if project.stores.source_files.is_empty() {
        return Err(vec![diagnostic(
            root_project.path.clone(),
            "resolved project has no source files to save".to_string(),
            ProjectDiagnosticKind::Lower,
        )]);
    }

    let mut files = IndexMap::new();
    for (path, source) in &project.stores.source_files {
        match source {
            ResolvedSourceFile::Dawn { imports, objects } => {
                let mut file = DawnFile {
                    imports: imports.clone(),
                    objects: IndexMap::new(),
                };
                for (name, object) in objects {
                    if let Some(object) = ctx.project_object(path, imports, name, object) {
                        file.insert(name.clone(), object);
                    }
                }
                match serde_yaml::to_string(&file) {
                    Ok(text) => {
                        files.insert(path.clone(), text);
                    }
                    Err(error) => ctx.diagnostics.push(diagnostic(
                        path.clone(),
                        format!("failed to serialize `{path}`: {error}"),
                        ProjectDiagnosticKind::DawnSchema,
                    )),
                }
            }
            ResolvedSourceFile::Effect { text } => {
                files.insert(path.clone(), text.clone());
            }
        }
    }

    if ctx.diagnostics.is_empty() {
        Ok(files)
    } else {
        Err(ctx.diagnostics)
    }
}

struct SaveCtx<'a> {
    project: &'a DawnProject,
    diagnostics: Vec<ProjectDiagnostic>,
}

impl SaveCtx<'_> {
    fn project_object(
        &mut self,
        source_path: &Utf8PathBuf,
        imports: &[DawnImport],
        name: &str,
        object: &ResolvedSourceObject,
    ) -> Option<DawnObject<Authored>> {
        match object {
            ResolvedSourceObject::Project(key) => {
                if key != self.project.stores.root_project.as_ref()? {
                    self.missing(
                        source_path,
                        format!("project `{name}` is not the loaded root project"),
                    );
                    return None;
                }
                Some(DawnObject::Project(
                    self.project_to_authored(source_path, imports)?,
                ))
            }
            ResolvedSourceObject::Display(key) => self
                .lookup(source_path, name, key, &self.project.stores.displays)
                .and_then(|display| self.display_to_authored(source_path, imports, display))
                .map(DawnObject::Display),
            ResolvedSourceObject::Controller(key) => self
                .lookup(source_path, name, key, &self.project.stores.controllers)
                .cloned()
                .map(DawnObject::Controller),
            ResolvedSourceObject::Layout(key) => self
                .lookup(source_path, name, key, &self.project.stores.layouts)
                .and_then(|layout| self.layout_to_authored(source_path, imports, layout))
                .map(DawnObject::Layout),
            ResolvedSourceObject::Fixture(key) => self
                .lookup(
                    source_path,
                    name,
                    key,
                    &self.project.stores.fixture_definitions,
                )
                .cloned()
                .map(DawnObject::Fixture),
            ResolvedSourceObject::Patch(key) => self
                .lookup(source_path, name, key, &self.project.stores.patches)
                .and_then(|patch| self.patch_to_authored(source_path, imports, patch))
                .map(DawnObject::Patch),
            ResolvedSourceObject::Sequence(key) => self
                .lookup(source_path, name, key, &self.project.stores.sequences)
                .and_then(|sequence| self.sequence_to_authored(source_path, imports, sequence))
                .map(DawnObject::Sequence),
            ResolvedSourceObject::Curve(key) => self
                .lookup(source_path, name, key, &self.project.stores.curves)
                .cloned()
                .map(DawnObject::Curve),
            ResolvedSourceObject::Unused(object) => Some(object.clone()),
        }
    }

    fn project_to_authored(
        &mut self,
        source_path: &Utf8PathBuf,
        imports: &[DawnImport],
    ) -> Option<Project<Authored>> {
        let display =
            self.display_inline_or_ref_to_authored(source_path, imports, &self.project.display)?;
        let mut sequences = Vec::with_capacity(self.project.sequences.len());
        for sequence in &self.project.sequences {
            sequences.push(self.sequence_inline_or_ref_to_authored(
                source_path,
                imports,
                sequence,
            )?);
        }
        Some(Project {
            display,
            sequences,
            stores: NoProjectStores,
        })
    }

    fn display_to_authored(
        &mut self,
        source_path: &Utf8PathBuf,
        imports: &[DawnImport],
        display: &Display<Resolved>,
    ) -> Option<Display<Authored>> {
        let mut controllers = Vec::with_capacity(display.controllers.len());
        for controller in &display.controllers {
            controllers.push(self.controller_inline_or_ref_to_authored(
                source_path,
                imports,
                controller,
            )?);
        }
        Some(Display {
            controllers,
            patch: self.patch_inline_or_ref_to_authored(source_path, imports, &display.patch)?,
            layout: self.layout_inline_or_ref_to_authored(source_path, imports, &display.layout)?,
        })
    }

    fn layout_to_authored(
        &mut self,
        source_path: &Utf8PathBuf,
        imports: &[DawnImport],
        layout: &Layout<Resolved>,
    ) -> Option<Layout<Authored>> {
        let mut fixtures = Vec::with_capacity(layout.fixtures.len());
        for fixture in &layout.fixtures {
            fixtures.push(FixturePlacement {
                id: fixture.id,
                name: fixture.name.clone(),
                fixture: self.fixture_inline_or_ref_to_authored(
                    source_path,
                    imports,
                    &fixture.fixture,
                )?,
                transform: fixture.transform,
            });
        }
        Some(Layout {
            target_order: layout.target_order.clone(),
            fixtures,
            groups: layout
                .groups
                .iter()
                .map(|group| Group {
                    id: group.id,
                    name: group.name.clone(),
                    members: group.members.clone(),
                })
                .collect(),
        })
    }

    fn patch_to_authored(
        &mut self,
        source_path: &Utf8PathBuf,
        imports: &[DawnImport],
        patch: &Patch<Resolved>,
    ) -> Option<Patch<Authored>> {
        let mut routes = Vec::with_capacity(patch.routes.len());
        for route in &patch.routes {
            routes.push(Route {
                fixture: route.fixture,
                controller: self.symbol_to_authored(source_path, imports, &route.controller)?,
                universe: route.universe,
                start: route.start,
            });
        }
        Some(Patch { routes })
    }

    fn sequence_to_authored(
        &mut self,
        source_path: &Utf8PathBuf,
        imports: &[DawnImport],
        sequence: &Sequence<Resolved>,
    ) -> Option<Sequence<Authored>> {
        let mut effects = Vec::with_capacity(sequence.effects.len());
        for effect in &sequence.effects {
            effects.push(self.sequence_effect_to_authored(source_path, imports, effect)?);
        }
        let mut automation_clips = Vec::with_capacity(sequence.automation_clips.len());
        for clip in &sequence.automation_clips {
            let mut targets = Vec::with_capacity(clip.targets.len());
            for target in &clip.targets {
                targets.push(target.0);
            }
            automation_clips.push(AutomationClip {
                id: clip.id,
                start: clip.start,
                duration: clip.duration,
                curve: CurveUse {
                    id: clip.curve.id,
                    curve: self.curve_inline_or_ref_to_authored(
                        source_path,
                        imports,
                        &clip.curve.curve,
                    )?,
                },
                targets,
            });
        }
        Some(Sequence {
            duration: sequence.duration,
            frame_rate: sequence.frame_rate,
            audio: sequence.audio.as_ref().map(|audio| audio.source.clone()),
            mark_collections: sequence.mark_collections.clone(),
            effects,
            automation_clips,
        })
    }

    fn sequence_effect_to_authored(
        &mut self,
        source_path: &Utf8PathBuf,
        imports: &[DawnImport],
        effect: &SequenceEffect<Resolved>,
    ) -> Option<SequenceEffect<Authored>> {
        let mut params = IndexMap::with_capacity(effect.params.len());
        for (name, param) in &effect.params {
            params.insert(
                name.clone(),
                self.effect_param_to_authored(source_path, imports, param)?,
            );
        }
        Some(SequenceEffect {
            id: effect.id.0,
            start: effect.start,
            duration: effect.duration,
            target: match effect.target {
                EffectTarget::Group { id } => EffectTarget::Group { id },
                EffectTarget::Fixture { id } => EffectTarget::Fixture { id },
            },
            scope: effect.scope,
            params,
            script: self.symbol_to_authored(source_path, imports, &effect.script)?,
        })
    }

    fn effect_param_to_authored(
        &mut self,
        source_path: &Utf8PathBuf,
        imports: &[DawnImport],
        param: &EffectParam<Resolved>,
    ) -> Option<EffectParam<Authored>> {
        Some(match param {
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
                    curve: self.curve_inline_or_ref_to_authored(
                        source_path,
                        imports,
                        &curve.curve,
                    )?,
                },
            },
            EffectParam::Array {
                element_type,
                values,
            } => {
                let mut authored_values = Vec::with_capacity(values.len());
                for value in values {
                    authored_values.push(self.effect_param_array_value_to_authored(
                        source_path,
                        imports,
                        value,
                    )?);
                }
                EffectParam::Array {
                    element_type: *element_type,
                    values: authored_values,
                }
            }
            EffectParam::Marks { key } => EffectParam::Marks { key: key.clone() },
        })
    }

    fn effect_param_array_value_to_authored(
        &mut self,
        source_path: &Utf8PathBuf,
        imports: &[DawnImport],
        value: &EffectParamArrayValue<Resolved>,
    ) -> Option<EffectParamArrayValue<Authored>> {
        Some(match value {
            EffectParamArrayValue::Integer(value) => EffectParamArrayValue::Integer(*value),
            EffectParamArrayValue::Float(value) => EffectParamArrayValue::Float(*value),
            EffectParamArrayValue::Boolean(value) => EffectParamArrayValue::Boolean(*value),
            EffectParamArrayValue::Color(value) => EffectParamArrayValue::Color(*value),
            EffectParamArrayValue::Curve(curve) => EffectParamArrayValue::Curve(CurveUse {
                id: curve.id,
                curve: self.curve_inline_or_ref_to_authored(source_path, imports, &curve.curve)?,
            }),
        })
    }

    fn display_inline_or_ref_to_authored(
        &mut self,
        source_path: &Utf8PathBuf,
        imports: &[DawnImport],
        value: &ResolvedInlineOrRef<Display<Resolved>, DisplayDefinitionKey>,
    ) -> Option<InlineOrRef<Display<Authored>>> {
        self.inline_or_ref_to_authored(source_path, imports, value, |ctx, value| {
            ctx.display_to_authored(source_path, imports, value)
        })
    }

    fn sequence_inline_or_ref_to_authored(
        &mut self,
        source_path: &Utf8PathBuf,
        imports: &[DawnImport],
        value: &ResolvedInlineOrRef<Sequence<Resolved>, SequenceDefinitionKey>,
    ) -> Option<InlineOrRef<Sequence<Authored>>> {
        self.inline_or_ref_to_authored(source_path, imports, value, |ctx, value| {
            ctx.sequence_to_authored(source_path, imports, value)
        })
    }

    fn controller_inline_or_ref_to_authored(
        &mut self,
        source_path: &Utf8PathBuf,
        imports: &[DawnImport],
        value: &ResolvedInlineOrRef<Controller, ControllerDefinitionKey>,
    ) -> Option<InlineOrRef<Controller>> {
        self.inline_or_ref_to_authored(source_path, imports, value, |_ctx, value| {
            Some(value.clone())
        })
    }

    fn patch_inline_or_ref_to_authored(
        &mut self,
        source_path: &Utf8PathBuf,
        imports: &[DawnImport],
        value: &ResolvedInlineOrRef<Patch<Resolved>, PatchDefinitionKey>,
    ) -> Option<InlineOrRef<Patch<Authored>>> {
        self.inline_or_ref_to_authored(source_path, imports, value, |ctx, value| {
            ctx.patch_to_authored(source_path, imports, value)
        })
    }

    fn layout_inline_or_ref_to_authored(
        &mut self,
        source_path: &Utf8PathBuf,
        imports: &[DawnImport],
        value: &ResolvedInlineOrRef<Layout<Resolved>, LayoutDefinitionKey>,
    ) -> Option<InlineOrRef<Layout<Authored>>> {
        self.inline_or_ref_to_authored(source_path, imports, value, |ctx, value| {
            ctx.layout_to_authored(source_path, imports, value)
        })
    }

    fn fixture_inline_or_ref_to_authored(
        &mut self,
        source_path: &Utf8PathBuf,
        imports: &[DawnImport],
        value: &ResolvedInlineOrRef<Fixture, FixtureDefinitionKey>,
    ) -> Option<InlineOrRef<Fixture>> {
        self.inline_or_ref_to_authored(source_path, imports, value, |_ctx, value| {
            Some(value.clone())
        })
    }

    fn curve_inline_or_ref_to_authored(
        &mut self,
        source_path: &Utf8PathBuf,
        imports: &[DawnImport],
        value: &ResolvedInlineOrRef<Curve, CurveDefinitionKey>,
    ) -> Option<InlineOrRef<Curve>> {
        self.inline_or_ref_to_authored(source_path, imports, value, |_ctx, value| {
            Some(value.clone())
        })
    }

    fn inline_or_ref_to_authored<T, K, U>(
        &mut self,
        source_path: &Utf8PathBuf,
        imports: &[DawnImport],
        value: &ResolvedInlineOrRef<T, K>,
        inline: impl FnOnce(&mut Self, &T) -> Option<U>,
    ) -> Option<InlineOrRef<U>>
    where
        K: DefinitionKeyParts,
    {
        match value {
            ResolvedInlineOrRef::Inline(value) => inline(self, value).map(InlineOrRef::Inline),
            ResolvedInlineOrRef::Ref(reference) => {
                self.validate_symbol_ref(source_path, imports, reference)?;
                Some(InlineOrRef::Ref(reference.reference.clone()))
            }
        }
    }

    fn symbol_to_authored<K>(
        &mut self,
        source_path: &Utf8PathBuf,
        imports: &[DawnImport],
        reference: &ResolvedSymbolRef<K>,
    ) -> Option<SymbolRef>
    where
        K: DefinitionKeyParts,
    {
        self.validate_symbol_ref(source_path, imports, reference)?;
        Some(reference.reference.clone())
    }

    fn validate_symbol_ref<K>(
        &mut self,
        source_path: &Utf8PathBuf,
        imports: &[DawnImport],
        reference: &ResolvedSymbolRef<K>,
    ) -> Option<()>
    where
        K: DefinitionKeyParts,
    {
        if reference.reference.name().as_str() != reference.key.name() {
            self.missing(
                source_path,
                format!(
                    "reference `{}` no longer names resolved object `{}`",
                    reference.reference.raw(),
                    reference.key.name()
                ),
            );
            return None;
        }
        match reference.reference.alias() {
            None => {
                if reference.key.path() != source_path {
                    self.missing(
                        source_path,
                        format!(
                            "local reference `{}` resolves to `{}`",
                            reference.reference.raw(),
                            reference.key.path()
                        ),
                    );
                    return None;
                }
            }
            Some(alias) => {
                let matching = imports
                    .iter()
                    .filter(|import| import.alias == alias)
                    .collect::<Vec<_>>();
                if matching.len() != 1 {
                    self.missing(
                        source_path,
                        format!(
                            "reference `{}` requires exactly one source import alias `{alias}`",
                            reference.reference.raw()
                        ),
                    );
                    return None;
                }
                let import_path =
                    canonicalize_path(&resolve_import_path(source_path, &matching[0].from));
                if import_path != *reference.key.path()
                    && !reference.key.path().starts_with(&import_path)
                {
                    self.missing(
                        source_path,
                        format!(
                            "reference `{}` resolves to `{}`, but alias `{alias}` imports `{}`",
                            reference.reference.raw(),
                            reference.key.path(),
                            matching[0].from
                        ),
                    );
                    return None;
                }
            }
        }
        Some(())
    }

    fn lookup<'a, K, T>(
        &mut self,
        source_path: &Utf8PathBuf,
        name: &str,
        key: &K,
        store: &'a IndexMap<K, ResolvedObject<T>>,
    ) -> Option<&'a T>
    where
        K: Eq + std::hash::Hash,
    {
        store.get(key).map(|object| &object.value).or_else(|| {
            self.missing(
                source_path,
                format!("source object `{name}` has no resolved store entry"),
            );
            None
        })
    }

    fn missing(&mut self, path: &Utf8PathBuf, message: String) {
        self.diagnostics.push(diagnostic(
            path.clone(),
            message,
            ProjectDiagnosticKind::Lower,
        ));
    }
}

trait DefinitionKeyParts {
    fn path(&self) -> &Utf8PathBuf;
    fn name(&self) -> &str;
}

macro_rules! impl_definition_key_parts {
    ($type:ty) => {
        impl DefinitionKeyParts for $type {
            fn path(&self) -> &Utf8PathBuf {
                &self.path
            }

            fn name(&self) -> &str {
                &self.name
            }
        }
    };
}

impl_definition_key_parts!(DisplayDefinitionKey);
impl_definition_key_parts!(SequenceDefinitionKey);
impl_definition_key_parts!(ControllerDefinitionKey);
impl_definition_key_parts!(PatchDefinitionKey);
impl_definition_key_parts!(LayoutDefinitionKey);
impl_definition_key_parts!(FixtureDefinitionKey);
impl_definition_key_parts!(CurveDefinitionKey);
impl_definition_key_parts!(EffectDefinitionKey);

fn diagnostic(
    file: Utf8PathBuf,
    message: String,
    kind: ProjectDiagnosticKind,
) -> ProjectDiagnostic {
    ProjectDiagnostic {
        severity: DiagnosticSeverity::Error,
        file,
        range: None::<TextRange>,
        message,
        kind,
    }
}
