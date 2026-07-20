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
                    SourceOwnership::DependencyReadOnly {
                        package: format!("path:{declared_path}"),
                        module_id: document.module_id(),
                    }
                }
                dawn_package::ResolvedModuleOrigin::RegistryDependency { package, .. } => {
                    SourceOwnership::DependencyReadOnly {
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

    pub fn project_document(&self, path: Utf8PathBuf) -> DocumentId {
        DocumentId::new(self.project_module_id(), path)
    }

    pub fn is_project_owned(&self, document: &DocumentId) -> bool {
        self.ownership(document) == Some(SourceOwnership::ProjectOwned)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SourceOwnership {
    ProjectOwned,
    DependencyReadOnly { package: String, module_id: Uuid },
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
