mod parse;
mod resolve;

pub(crate) use parse::mapping;
use parse::{
    AliasObjectKey, ParsedImportSource, ResolvedObject, SourceObjectValue, parse_curve,
    parse_gradient, parse_imports, parse_prop_definition, sequence_field, string_field,
};
use resolve::DomainResolver;

pub(super) struct Loader {
    pub(crate) source_graph: dawn_package::ResolvedSourceGraph,
    pub(crate) entrypoint: Option<dawn_language::identity::DocumentId>,
    pub(crate) documents: IndexMap<dawn_language::identity::DocumentId, SourceDocument>,
    pub(crate) visible_objects:
        IndexMap<dawn_language::identity::DocumentId, IndexMap<AliasObjectKey, ResolvedObject>>,
    pub(crate) loading_documents: IndexSet<dawn_language::identity::DocumentId>,
    pub(crate) definitions: ProjectDefinitionStores,
    pub(crate) referenced_assets: Vec<ReferencedAsset>,
    pub(crate) next_asset_id: u32,
    pub(crate) source_overrides: IndexMap<dawn_language::identity::DocumentId, String>,
    pub(crate) operator_reconciliation:
        Option<BTreeMap<dawn_language::identity::DocumentId, Vec<OperatorDefinitionId>>>,
}

impl Loader {
    pub(super) fn new(
        source_graph: dawn_package::ResolvedSourceGraph,
    ) -> Result<Self, LoadProjectError> {
        source_graph
            .validate()
            .map_err(|error| LoadProjectError::InvalidDocument {
                path: Utf8PathBuf::from(dawn_package::MANIFEST_FILE),
                range: None,
                message: error.to_string(),
            })?;
        let project_module = source_graph
            .module(source_graph.project_module_id())
            .map_err(|error| LoadProjectError::InvalidDocument {
                path: Utf8PathBuf::from(dawn_package::MANIFEST_FILE),
                range: None,
                message: error.to_string(),
            })?;
        let entrypoint = project_module.manifest.project.as_ref().map(|project| {
            dawn_language::identity::DocumentId::new(
                source_graph.project_module_id(),
                Utf8PathBuf::from(&project.entrypoint),
            )
        });
        if let Some(entrypoint) = &entrypoint {
            let absolute = project_module.root.join(entrypoint.path());
            if !absolute.is_file() {
                return Err(LoadProjectError::InvalidEntrypoint { path: absolute });
            }
        }
        Ok(Self {
            source_graph,
            entrypoint,
            documents: IndexMap::new(),
            visible_objects: IndexMap::new(),
            loading_documents: IndexSet::new(),
            definitions: ProjectDefinitionStores::default(),
            referenced_assets: Vec::new(),
            next_asset_id: 1,
            source_overrides: IndexMap::new(),
            operator_reconciliation: None,
        })
    }

    pub(super) fn for_operator_reconciliation(
        source_graph: dawn_package::ResolvedSourceGraph,
        previous: &dawn_language::operator::OperatorDefinitionStore,
    ) -> Result<Self, LoadProjectError> {
        let mut loader = Self::new(source_graph)?;
        let mut definitions = BTreeMap::<_, Vec<_>>::new();
        for id in previous.definitions.keys() {
            definitions
                .entry(id.0.document_id().clone())
                .or_default()
                .push(id.clone());
        }
        for document_definitions in definitions.values_mut() {
            document_definitions.sort_by(|left, right| left.0.object().cmp(right.0.object()));
        }
        loader.operator_reconciliation = Some(definitions);
        Ok(loader)
    }

    pub(super) fn source_identity(
        &self,
        document: &dawn_language::identity::DocumentId,
        object: String,
    ) -> dawn_language::identity::SourceIdentity {
        dawn_language::identity::SourceIdentity::from_document(document.clone(), object)
    }

    pub(super) fn load(self) -> Result<ProjectSession, LoadProjectError> {
        let compiled = self.compile()?;
        let project = compiled
            .project
            .ok_or_else(|| LoadProjectError::InvalidEntrypoint {
                path: compiled
                    .source
                    .project_root()
                    .join(dawn_package::MANIFEST_FILE),
            })?;
        Ok(ProjectSession {
            project,
            source: compiled.source,
        })
    }

    pub(super) fn compile(mut self) -> Result<crate::CompiledSourceGraph, LoadProjectError> {
        let mut roots = std::collections::BTreeSet::new();
        for (module_id, module) in self.source_graph.modules() {
            for export in module.manifest.exports.values() {
                for document in &export.documents {
                    roots.insert(dawn_language::identity::DocumentId::new(
                        *module_id,
                        Utf8PathBuf::from(document),
                    ));
                }
            }
        }
        if let Some(entrypoint) = &self.entrypoint {
            roots.insert(entrypoint.clone());
        }
        for root in &roots {
            self.load_document(root)?;
        }

        let mut typed = if let Some(entrypoint) = self.entrypoint.clone() {
            self.resolve_project(&entrypoint)?
        } else {
            self.scratch_project(roots.iter().next().cloned().unwrap_or_else(|| {
                dawn_language::identity::DocumentId::new(
                    self.source_graph.project_module_id(),
                    Utf8PathBuf::from(dawn_package::MANIFEST_FILE),
                )
            }))
        };
        self.validate_exported_objects(&mut typed)?;
        if self.entrypoint.is_some() {
            let entrypoint =
                self.entrypoint
                    .as_ref()
                    .ok_or_else(|| LoadProjectError::InvalidEntrypoint {
                        path: self
                            .source_graph
                            .project_module()
                            .root
                            .join(dawn_package::MANIFEST_FILE),
                    })?;
            dawn_language::validation::validate_project(&typed).map_err(|error| {
                LoadProjectError::InvalidDocument {
                    path: entrypoint.path().to_path_buf(),
                    range: None,
                    message: format!("project validation failed: {error:?}"),
                }
            })?;
        }
        let project = self.entrypoint.as_ref().map(|_| typed.clone());
        let definitions = typed.definitions.clone();
        Ok(crate::CompiledSourceGraph {
            source: SourceProject {
                source_graph: self.source_graph,
                entrypoint: self.entrypoint,
                documents: self.documents,
                referenced_assets: self.referenced_assets,
            },
            project,
            definitions,
        })
    }

    fn scratch_project(&self, document: dawn_language::identity::DocumentId) -> DawnProject {
        let project_identity = dawn_language::identity::SourceIdentity::from_document(
            document.clone(),
            "__package_compile_project".to_string(),
        );
        let setup_identity = dawn_language::identity::SourceIdentity::from_document(
            document,
            "__package_compile_setup".to_string(),
        );
        DawnProject {
            root: ProjectRoot {
                id: ProjectId(project_identity),
                setup: SetupId(setup_identity),
                sequences: Vec::new(),
            },
            setups: IndexMap::new(),
            element_trees: IndexMap::new(),
            preview_layouts: IndexMap::new(),
            patches: IndexMap::new(),
            controllers: IndexMap::new(),
            sequences: IndexMap::new(),
            definitions: self.definitions.clone(),
        }
    }

    fn validate_exported_objects(
        &mut self,
        project: &mut DawnProject,
    ) -> Result<(), LoadProjectError> {
        let mut exported = Vec::new();
        let mut seen = std::collections::BTreeSet::new();
        for (module_id, module) in self.source_graph.modules() {
            for group in module.manifest.exports.values() {
                for document in &group.documents {
                    let document_id = dawn_language::identity::DocumentId::new(
                        *module_id,
                        Utf8PathBuf::from(document),
                    );
                    let visible = self.visible_objects.get(&document_id).ok_or_else(|| {
                        LoadProjectError::InvalidDocument {
                            path: document_id.path().to_path_buf(),
                            range: None,
                            message: "export document was not compiled".to_string(),
                        }
                    })?;
                    for (key, object) in visible {
                        if key.alias.is_none()
                            && seen.insert((document_id.clone(), key.object.clone()))
                        {
                            exported.push((document_id.clone(), object.clone()));
                        }
                    }
                }
            }
        }

        let active_entrypoint = self.entrypoint.clone();
        let mut resolver = DomainResolver {
            loader: self,
            project,
        };
        for (document, object) in exported {
            match object {
                ResolvedObject::Project(id) => {
                    if active_entrypoint.as_ref() != Some(&document)
                        || resolver.project.root.id != id
                    {
                        return Err(LoadProjectError::InvalidDocument {
                            path: document.path().to_path_buf(),
                            range: None,
                            message:
                                "exported project objects must be the active manifest entrypoint"
                                    .to_string(),
                        });
                    }
                }
                ResolvedObject::Setup(id) => {
                    resolver.resolve_setup(&document, &id)?;
                }
                ResolvedObject::Controller(id) => {
                    resolver.resolve_controller(&id)?;
                }
                ResolvedObject::ElementTree(id) => {
                    resolver.resolve_element_tree(&id)?;
                }
                ResolvedObject::PreviewLayout(id) => {
                    resolver.resolve_preview_layout(&id)?;
                }
                ResolvedObject::Patch(id) => {
                    resolver.resolve_patch(&id)?;
                }
                ResolvedObject::PropDefinition(id) => {
                    if !resolver
                        .project
                        .definitions
                        .props
                        .definitions
                        .contains_key(&id)
                    {
                        return Err(missing_exported_definition(&document, &id.0));
                    }
                }
                ResolvedObject::FixtureProfile(id) => {
                    resolver.resolve_fixture_profile(&id)?;
                }
                ResolvedObject::Curve(id) => {
                    resolver.resolve_curve(document.path(), &id)?;
                }
                ResolvedObject::Gradient(id) => {
                    if !resolver
                        .project
                        .definitions
                        .gradients
                        .definitions
                        .contains_key(&id)
                    {
                        return Err(missing_exported_definition(&document, &id.0));
                    }
                }
                ResolvedObject::Sequence(id) => {
                    resolver.resolve_sequence(&id)?;
                }
                ResolvedObject::EffectDefinition(id) => {
                    resolver.resolve_effect_definition(&id)?;
                }
                ResolvedObject::OperatorDefinition(id) => {
                    if !resolver
                        .project
                        .definitions
                        .operators
                        .definitions
                        .contains_key(&id)
                    {
                        return Err(missing_exported_definition(&document, &id.0));
                    }
                }
            }
        }
        Ok(())
    }

    pub(super) fn load_document(
        &mut self,
        document_id: &dawn_language::identity::DocumentId,
    ) -> Result<(), LoadProjectError> {
        if self.documents.contains_key(document_id) {
            return Ok(());
        }
        if self.loading_documents.contains(document_id) {
            // Import cycles are valid: each Dawn document indexes its local objects
            // before resolving imports, so the active document is already visible.
            return Ok(());
        }
        self.loading_documents.insert(document_id.clone());
        let absolute = self.absolute_document_path(document_id)?;
        let file_name = document_id.path().file_name().unwrap_or_default();
        let result = if file_name.ends_with(".effect.dawn") {
            self.load_effect_document(document_id, &absolute)
        } else if file_name.ends_with(".operator.dawn") {
            self.load_operator_document(document_id, &absolute)
        } else {
            self.load_dawn_document(document_id, &absolute)
        };
        self.loading_documents.shift_remove(document_id);
        result
    }

    pub(super) fn absolute_document_path(
        &self,
        document_id: &dawn_language::identity::DocumentId,
    ) -> Result<Utf8PathBuf, LoadProjectError> {
        self.source_graph
            .module(document_id.module_id())
            .map(|module| module.root.join(document_id.path()))
            .map_err(|error| LoadProjectError::InvalidDocument {
                path: document_id.path().to_path_buf(),
                range: None,
                message: error.to_string(),
            })
    }

    pub(super) fn read_source(
        &self,
        document_id: &dawn_language::identity::DocumentId,
        absolute: &Utf8Path,
    ) -> Result<String, LoadProjectError> {
        if let Some(source) = self.source_overrides.get(document_id) {
            return Ok(source.clone());
        }
        fs::read_to_string(absolute).map_err(|source| LoadProjectError::Io {
            path: absolute.to_path_buf(),
            source,
        })
    }

    pub(super) fn load_effect_document(
        &mut self,
        document_id: &dawn_language::identity::DocumentId,
        absolute: &Utf8Path,
    ) -> Result<(), LoadProjectError> {
        let relative = document_id.path();
        let source = self.read_source(document_id, absolute)?;
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
            let id = EffectDefinitionId(self.source_identity(document_id, name.clone()));
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
        self.visible_objects.insert(document_id.clone(), visible);
        let document =
            SourceDocument::new(Vec::new(), objects, SourceDocumentKind::Effect { source })
                .map_err(|message| LoadProjectError::InvalidDocument {
                    path: relative.to_path_buf(),
                    range: None,
                    message,
                })?;
        self.documents.insert(document_id.clone(), document);
        Ok(())
    }

    pub(super) fn load_operator_document(
        &mut self,
        document_id: &dawn_language::identity::DocumentId,
        absolute: &Utf8Path,
    ) -> Result<(), LoadProjectError> {
        let relative = document_id.path();
        let source = self.read_source(document_id, absolute)?;
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
            let id = OperatorDefinitionId(self.source_identity(document_id, name.clone()));
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
        self.visible_objects.insert(document_id.clone(), visible);
        let document =
            SourceDocument::new(Vec::new(), objects, SourceDocumentKind::Operator { source })
                .map_err(|message| LoadProjectError::InvalidDocument {
                    path: relative.to_path_buf(),
                    range: None,
                    message,
                })?;
        self.documents.insert(document_id.clone(), document);
        Ok(())
    }

    pub(super) fn load_dawn_document(
        &mut self,
        document_id: &dawn_language::identity::DocumentId,
        absolute: &Utf8Path,
    ) -> Result<(), LoadProjectError> {
        let relative = document_id.path();
        let text = self.read_source(document_id, absolute)?;
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
            let object = match object_type {
                "project" => ResolvedObject::Project(ProjectId(
                    self.source_identity(document_id, key.to_string()),
                )),
                "setup" => ResolvedObject::Setup(SetupId(
                    self.source_identity(document_id, key.to_string()),
                )),
                "controller" => ResolvedObject::Controller(ControllerId(
                    self.source_identity(document_id, key.to_string()),
                )),
                "element_tree" => ResolvedObject::ElementTree(ElementTreeId(
                    self.source_identity(document_id, key.to_string()),
                )),
                "preview_layout" => ResolvedObject::PreviewLayout(PreviewLayoutId(
                    self.source_identity(document_id, key.to_string()),
                )),
                "patch" => ResolvedObject::Patch(PatchId(
                    self.source_identity(document_id, key.to_string()),
                )),
                "prop" => ResolvedObject::PropDefinition(PropDefinitionId(
                    self.source_identity(document_id, key.to_string()),
                )),
                "fixture_profile" => ResolvedObject::FixtureProfile(FixtureProfileId(
                    self.source_identity(document_id, key.to_string()),
                )),
                "curve" => ResolvedObject::Curve(CurveId(
                    self.source_identity(document_id, key.to_string()),
                )),
                "gradient" => ResolvedObject::Gradient(GradientId(
                    self.source_identity(document_id, key.to_string()),
                )),
                "sequence" => ResolvedObject::Sequence(SequenceId(
                    self.source_identity(document_id, key.to_string()),
                )),
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

        self.visible_objects.insert(document_id.clone(), visible);

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
            let targets = self.resolve_import(document_id, &import.source)?;
            let mut names = IndexSet::new();
            for target in &targets {
                if !imported_targets.insert(target.clone()) {
                    return Err(LoadProjectError::InvalidDocument {
                        path: relative.to_path_buf(),
                        range: None,
                        message: format!(
                            "document `{}:{}` is imported more than once; each target must have one alias",
                            target.module_id(),
                            target.path()
                        ),
                    });
                }
                self.load_document(target)?;
                let target_visible = self.visible_objects.get(target).ok_or_else(|| {
                    LoadProjectError::InvalidDocument {
                        path: target.path().to_path_buf(),
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
                if document_id.module_id() == self.source_graph.project_module_id()
                    && let Some(previous) = self
                        .operator_reconciliation
                        .as_ref()
                        .and_then(|definitions| definitions.get(target))
                {
                    for id in previous {
                        let name = id.0.object().to_string();
                        if target_visible
                            .keys()
                            .any(|key| key.alias.is_none() && key.object.as_str() == name.as_str())
                        {
                            continue;
                        }
                        if !names.insert(name.clone()) {
                            return Err(LoadProjectError::InvalidDocument {
                                path: relative.to_path_buf(),
                                range: None,
                                message: format!(
                                    "duplicate exported object `{name}` in import alias `{}`",
                                    import.alias
                                ),
                            });
                        }
                        imported_visible.push((
                            AliasObjectKey {
                                alias: Some(import.alias.clone()),
                                object: name,
                            },
                            ResolvedObject::OperatorDefinition(id.clone()),
                        ));
                    }
                }
            }
            import_edges.push(ImportEdge {
                alias: import.alias.clone(),
                source: match &import.source {
                    ParsedImportSource::LocalDocuments { documents } => {
                        ImportSource::LocalDocuments {
                            documents: documents.clone(),
                        }
                    }
                    ParsedImportSource::DependencyExport { dependency, export } => {
                        ImportSource::DependencyExport {
                            dependency: dependency.clone(),
                            export: export.clone(),
                        }
                    }
                },
                targets,
            });
        }
        if let Some(visible) = self.visible_objects.get_mut(document_id) {
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
        self.documents.insert(document_id.clone(), document);
        Ok(())
    }

    pub(super) fn resolve_import(
        &self,
        importer: &dawn_language::identity::DocumentId,
        import_source: &ParsedImportSource,
    ) -> Result<Vec<dawn_language::identity::DocumentId>, LoadProjectError> {
        match import_source {
            ParsedImportSource::LocalDocuments { documents } => documents
                .iter()
                .map(|path| {
                    let target = dawn_language::identity::DocumentId::new(
                        importer.module_id(),
                        path.clone(),
                    );
                    let absolute = self.absolute_document_path(&target)?;
                    if !absolute.is_file() {
                        return Err(LoadProjectError::InvalidDocument {
                            path: importer.path().to_path_buf(),
                            range: None,
                            message: format!("local import target does not exist: {path}"),
                        });
                    }
                    Ok(target)
                })
                .collect(),
            ParsedImportSource::DependencyExport { dependency, export } => {
                let target_module = self
                    .source_graph
                    .dependency(importer.module_id(), dependency)
                    .map_err(|error| LoadProjectError::InvalidDocument {
                        path: importer.path().to_path_buf(),
                        range: None,
                        message: error.to_string(),
                    })?;
                let group = target_module.manifest.exports.get(export).ok_or_else(|| {
                    LoadProjectError::InvalidDocument {
                        path: importer.path().to_path_buf(),
                        range: None,
                        message: format!(
                            "dependency `{dependency}` does not export group `{export}`"
                        ),
                    }
                })?;
                group
                    .documents
                    .iter()
                    .map(|path| {
                        let target = dawn_language::identity::DocumentId::new(
                            target_module.manifest.module_id,
                            Utf8PathBuf::from(path),
                        );
                        let absolute = self.absolute_document_path(&target)?;
                        if !absolute.is_file() {
                            return Err(LoadProjectError::InvalidDocument {
                                path: importer.path().to_path_buf(),
                                range: None,
                                message: format!(
                                    "dependency `{dependency}` export `{export}` is missing `{path}`"
                                ),
                            });
                        }
                        Ok(target)
                    })
                    .collect()
            }
        }
    }

    pub(super) fn resolve_project(
        &mut self,
        entrypoint: &dawn_language::identity::DocumentId,
    ) -> Result<DawnProject, LoadProjectError> {
        let root_object = self.single_project_object(entrypoint)?;
        let root_id = ProjectId(
            self.source_identity(
                entrypoint,
                Identifier::new(root_object.key.clone())
                    .map_err(|_| LoadProjectError::InvalidDocument {
                        path: entrypoint.path().to_path_buf(),
                        range: None,
                        message: "project object key is not a valid identifier".to_string(),
                    })?
                    .as_str()
                    .to_string(),
            ),
        );
        let setup = self.reference_as_setup(
            entrypoint,
            string_field(entrypoint.path(), root_object.value, "setup")?,
        )?;
        let sequences = sequence_field(entrypoint.path(), root_object.value, "sequences")?
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
                path: entrypoint.path().to_path_buf(),
                range: None,
                message: format!("project validation failed: {error:?}"),
            }
        })?;
        Ok(project)
    }

    fn single_project_object<'a>(
        &'a self,
        document_id: &dawn_language::identity::DocumentId,
    ) -> Result<SourceObjectValue<'a>, LoadProjectError> {
        let path = document_id.path();
        let document = self.dawn_document(document_id)?;
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
                if found.is_some() {
                    return Err(LoadProjectError::InvalidDocument {
                        path: path.to_path_buf(),
                        range: source_range_for_value(path, value),
                        message: "entrypoint must contain exactly one project object".to_string(),
                    });
                }
                found = Some(SourceObjectValue {
                    key: key.to_string(),
                    value,
                });
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
    ) -> Result<(dawn_language::identity::DocumentId, String, Value), LoadProjectError> {
        let identity = id.source_identity();
        if !self
            .documents
            .get(identity.document_id())
            .is_some_and(|document| {
                document
                    .objects
                    .iter()
                    .any(|object| object.kind == id.source_kind() && object.id == identity.object())
            })
        {
            return Err(LoadProjectError::InvalidReference {
                path: identity.document().to_path_buf(),
                range: None,
                reference: id.id_string(),
            });
        }
        let document = self.dawn_document(identity.document_id())?;
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
            identity.document_id().clone(),
            identity.object().to_string(),
            value,
        ))
    }

    fn dawn_document<'a>(
        &'a self,
        document_id: &dawn_language::identity::DocumentId,
    ) -> Result<&'a Value, LoadProjectError> {
        let path = document_id.path();
        let document =
            self.documents
                .get(document_id)
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
        document_id: &dawn_language::identity::DocumentId,
        reference: &str,
    ) -> Result<ResolvedObject, LoadProjectError> {
        let path = document_id.path();
        let range = source_range_for_scalar(path, reference);
        let (alias, object) = reference
            .split_once('.')
            .map_or((None, reference), |(alias, object)| (Some(alias), object));
        self.visible_objects
            .get(document_id)
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
        document_id: &dawn_language::identity::DocumentId,
        reference: &str,
    ) -> Result<SetupId, LoadProjectError> {
        let path = document_id.path();
        match self.resolve_reference(document_id, reference)? {
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
        document_id: &dawn_language::identity::DocumentId,
        reference: &str,
    ) -> Result<SequenceId, LoadProjectError> {
        let path = document_id.path();
        match self.resolve_reference(document_id, reference)? {
            ResolvedObject::Sequence(id) => Ok(id),
            _ => Err(LoadProjectError::InvalidReference {
                path: path.to_path_buf(),
                range: source_range_for_scalar(path, reference),
                reference: reference.to_string(),
            }),
        }
    }
}

fn missing_exported_definition(
    document: &dawn_language::identity::DocumentId,
    identity: &dawn_language::identity::SourceIdentity,
) -> LoadProjectError {
    LoadProjectError::InvalidReference {
        path: document.path().to_path_buf(),
        range: None,
        reference: identity.object().to_string(),
    }
}

use std::collections::BTreeMap;
use std::fs;

use camino::{Utf8Path, Utf8PathBuf};
use dawn_language::controller::ControllerId;
use dawn_language::dsl::{Identifier, compile_effects, compile_operators};
use dawn_language::effect::{
    CurveDefinition, CurveId, EffectDefinition, EffectDefinitionId, GradientDefinition, GradientId,
};
use dawn_language::element::ElementTreeId;
use dawn_language::fixture_profile::FixtureProfileId;
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
    ImportEdge, ImportSource, ProjectSession, ReferencedAsset, SourceDocument, SourceDocumentKind,
    SourceObjectId, SourceObjectKind, SourceProject,
};
use crate::{IoDiagnosticCode, LoadProjectError};
