mod parse;
mod resolve;

use parse::{
    AliasObjectKey, ResolvedObject, SourceObjectValue, parse_curve, parse_gradient,
    parse_prop_definition, sequence_field, string_field,
};
pub(crate) use parse::{mapping, normalize_relative, parse_imports, relative_path};
use resolve::DomainResolver;

pub(super) struct Loader {
    pub(crate) source_root: Utf8PathBuf,
    pub(crate) entrypoint: Utf8PathBuf,
    pub(crate) documents: IndexMap<Utf8PathBuf, SourceDocument>,
    pub(crate) visible_objects: IndexMap<Utf8PathBuf, IndexMap<AliasObjectKey, ResolvedObject>>,
    pub(crate) loading_documents: IndexSet<Utf8PathBuf>,
    pub(crate) definitions: ProjectDefinitionStores,
    pub(crate) referenced_assets: Vec<ReferencedAsset>,
    pub(crate) next_asset_id: u32,
    pub(crate) source_overrides: IndexMap<Utf8PathBuf, String>,
}

impl Loader {
    pub(super) fn new(path: &Utf8Path) -> Result<Self, LoadProjectError> {
        let absolute = path
            .canonicalize_utf8()
            .map_err(|source| LoadProjectError::Io {
                path: path.to_path_buf(),
                source,
            })?;
        if !absolute.is_file() {
            return Err(LoadProjectError::InvalidEntrypoint { path: absolute });
        }
        let source_root = absolute
            .parent()
            .ok_or_else(|| LoadProjectError::InvalidEntrypoint {
                path: absolute.clone(),
            })?
            .to_path_buf();
        let entrypoint = relative_path(&source_root, &absolute)?;
        Ok(Self {
            source_root,
            entrypoint,
            documents: IndexMap::new(),
            visible_objects: IndexMap::new(),
            loading_documents: IndexSet::new(),
            definitions: ProjectDefinitionStores::default(),
            referenced_assets: Vec::new(),
            next_asset_id: 1,
            source_overrides: IndexMap::new(),
        })
    }

    pub(super) fn load(mut self) -> Result<ProjectSession, LoadProjectError> {
        let entrypoint = self.entrypoint.clone();
        self.load_document(&entrypoint)?;
        let project = self.resolve_project(&entrypoint)?;
        let entrypoint = self.entrypoint.clone();
        Ok(ProjectSession {
            project,
            source: SourceProject {
                source_root: self.source_root,
                entrypoint,
                documents: self.documents,
                referenced_assets: self.referenced_assets,
            },
        })
    }

    pub(super) fn load_document(&mut self, relative: &Utf8Path) -> Result<(), LoadProjectError> {
        if self.documents.contains_key(relative) {
            return Ok(());
        }
        if self.loading_documents.contains(relative) {
            // Import cycles are valid: each Dawn document indexes its local objects
            // before resolving imports, so the active document is already visible.
            return Ok(());
        }
        self.loading_documents.insert(relative.to_path_buf());
        let absolute = self.source_root.join(relative);
        let file_name = relative.file_name().unwrap_or_default();
        let result = if file_name.ends_with(".effect.dawn") {
            self.load_effect_document(relative, &absolute)
        } else if file_name.ends_with(".operator.dawn") {
            self.load_operator_document(relative, &absolute)
        } else {
            self.load_dawn_document(relative, &absolute)
        };
        self.loading_documents.shift_remove(relative);
        result
    }

    pub(super) fn read_source(
        &self,
        relative: &Utf8Path,
        absolute: &Utf8Path,
    ) -> Result<String, LoadProjectError> {
        if let Some(source) = self.source_overrides.get(relative) {
            return Ok(source.clone());
        }
        fs::read_to_string(absolute).map_err(|source| LoadProjectError::Io {
            path: absolute.to_path_buf(),
            source,
        })
    }

    pub(super) fn load_effect_document(
        &mut self,
        relative: &Utf8Path,
        absolute: &Utf8Path,
    ) -> Result<(), LoadProjectError> {
        let source = self.read_source(relative, absolute)?;
        let compiled =
            compile_effects(&source).map_err(|diagnostics| LoadProjectError::InvalidEffect {
                path: relative.to_path_buf(),
                diagnostics: diagnostics
                    .into_iter()
                    .map(|diagnostic| {
                        dsl_diagnostic(
                            relative,
                            &source,
                            diagnostic,
                            IoDiagnosticCode::EffectCompile,
                        )
                    })
                    .collect(),
            })?;
        let mut visible = IndexMap::new();
        let mut objects = Vec::new();
        for effect in compiled {
            let name = effect.name().as_str().to_string();
            let id = EffectDefinitionId(SourceIdentity::new(relative.to_path_buf(), name.clone()));
            self.definitions
                .effects
                .insert(id.clone(), EffectDefinition::custom(id.clone(), effect));
            let source_object = SourceObjectId {
                kind: SourceObjectKind::EffectDefinition,
                id: name.clone(),
            };
            objects.push(source_object);
            visible.insert(
                AliasObjectKey {
                    alias: None,
                    object: name.clone(),
                },
                ResolvedObject::EffectDefinition(id),
            );
        }
        self.visible_objects.insert(relative.to_path_buf(), visible);
        let document =
            SourceDocument::new(Vec::new(), objects, SourceDocumentKind::Effect { source })
                .map_err(|message| LoadProjectError::InvalidDocument {
                    path: relative.to_path_buf(),
                    range: None,
                    message,
                })?;
        self.documents.insert(relative.to_path_buf(), document);
        Ok(())
    }

    pub(super) fn load_operator_document(
        &mut self,
        relative: &Utf8Path,
        absolute: &Utf8Path,
    ) -> Result<(), LoadProjectError> {
        let source = self.read_source(relative, absolute)?;
        let compiled = compile_operators(&source).map_err(|diagnostics| {
            LoadProjectError::InvalidOperator {
                path: relative.to_path_buf(),
                diagnostics: diagnostics
                    .into_iter()
                    .map(|diagnostic| {
                        dsl_diagnostic(
                            relative,
                            &source,
                            diagnostic,
                            IoDiagnosticCode::OperatorCompile,
                        )
                    })
                    .collect(),
            }
        })?;
        let mut visible = IndexMap::new();
        let mut objects = Vec::new();
        for operator in compiled {
            let name = operator.name().as_str().to_string();
            let id =
                OperatorDefinitionId(SourceIdentity::new(relative.to_path_buf(), name.clone()));
            let definition = custom_operator_definition(id.clone(), operator);
            self.definitions.operators.insert(id.clone(), definition);
            let source_object = SourceObjectId {
                kind: SourceObjectKind::OperatorDefinition,
                id: name.clone(),
            };
            objects.push(source_object);
            visible.insert(
                AliasObjectKey {
                    alias: None,
                    object: name.clone(),
                },
                ResolvedObject::OperatorDefinition(id),
            );
        }
        self.visible_objects.insert(relative.to_path_buf(), visible);
        let document =
            SourceDocument::new(Vec::new(), objects, SourceDocumentKind::Operator { source })
                .map_err(|message| LoadProjectError::InvalidDocument {
                    path: relative.to_path_buf(),
                    range: None,
                    message,
                })?;
        self.documents.insert(relative.to_path_buf(), document);
        Ok(())
    }

    pub(super) fn load_dawn_document(
        &mut self,
        relative: &Utf8Path,
        absolute: &Utf8Path,
    ) -> Result<(), LoadProjectError> {
        let text = self.read_source(relative, absolute)?;
        let value = parse_yaml_value(relative, &text)?;
        let map = mapping(&value).ok_or_else(|| LoadProjectError::InvalidDocument {
            path: relative.to_path_buf(),
            range: None,
            message: "document root must be a mapping".to_string(),
        })?;
        let imports = parse_imports(relative, map)?;
        let mut visible = IndexMap::new();

        let mut objects = Vec::new();
        for (key, object_value) in map {
            let Some(key) = key.as_str() else {
                return Err(LoadProjectError::InvalidDocument {
                    path: relative.to_path_buf(),
                    range: None,
                    message: "object keys must be strings".to_string(),
                });
            };
            if key == "imports" {
                continue;
            }
            Identifier::new(key.to_string()).map_err(|_| LoadProjectError::InvalidDocument {
                path: relative.to_path_buf(),
                range: source_range_for_value(relative, object_value),
                message: format!("invalid object identifier `{key}`"),
            })?;
            let object_type = string_field(relative, object_value, "type")?;
            let object =
                match object_type {
                    "project" => ResolvedObject::Project(ProjectId(SourceIdentity::new(
                        relative.to_path_buf(),
                        key.to_string(),
                    ))),
                    "setup" => ResolvedObject::Setup(SetupId(SourceIdentity::new(
                        relative.to_path_buf(),
                        key.to_string(),
                    ))),
                    "controller" => ResolvedObject::Controller(ControllerId(SourceIdentity::new(
                        relative.to_path_buf(),
                        key.to_string(),
                    ))),
                    "element_tree" => ResolvedObject::ElementTree(ElementTreeId(
                        SourceIdentity::new(relative.to_path_buf(), key.to_string()),
                    )),
                    "preview_layout" => ResolvedObject::PreviewLayout(PreviewLayoutId(
                        SourceIdentity::new(relative.to_path_buf(), key.to_string()),
                    )),
                    "patch" => ResolvedObject::Patch(PatchId(SourceIdentity::new(
                        relative.to_path_buf(),
                        key.to_string(),
                    ))),
                    "prop" => ResolvedObject::PropDefinition(PropDefinitionId(
                        SourceIdentity::new(relative.to_path_buf(), key.to_string()),
                    )),
                    "fixture_profile" => ResolvedObject::FixtureProfile(FixtureProfileId(
                        SourceIdentity::new(relative.to_path_buf(), key.to_string()),
                    )),
                    "curve" => ResolvedObject::Curve(CurveId(SourceIdentity::new(
                        relative.to_path_buf(),
                        key.to_string(),
                    ))),
                    "gradient" => ResolvedObject::Gradient(GradientId(SourceIdentity::new(
                        relative.to_path_buf(),
                        key.to_string(),
                    ))),
                    "sequence" => ResolvedObject::Sequence(SequenceId(SourceIdentity::new(
                        relative.to_path_buf(),
                        key.to_string(),
                    ))),
                    other => {
                        return Err(LoadProjectError::InvalidDocument {
                            path: relative.to_path_buf(),
                            range: source_range_for_field_value(relative, object_value, "type"),
                            message: format!("unsupported object type `{other}`"),
                        });
                    }
                };
            if let ResolvedObject::Curve(id) = &object {
                self.definitions.curves.insert(
                    id.clone(),
                    CurveDefinition {
                        curve: parse_curve(relative, object_value)?,
                    },
                );
            }
            if let ResolvedObject::Gradient(id) = &object {
                self.definitions.gradients.insert(
                    id.clone(),
                    GradientDefinition {
                        gradient: parse_gradient(relative, object_value)?,
                    },
                );
            }
            if let ResolvedObject::PropDefinition(id) = &object {
                self.definitions
                    .props
                    .definitions
                    .insert(id.clone(), parse_prop_definition(relative, object_value)?);
            }
            let source_object = SourceObjectId {
                kind: object.source_kind(),
                id: key.to_string(),
            };
            objects.push(source_object);
            visible.insert(
                AliasObjectKey {
                    alias: None,
                    object: key.to_string(),
                },
                object,
            );
        }

        self.visible_objects.insert(relative.to_path_buf(), visible);

        let mut import_edges = Vec::new();
        let mut imported_visible = Vec::new();
        let mut import_aliases = IndexSet::new();
        let mut imported_targets = IndexSet::new();
        for import in &imports {
            if import.alias == "builtins" {
                return Err(LoadProjectError::InvalidDocument {
                    path: relative.to_path_buf(),
                    range: None,
                    message: "import alias `builtins` is reserved".to_string(),
                });
            }
            if !import_aliases.insert(import.alias.clone()) {
                return Err(LoadProjectError::InvalidDocument {
                    path: relative.to_path_buf(),
                    range: None,
                    message: format!("duplicate import alias `{}`", import.alias),
                });
            }
            let targets = self.resolve_import(relative, &import.from)?;
            let mut names = IndexSet::new();
            for target in &targets {
                if !imported_targets.insert(target.clone()) {
                    return Err(LoadProjectError::InvalidDocument {
                        path: relative.to_path_buf(),
                        range: None,
                        message: format!(
                            "document `{target}` is imported more than once; each target must have one alias"
                        ),
                    });
                }
                self.load_document(target)?;
                let target_visible = self.visible_objects.get(target).ok_or_else(|| {
                    LoadProjectError::InvalidDocument {
                        path: target.clone(),
                        range: None,
                        message: "loaded document had no visible object index".to_string(),
                    }
                })?;
                for (key, object) in target_visible {
                    if key.alias.is_some() {
                        continue;
                    }
                    if !names.insert(key.object.clone()) {
                        return Err(LoadProjectError::InvalidDocument {
                            path: relative.to_path_buf(),
                            range: None,
                            message: format!(
                                "duplicate exported object `{}` in import alias `{}`",
                                key.object, import.alias
                            ),
                        });
                    }
                    imported_visible.push((
                        AliasObjectKey {
                            alias: Some(import.alias.clone()),
                            object: key.object.clone(),
                        },
                        object.clone(),
                    ));
                }
            }
            import_edges.push(ImportEdge {
                alias: import.alias.clone(),
                from: import.from.clone(),
                targets,
            });
        }
        if let Some(visible) = self.visible_objects.get_mut(relative) {
            visible.extend(imported_visible);
        }

        let document = SourceDocument::new(
            import_edges,
            objects,
            SourceDocumentKind::Dawn {
                original_value: value,
            },
        )
        .map_err(|message| LoadProjectError::InvalidDocument {
            path: relative.to_path_buf(),
            range: None,
            message,
        })?;
        self.documents.insert(relative.to_path_buf(), document);
        Ok(())
    }

    pub(super) fn resolve_import(
        &self,
        importer: &Utf8Path,
        import_from: &Utf8Path,
    ) -> Result<Vec<Utf8PathBuf>, LoadProjectError> {
        if import_from.is_absolute() {
            return Err(LoadProjectError::InvalidDocument {
                path: importer.to_path_buf(),
                range: None,
                message: "import paths must be relative".to_string(),
            });
        }
        let importer_dir = importer.parent().unwrap_or_else(|| Utf8Path::new(""));
        let target_relative = normalize_relative(importer_dir.join(import_from));
        let absolute = self.source_root.join(&target_relative);
        if absolute.is_dir() {
            let mut children = Vec::new();
            for entry in fs::read_dir(&absolute).map_err(|source| LoadProjectError::Io {
                path: absolute.clone(),
                source,
            })? {
                let entry = entry.map_err(|source| LoadProjectError::Io {
                    path: absolute.clone(),
                    source,
                })?;
                let path = Utf8PathBuf::from_path_buf(entry.path()).map_err(|path| {
                    LoadProjectError::InvalidDocument {
                        path: absolute.clone(),
                        range: None,
                        message: format!("non-utf8 import child path `{}`", path.display()),
                    }
                })?;
                if path.is_file() && path.file_name().is_some_and(|name| name.ends_with(".dawn")) {
                    children.push(relative_path(&self.source_root, &path)?);
                }
            }
            children.sort();
            return Ok(children);
        }
        if absolute.is_file() {
            return Ok(vec![target_relative]);
        }
        Err(LoadProjectError::InvalidDocument {
            path: importer.to_path_buf(),
            range: None,
            message: format!("import target does not exist: {import_from}"),
        })
    }

    pub(super) fn resolve_project(
        &mut self,
        entrypoint: &Utf8Path,
    ) -> Result<DawnProject, LoadProjectError> {
        let root_object = self.single_project_object(entrypoint)?;
        let root_id = ProjectId(SourceIdentity::new(
            entrypoint.to_path_buf(),
            Identifier::new(root_object.key.clone())
                .map_err(|_| LoadProjectError::InvalidDocument {
                    path: entrypoint.to_path_buf(),
                    range: None,
                    message: "project object key is not a valid identifier".to_string(),
                })?
                .as_str()
                .to_string(),
        ));
        let setup = self.reference_as_setup(
            entrypoint,
            string_field(entrypoint, root_object.value, "setup")?,
        )?;
        let sequences = sequence_field(entrypoint, root_object.value, "sequences")?
            .iter()
            .map(|reference| self.reference_as_sequence(entrypoint, reference))
            .collect::<Result<Vec<_>, _>>()?;

        let mut project = DawnProject {
            root: ProjectRoot {
                id: root_id,
                setup: setup.clone(),
                sequences: sequences.clone(),
            },
            setups: IndexMap::new(),
            element_trees: IndexMap::new(),
            preview_layouts: IndexMap::new(),
            patches: IndexMap::new(),
            controllers: IndexMap::new(),
            sequences: IndexMap::new(),
            definitions: ProjectDefinitionStores::default(),
        };
        project.definitions = self.definitions.clone();

        let mut resolver = DomainResolver {
            loader: self,
            project: &mut project,
        };
        resolver.resolve_setup(entrypoint, &setup)?;
        for sequence in sequences {
            resolver.resolve_sequence(&sequence)?;
        }
        dawn_language::validation::validate_project(&project).map_err(|error| {
            LoadProjectError::InvalidDocument {
                path: entrypoint.to_path_buf(),
                range: None,
                message: format!("project validation failed: {error:?}"),
            }
        })?;
        Ok(project)
    }

    fn single_project_object<'a>(
        &'a self,
        path: &Utf8Path,
    ) -> Result<SourceObjectValue<'a>, LoadProjectError> {
        let document = self.dawn_document(path)?;
        let mut found = None;
        let document_map = mapping(document).ok_or_else(|| LoadProjectError::InvalidDocument {
            path: path.to_path_buf(),
            range: None,
            message: "document root must be a mapping".to_string(),
        })?;
        for (key, value) in document_map.iter() {
            let Some(key) = key.as_str() else {
                continue;
            };
            if key == "imports" {
                continue;
            }
            if string_field(path, value, "type")? == "project" {
                found = Some(SourceObjectValue {
                    key: key.to_string(),
                    value,
                });
                break;
            }
        }
        found.ok_or_else(|| LoadProjectError::InvalidDocument {
            path: path.to_path_buf(),
            range: None,
            message: "entrypoint must contain a project object".to_string(),
        })
    }

    pub(super) fn object_value(
        &self,
        id: &ResolvedObject,
    ) -> Result<(Utf8PathBuf, String, Value), LoadProjectError> {
        let identity = id.source_identity();
        if !self
            .documents
            .get(identity.document())
            .is_some_and(|document| {
                document
                    .objects
                    .iter()
                    .any(|object| object.kind == id.source_kind() && object.id == identity.object())
            })
        {
            return Err(LoadProjectError::InvalidReference {
                path: self.entrypoint.clone(),
                range: None,
                reference: id.id_string(),
            });
        }
        let document = self.dawn_document(identity.document())?;
        let document_map = mapping(document).ok_or_else(|| LoadProjectError::InvalidDocument {
            path: identity.document().to_path_buf(),
            range: None,
            message: "document root must be a mapping".to_string(),
        })?;
        let value = document_map
            .get(Value::String(identity.object().to_string()))
            .ok_or_else(|| LoadProjectError::InvalidReference {
                path: identity.document().to_path_buf(),
                range: None,
                reference: identity.object().to_string(),
            })?
            .clone();
        Ok((
            identity.document().to_path_buf(),
            identity.object().to_string(),
            value,
        ))
    }

    fn dawn_document<'a>(&'a self, path: &Utf8Path) -> Result<&'a Value, LoadProjectError> {
        let document =
            self.documents
                .get(path)
                .ok_or_else(|| LoadProjectError::InvalidDocument {
                    path: path.to_path_buf(),
                    range: None,
                    message: "document not loaded".to_string(),
                })?;
        match &document.kind {
            SourceDocumentKind::Dawn { original_value } => Ok(original_value),
            SourceDocumentKind::Effect { .. } => Err(LoadProjectError::InvalidDocument {
                path: path.to_path_buf(),
                range: None,
                message: "expected YAML Dawn document".to_string(),
            }),
            SourceDocumentKind::Operator { .. } => Err(LoadProjectError::InvalidDocument {
                path: path.to_path_buf(),
                range: None,
                message: "expected YAML Dawn document".to_string(),
            }),
        }
    }

    pub(super) fn resolve_reference(
        &self,
        path: &Utf8Path,
        reference: &str,
    ) -> Result<ResolvedObject, LoadProjectError> {
        let range = source_range_for_scalar(path, reference);
        let (alias, object) = reference
            .split_once('.')
            .map_or((None, reference), |(alias, object)| (Some(alias), object));
        self.visible_objects
            .get(path)
            .and_then(|visible| {
                visible.get(&AliasObjectKey {
                    alias: alias.map(ToString::to_string),
                    object: object.to_string(),
                })
            })
            .cloned()
            .ok_or_else(|| LoadProjectError::InvalidReference {
                path: path.to_path_buf(),
                range,
                reference: reference.to_string(),
            })
    }

    pub(super) fn reference_as_setup(
        &self,
        path: &Utf8Path,
        reference: &str,
    ) -> Result<SetupId, LoadProjectError> {
        match self.resolve_reference(path, reference)? {
            ResolvedObject::Setup(id) => Ok(id),
            _ => Err(LoadProjectError::InvalidReference {
                path: path.to_path_buf(),
                range: source_range_for_scalar(path, reference),
                reference: reference.to_string(),
            }),
        }
    }

    pub(super) fn reference_as_sequence(
        &self,
        path: &Utf8Path,
        reference: &str,
    ) -> Result<SequenceId, LoadProjectError> {
        match self.resolve_reference(path, reference)? {
            ResolvedObject::Sequence(id) => Ok(id),
            _ => Err(LoadProjectError::InvalidReference {
                path: path.to_path_buf(),
                range: source_range_for_scalar(path, reference),
                reference: reference.to_string(),
            }),
        }
    }
}
use std::fs;

use camino::{Utf8Path, Utf8PathBuf};
use dawn_language::controller::ControllerId;
use dawn_language::dsl::{Identifier, compile_effects, compile_operators};
use dawn_language::effect::{
    CurveDefinition, CurveId, EffectDefinition, EffectDefinitionId, GradientDefinition, GradientId,
};
use dawn_language::element::ElementTreeId;
use dawn_language::fixture_profile::FixtureProfileId;
use dawn_language::identity::SourceIdentity;
use dawn_language::model::{DawnProject, ProjectDefinitionStores, ProjectId, ProjectRoot};
use dawn_language::operator::{OperatorDefinitionId, custom_operator_definition};
use dawn_language::patch::PatchId;
use dawn_language::preview::{PreviewLayoutId, PropDefinitionId};
use dawn_language::sequence::SequenceId;
use dawn_language::setup::SetupId;
use indexmap::{IndexMap, IndexSet};
use yaml_serde::Value;

use crate::diagnostics::{
    dsl_diagnostic, parse_yaml_value, source_range_for_field_value, source_range_for_scalar,
    source_range_for_value,
};
use crate::source::{
    ImportEdge, ProjectSession, ReferencedAsset, SourceDocument, SourceDocumentKind,
    SourceObjectId, SourceObjectKind, SourceProject,
};
use crate::{IoDiagnosticCode, LoadProjectError};
