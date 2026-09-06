use crate::dto::{
    DocumentDefaultObjectKey, DocumentDescriptor, DocumentObjectDescriptor, DocumentViewId,
    ObjectKind,
};
use camino::Utf8Path;
use dawn_project_io::{ProjectSession, source_document_text};
use std::collections::{BTreeMap, BTreeSet};

pub(crate) fn generated_source_texts(
    session: &ProjectSession,
    paths: &BTreeSet<String>,
) -> Result<BTreeMap<String, String>, String> {
    let mut texts = BTreeMap::new();
    for path in paths {
        let id = session
            .source
            .document_for_workspace_path(Utf8Path::new(path))
            .ok_or_else(|| format!("No source document owns {path}"))?;
        if let Some(text) = source_document_text(session, &id).map_err(|error| error.to_string())? {
            texts.insert(path.clone(), text);
        }
    }
    Ok(texts)
}

pub(crate) fn descriptor_for_path(
    session: &ProjectSession,
    path: &Utf8Path,
) -> Option<DocumentDescriptor> {
    super::absolute_project_path(session, path)?;
    let document = session
        .source
        .document_for_workspace_path(path)
        .and_then(|id| session.source.documents.get(&id));
    let objects: Vec<_> = document
        .into_iter()
        .flat_map(|document| document.objects())
        .map(|object| DocumentObjectDescriptor {
            key: object.id().to_string(),
            kind: ObjectKind::from(object.kind()),
        })
        .collect();
    let default_object_keys: Vec<_> = objects
        .iter()
        .filter_map(|object| {
            object
                .kind
                .document_view()
                .map(|view| DocumentDefaultObjectKey {
                    view,
                    object_key: object.key.clone(),
                })
        })
        .collect();
    let mut available_views = vec![DocumentViewId::Text];
    for object in &default_object_keys {
        if !available_views.contains(&object.view) {
            available_views.push(object.view.clone());
        }
    }
    Some(DocumentDescriptor {
        path: path.to_string(),
        objects,
        available_views,
        default_object_keys,
    })
}
