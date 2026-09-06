use camino::{Utf8Path, Utf8PathBuf};
use dawn_language::dsl::Identifier;
use dawn_language::identity::DocumentId;
use dawn_language::model::DawnProject;
use dawn_language::sequence::AssetId;
use indexmap::{IndexMap, IndexSet};
use std::collections::{BTreeMap, BTreeSet};
use uuid::Uuid;
use yaml_serde::Value;

#[derive(Clone, Debug, PartialEq)]
pub struct ProjectSession {
    pub project: DawnProject,
    pub source: SourceProject,
}

#[derive(Clone, Debug, PartialEq)]
pub struct SourceProject {
    pub source_graph: dawn_package::ResolvedSourceGraph,
    pub entrypoint: Option<DocumentId>,
    pub documents: IndexMap<DocumentId, SourceDocument>,
    pub referenced_assets: Vec<ReferencedAsset>,
}

impl SourceProject {
    pub fn project_module_id(&self) -> Uuid {
        self.source_graph.project_module_id()
    }

    pub fn project_root(&self) -> &Utf8Path {
        self.source_graph.project_module().root.as_path()
    }

    pub fn module(&self, module_id: Uuid) -> Option<&dawn_package::ResolvedModule> {
        self.source_graph.module(module_id).ok()
    }

    pub fn ownership(&self, document: &DocumentId) -> Option<SourceOwnership> {
        self.module(document.module_id())
            .map(|module| match &module.origin {
                dawn_package::ResolvedModuleOrigin::Project => SourceOwnership::ProjectOwned,
                dawn_package::ResolvedModuleOrigin::PathDependency { declared_path, .. } => {
                    SourceOwnership::PathDependencyOwned {
                        declared_path: declared_path.clone(),
                        module_id: document.module_id(),
                    }
                }
                dawn_package::ResolvedModuleOrigin::RegistryDependency { package, .. } => {
                    SourceOwnership::RegistryReadOnly {
                        package: package.as_str().to_string(),
                        module_id: document.module_id(),
                    }
                }
            })
    }

    pub fn absolute_path(&self, document: &DocumentId) -> Option<Utf8PathBuf> {
        self.module(document.module_id())
            .map(|module| module.root.join(document.path()))
    }

    pub fn workspace_module_for_path(
        &self,
        relative_path: &Utf8Path,
    ) -> Option<(Uuid, Utf8PathBuf)> {
        workspace_module_for_path(&self.source_graph, relative_path)
    }

    pub fn document_for_workspace_path(&self, relative_path: &Utf8Path) -> Option<DocumentId> {
        let (module_id, module_relative) = self.workspace_module_for_path(relative_path)?;
        let document_id = DocumentId::new(module_id, module_relative);
        self.documents
            .contains_key(&document_id)
            .then_some(document_id)
    }

    pub fn workspace_path_for_document(&self, document: &DocumentId) -> Option<Utf8PathBuf> {
        let module = self.module(document.module_id())?;
        if matches!(
            module.origin,
            dawn_package::ResolvedModuleOrigin::RegistryDependency { .. }
        ) {
            return None;
        }
        let absolute = module.root.join(document.path());
        let relative = absolute.strip_prefix(self.project_root()).ok()?;
        Some(relative.to_path_buf())
    }

    pub fn is_structural_workspace_path(&self, path: &Utf8Path) -> bool {
        self.entrypoint
            .iter()
            .chain(
                self.documents
                    .values()
                    .flat_map(|document| document.imports().iter())
                    .flat_map(|edge| edge.targets().iter()),
            )
            .filter_map(|document_id| self.workspace_path_for_document(document_id))
            .any(|document_path| {
                document_path == path
                    || document_path
                        .strip_prefix(path)
                        .is_ok_and(|suffix| !suffix.as_str().is_empty())
            })
    }

    pub fn project_document(&self, path: Utf8PathBuf) -> DocumentId {
        DocumentId::new(self.project_module_id(), path)
    }

    pub fn is_project_owned(&self, document: &DocumentId) -> bool {
        self.ownership(document) == Some(SourceOwnership::ProjectOwned)
    }

    pub fn is_editable(&self, document: &DocumentId) -> bool {
        matches!(
            self.ownership(document),
            Some(SourceOwnership::ProjectOwned | SourceOwnership::PathDependencyOwned { .. })
        )
    }
}

pub(crate) fn workspace_module_for_path(
    graph: &dawn_package::ResolvedSourceGraph,
    relative_path: &Utf8Path,
) -> Option<(Uuid, Utf8PathBuf)> {
    let absolute = graph.project_module().root.join(relative_path);
    let (module_id, module) = graph
        .modules()
        .iter()
        .filter(|(_, module)| {
            !matches!(
                module.origin,
                dawn_package::ResolvedModuleOrigin::RegistryDependency { .. }
            ) && absolute.starts_with(&module.root)
        })
        .max_by_key(|(_, module)| module.root.components().count())?;
    let module_relative = absolute.strip_prefix(&module.root).ok()?;
    Some((*module_id, module_relative.to_path_buf()))
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SourceOwnership {
    ProjectOwned,
    PathDependencyOwned {
        declared_path: String,
        module_id: Uuid,
    },
    RegistryReadOnly {
        package: String,
        module_id: Uuid,
    },
}

#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct SourceObjectId {
    pub(crate) kind: SourceObjectKind,
    pub(crate) id: String,
}

impl SourceObjectId {
    pub fn new(kind: SourceObjectKind, id: String) -> Result<Self, String> {
        Identifier::new(id.clone())
            .map_err(|_| format!("invalid source object identifier `{id}`"))?;
        Ok(Self { kind, id })
    }

    pub fn kind(&self) -> &SourceObjectKind {
        &self.kind
    }

    pub fn id(&self) -> &str {
        self.id.as_str()
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub enum SourceObjectKind {
    Project,
    Setup,
    Controller,
    ElementTree,
    PreviewLayout,
    Patch,
    PropDefinition,
    FixtureProfile,
    Curve,
    Gradient,
    Sequence,
    EffectDefinition,
    OperatorDefinition,
    EffectInstance,
}

#[derive(Clone, Debug, PartialEq)]
pub struct SourceDocument {
    pub(crate) imports: Vec<ImportEdge>,
    pub(crate) objects: Vec<SourceObjectId>,
    pub(crate) kind: SourceDocumentKind,
}

impl SourceDocument {
    pub fn new(
        imports: Vec<ImportEdge>,
        objects: Vec<SourceObjectId>,
        kind: SourceDocumentKind,
    ) -> Result<Self, String> {
        let mut aliases = IndexSet::new();
        let mut targets = IndexSet::new();
        for import in &imports {
            if !aliases.insert(import.alias.clone()) {
                return Err(format!("duplicate import alias `{}`", import.alias));
            }
            for target in &import.targets {
                if !targets.insert(target.clone()) {
                    return Err(format!(
                        "document `{}:{}` is imported more than once",
                        target.module_id(),
                        target.path()
                    ));
                }
            }
        }
        let mut object_ids = IndexSet::new();
        for object in &objects {
            if Identifier::new(object.id.clone()).is_err() {
                return Err(format!("invalid source object identifier `{}`", object.id));
            }
            if !object_ids.insert(object.id.clone()) {
                return Err(format!("duplicate source object `{}`", object.id));
            }
            let kind_matches_document = match &kind {
                SourceDocumentKind::Effect { .. } => {
                    object.kind == SourceObjectKind::EffectDefinition
                }
                SourceDocumentKind::Operator { .. } => {
                    object.kind == SourceObjectKind::OperatorDefinition
                }
                SourceDocumentKind::Dawn { .. } => !matches!(
                    object.kind,
                    SourceObjectKind::EffectDefinition | SourceObjectKind::OperatorDefinition
                ),
            };
            if !kind_matches_document {
                return Err(format!(
                    "source object `{}` is not valid in this document kind",
                    object.id
                ));
            }
        }
        Ok(Self {
            imports,
            objects,
            kind,
        })
    }

    pub fn imports(&self) -> &[ImportEdge] {
        &self.imports
    }

    pub fn objects(&self) -> &[SourceObjectId] {
        &self.objects
    }

    pub fn kind(&self) -> &SourceDocumentKind {
        &self.kind
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum SourceDocumentKind {
    Dawn { original_value: Value },
    Effect { source: String },
    Operator { source: String },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ImportEdge {
    pub(crate) alias: String,
    pub(crate) source: ImportSource,
    pub(crate) targets: Vec<DocumentId>,
}

impl ImportEdge {
    pub fn alias(&self) -> &str {
        &self.alias
    }

    pub fn source(&self) -> &ImportSource {
        &self.source
    }

    pub fn targets(&self) -> &[DocumentId] {
        &self.targets
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ImportSource {
    LocalDocuments { documents: Vec<Utf8PathBuf> },
    DependencyExport { dependency: String, export: String },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReferencedAsset {
    pub id: AssetId,
    pub module_id: Uuid,
    pub relative_path: Utf8PathBuf,
    pub absolute_path: Utf8PathBuf,
    pub referenced_by: BTreeSet<DocumentId>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExportReport {
    pub written_files: Vec<Utf8PathBuf>,
    pub copied_assets: Vec<Utf8PathBuf>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SaveReport {
    pub written_files: Vec<Utf8PathBuf>,
}

pub fn source_file_list(session: &ProjectSession) -> BTreeMap<DocumentId, Vec<String>> {
    session
        .source
        .documents
        .iter()
        .map(|(path, document)| {
            (
                path.clone(),
                document
                    .objects
                    .iter()
                    .map(|object| object.id.clone())
                    .collect(),
            )
        })
        .collect()
}
