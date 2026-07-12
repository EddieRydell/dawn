mod sequence;
mod setup;
mod values;

use sequence::sequence_value;
use setup::{controller_value, fixture_definition_value, layout_value, patch_value, setup_value};
use values::{curve_value, gradient_value, string_value, typed_object, write_source_reference};

pub(super) fn write_source_documents(
    session: &ProjectSession,
    output_root: &Utf8Path,
) -> Result<Vec<Utf8PathBuf>, ExportProjectError> {
    let mut prepared = Vec::new();
    for (relative_path, document) in &session.source.documents {
        if !is_project_owned_path(relative_path) {
            continue;
        }
        let output_path = output_root.join(relative_path);
        if let Some(parent) = output_path.parent() {
            fs::create_dir_all(parent).map_err(|source| ExportProjectError::Io {
                path: parent.to_path_buf(),
                source,
            })?;
        }

        let text =
            document_text(session, relative_path, document).map_err(|error| match error {
                ExportProjectError::Serialize { source, .. } => ExportProjectError::Serialize {
                    path: output_path.clone(),
                    source,
                },
                other => other,
            })?;
        let previous = if output_path.is_file() {
            Some(
                fs::read(&output_path).map_err(|source| ExportProjectError::Io {
                    path: output_path.clone(),
                    source,
                })?,
            )
        } else {
            None
        };
        prepared.push((relative_path.clone(), output_path, text, previous));
    }
    let mut written = 0usize;
    while written < prepared.len() {
        let (_, output_path, text, _) = &prepared[written];
        if let Err(write_error) = fs::write(output_path, text) {
            let mut rollback_failures = Vec::new();
            for (_, written_path, _, previous) in prepared[..=written].iter().rev() {
                let rollback = match previous {
                    Some(bytes) => fs::write(written_path, bytes),
                    None => fs::remove_file(written_path),
                };
                if let Err(error) = rollback {
                    rollback_failures.push(format!("{written_path}: {error}"));
                }
            }
            if rollback_failures.is_empty() {
                return Err(ExportProjectError::Io {
                    path: output_path.clone(),
                    source: write_error,
                });
            }
            return Err(ExportProjectError::Io {
                path: output_path.clone(),
                source: io::Error::other(format!(
                    "{write_error}; rollback also failed: {}",
                    rollback_failures.join(", ")
                )),
            });
        }
        written += 1;
    }
    Ok(prepared
        .into_iter()
        .map(|(relative_path, _, _, _)| relative_path)
        .collect())
}

pub(super) fn document_text(
    session: &ProjectSession,
    relative_path: &Utf8Path,
    document: &SourceDocument,
) -> Result<String, ExportProjectError> {
    match &document.kind {
        SourceDocumentKind::Dawn { original_value } => {
            let existing = mapping(original_value);
            let mut root = Mapping::new();
            if !document.imports.is_empty() {
                root.insert(
                    string_value("imports"),
                    import_decls_value(&document.imports),
                );
            }
            for object in &document.objects {
                let value = if has_typed_object(session, relative_path, object) {
                    serialize_source_object(session, relative_path, object)?
                } else {
                    existing
                        .and_then(|mapping| mapping.get(string_value(&object.id)))
                        .cloned()
                        .ok_or_else(|| ExportProjectError::InvalidReference {
                            path: relative_path.to_path_buf(),
                            reference: object.id.clone(),
                            message: "source object is missing from the source document"
                                .to_string(),
                        })?
                };
                root.insert(string_value(&object.id), value);
            }
            yaml_serde::to_string(&Value::Mapping(root)).map_err(|source| {
                ExportProjectError::Serialize {
                    path: relative_path.to_path_buf(),
                    source,
                }
            })
        }
        SourceDocumentKind::Effect { source } => Ok(source.clone()),
        SourceDocumentKind::Operator { source } => Ok(source.clone()),
    }
}

pub(super) fn has_typed_object(
    session: &ProjectSession,
    document: &Utf8Path,
    id: &SourceObjectId,
) -> bool {
    match id.kind {
        SourceObjectKind::Project => qualified_identity(session, document, id)
            .is_some_and(|identity| session.project.root.id == ProjectId(identity)),
        SourceObjectKind::Setup => qualified_identity(session, document, id)
            .is_some_and(|identity| session.project.setups.contains_key(&SetupId(identity))),
        SourceObjectKind::Controller => {
            qualified_identity(session, document, id).is_some_and(|identity| {
                session
                    .project
                    .controllers
                    .contains_key(&ControllerId(identity))
            })
        }
        SourceObjectKind::Layout => qualified_identity(session, document, id)
            .is_some_and(|identity| session.project.layouts.contains_key(&LayoutId(identity))),
        SourceObjectKind::Patch => qualified_identity(session, document, id)
            .is_some_and(|identity| session.project.patches.contains_key(&PatchId(identity))),
        SourceObjectKind::FixtureDefinition => qualified_identity(session, document, id)
            .is_some_and(|identity| {
                session
                    .project
                    .definitions
                    .fixtures
                    .definitions
                    .contains_key(&FixtureDefinitionId(identity))
            }),
        SourceObjectKind::Curve => {
            qualified_identity(session, document, id).is_some_and(|identity| {
                session
                    .project
                    .definitions
                    .curves
                    .definitions
                    .contains_key(&CurveId(identity))
            })
        }
        SourceObjectKind::Gradient => {
            qualified_identity(session, document, id).is_some_and(|identity| {
                session
                    .project
                    .definitions
                    .gradients
                    .definitions
                    .contains_key(&GradientId(identity))
            })
        }
        SourceObjectKind::Sequence => {
            qualified_identity(session, document, id).is_some_and(|identity| {
                session
                    .project
                    .sequences
                    .contains_key(&SequenceId(identity))
            })
        }
        SourceObjectKind::EffectDefinition => qualified_identity(session, document, id)
            .is_some_and(|identity| {
                session
                    .project
                    .definitions
                    .effects
                    .definitions
                    .contains_key(&EffectDefinitionId(identity))
            }),
        SourceObjectKind::OperatorDefinition => qualified_identity(session, document, id)
            .is_some_and(|identity| {
                session
                    .project
                    .definitions
                    .operators
                    .definitions
                    .contains_key(&OperatorDefinitionId(identity))
            }),
        SourceObjectKind::EffectInstance => false,
    }
}

pub(super) fn qualified_identity(
    session: &ProjectSession,
    document: &Utf8Path,
    id: &SourceObjectId,
) -> Option<SourceIdentity> {
    session
        .source
        .documents
        .get(document)
        .is_some_and(|document| document.objects.contains(id))
        .then(|| SourceIdentity::new(document.to_path_buf(), id.id.clone()))
}

pub(super) fn serialize_source_object(
    session: &ProjectSession,
    from_document: &Utf8Path,
    id: &SourceObjectId,
) -> Result<Value, ExportProjectError> {
    match id.kind {
        SourceObjectKind::Project => project_root_value(session, from_document),
        SourceObjectKind::Setup => {
            let identity = qualified_identity(session, from_document, id)
                .ok_or_else(|| missing_typed_object(from_document, id))?;
            let setup = session
                .project
                .setups
                .get(&SetupId(identity))
                .ok_or_else(|| missing_typed_object(from_document, id))?;
            setup_value(session, from_document, setup)
        }
        SourceObjectKind::Controller => {
            let identity = qualified_identity(session, from_document, id)
                .ok_or_else(|| missing_typed_object(from_document, id))?;
            let controller = session
                .project
                .controllers
                .get(&ControllerId(identity))
                .ok_or_else(|| missing_typed_object(from_document, id))?;
            controller_value(controller)
        }
        SourceObjectKind::Layout => {
            let identity = qualified_identity(session, from_document, id)
                .ok_or_else(|| missing_typed_object(from_document, id))?;
            let layout = session
                .project
                .layouts
                .get(&LayoutId(identity))
                .ok_or_else(|| missing_typed_object(from_document, id))?;
            layout_value(session, from_document, layout)
        }
        SourceObjectKind::Patch => {
            let identity = qualified_identity(session, from_document, id)
                .ok_or_else(|| missing_typed_object(from_document, id))?;
            let patch = session
                .project
                .patches
                .get(&PatchId(identity))
                .ok_or_else(|| missing_typed_object(from_document, id))?;
            patch_value(session, from_document, patch)
        }
        SourceObjectKind::FixtureDefinition => {
            let identity = qualified_identity(session, from_document, id)
                .ok_or_else(|| missing_typed_object(from_document, id))?;
            let definition = session
                .project
                .definitions
                .fixtures
                .definitions
                .get(&FixtureDefinitionId(identity))
                .ok_or_else(|| missing_typed_object(from_document, id))?;
            fixture_definition_value(definition)
        }
        SourceObjectKind::Curve => {
            let identity = qualified_identity(session, from_document, id)
                .ok_or_else(|| missing_typed_object(from_document, id))?;
            let curve = session
                .project
                .definitions
                .curves
                .definitions
                .get(&CurveId(identity))
                .ok_or_else(|| missing_typed_object(from_document, id))?;
            curve_value(&curve.curve)
        }
        SourceObjectKind::Gradient => {
            let identity = qualified_identity(session, from_document, id)
                .ok_or_else(|| missing_typed_object(from_document, id))?;
            let gradient = session
                .project
                .definitions
                .gradients
                .definitions
                .get(&GradientId(identity))
                .ok_or_else(|| missing_typed_object(from_document, id))?;
            gradient_value(&gradient.gradient)
        }
        SourceObjectKind::Sequence => {
            let identity = qualified_identity(session, from_document, id)
                .ok_or_else(|| missing_typed_object(from_document, id))?;
            let sequence = session
                .project
                .sequences
                .get(&SequenceId(identity))
                .ok_or_else(|| missing_typed_object(from_document, id))?;
            sequence_value(session, from_document, sequence)
        }
        SourceObjectKind::EffectDefinition
        | SourceObjectKind::OperatorDefinition
        | SourceObjectKind::EffectInstance => Err(ExportProjectError::InvalidReference {
            path: from_document.to_path_buf(),
            reference: id.id.clone(),
            message: "DSL definitions are preserved as source documents".to_string(),
        }),
    }
}

pub(super) fn missing_typed_object(path: &Utf8Path, id: &SourceObjectId) -> ExportProjectError {
    ExportProjectError::InvalidReference {
        path: path.to_path_buf(),
        reference: id.id.clone(),
        message: "typed project object is missing".to_string(),
    }
}

pub(super) fn import_decls_value(imports: &[ImportEdge]) -> Value {
    Value::Sequence(
        imports
            .iter()
            .map(|import| {
                let mut value = Mapping::new();
                value.insert(string_value("from"), Value::String(import.from.to_string()));
                value.insert(string_value("as"), Value::String(import.alias.clone()));
                Value::Mapping(value)
            })
            .collect(),
    )
}

pub(super) fn project_root_value(
    session: &ProjectSession,
    from_document: &Utf8Path,
) -> Result<Value, ExportProjectError> {
    let mut value = typed_object("project");
    value.insert(
        string_value("setup"),
        Value::String(write_source_reference(
            session,
            from_document,
            SourceObjectKind::Setup,
            &session.project.root.setup.0,
        )?),
    );
    value.insert(
        string_value("sequences"),
        Value::Sequence(
            session
                .project
                .root
                .sequences
                .iter()
                .map(|sequence| {
                    write_source_reference(
                        session,
                        from_document,
                        SourceObjectKind::Sequence,
                        &sequence.0,
                    )
                    .map(Value::String)
                })
                .collect::<Result<Vec<_>, _>>()?,
        ),
    );
    Ok(Value::Mapping(value))
}
use std::{fs, io};

use camino::{Utf8Path, Utf8PathBuf};
use dawn_language::effect::{CurveId, EffectDefinitionId, GradientId};
use dawn_language::identity::SourceIdentity;
use dawn_language::model::ProjectId;
use dawn_language::operator::OperatorDefinitionId;
use dawn_language::sequence::SequenceId;
use dawn_language::setup::{ControllerId, FixtureDefinitionId, LayoutId, PatchId, SetupId};
use yaml_serde::{Mapping, Value};

use crate::ExportProjectError;
use crate::loader::mapping;
use crate::source::{
    ImportEdge, ProjectSession, SourceDocument, SourceDocumentKind, SourceObjectId,
    SourceObjectKind, is_project_owned_path,
};
