use camino::{Utf8Component, Utf8Path, Utf8PathBuf};
use dawn_language::dsl::Identifier;
use dawn_language::model::DawnProject;
use dawn_language::sequence::AssetId;
use indexmap::{IndexMap, IndexSet};
use std::collections::BTreeMap;
use yaml_serde::Value;

#[derive(Clone, Debug, PartialEq)]
pub struct ProjectSession {
    pub project: DawnProject,
    pub source: SourceProject,
}

#[derive(Clone, Debug, PartialEq)]
pub struct SourceProject {
    pub source_root: Utf8PathBuf,
    pub entrypoint: Utf8PathBuf,
    pub documents: IndexMap<Utf8PathBuf, SourceDocument>,
    pub referenced_assets: Vec<ReferencedAsset>,
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
    Layout,
    Patch,
    FixtureDefinition,
    Curve,
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
                    return Err(format!("document `{target}` is imported more than once"));
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
    pub(crate) from: Utf8PathBuf,
    pub(crate) targets: Vec<Utf8PathBuf>,
}

impl ImportEdge {
    pub fn alias(&self) -> &str {
        &self.alias
    }

    pub fn from(&self) -> &Utf8Path {
        &self.from
    }

    pub fn targets(&self) -> &[Utf8PathBuf] {
        &self.targets
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReferencedAsset {
    pub id: AssetId,
    pub relative_path: Utf8PathBuf,
    pub absolute_path: Utf8PathBuf,
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

pub fn relative_path_from_document(from_document: &Utf8Path, target: &Utf8Path) -> Utf8PathBuf {
    let from = normal_components(from_document.parent().unwrap_or(Utf8Path::new("")));
    let target = normal_components(target);
    let common = from
        .iter()
        .zip(&target)
        .take_while(|(left, right)| left == right)
        .count();
    std::iter::repeat_n("..".to_string(), from.len() - common)
        .chain(target.into_iter().skip(common))
        .collect()
}

fn normal_components(path: &Utf8Path) -> Vec<String> {
    path.components()
        .filter_map(|component| match component {
            Utf8Component::Normal(value) => Some(value.to_string()),
            Utf8Component::ParentDir => Some("..".to_string()),
            _ => None,
        })
        .collect()
}

pub fn is_project_owned_path(path: &Utf8Path) -> bool {
    !path.is_absolute()
        && path
            .components()
            .all(|component| matches!(component, Utf8Component::Normal(_) | Utf8Component::CurDir))
}

pub fn source_file_list(session: &ProjectSession) -> BTreeMap<Utf8PathBuf, Vec<String>> {
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
