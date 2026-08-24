use std::collections::{BTreeMap, BTreeSet};
use std::fs;

use camino::{Utf8Path, Utf8PathBuf};
use dawn_project_io::{
    ProjectRecovery, ProjectSession, SourceDocument, SourceObjectKind,
    source_document_text as generated_source_document_text,
};

use crate::dto::{
    AppSnapshot, BufferExternalState, DocumentDefaultObjectKey, DocumentDescriptor,
    DocumentObjectDescriptor, DocumentViewId, EditorBuffer, ObjectKind,
};
use crate::persistence::PersistedProjectSession;

pub(crate) fn generated_source_texts(
    session: &ProjectSession,
    paths: &BTreeSet<String>,
) -> Result<BTreeMap<String, String>, String> {
    let mut texts = BTreeMap::new();
    for path in paths {
        let document_id = session.source.project_document(Utf8PathBuf::from(path));
        match generated_source_document_text(session, &document_id) {
            Ok(Some(text)) => {
                texts.insert(path.clone(), text);
            }
            Ok(None) => {}
            Err(error) => return Err(error.to_string()),
        }
    }
    Ok(texts)
}

pub(crate) fn refresh_clean_buffers(
    snapshot: &mut AppSnapshot,
    generated_text: &BTreeMap<String, String>,
) {
    for (path, text) in generated_text {
        if let Some(tab) = snapshot.tabs.iter_mut().find(|tab| tab.path == *path) {
            if tab.dirty {
                tab.external_state = BufferExternalState::ChangedOnDisk;
            } else {
                tab.text = text.clone();
                tab.external_state = BufferExternalState::Current;
            }
        }
        if let Some(buffer) = snapshot
            .active_buffer
            .as_mut()
            .filter(|buffer| buffer.path == *path)
        {
            if buffer.dirty {
                buffer.external_state = BufferExternalState::ChangedOnDisk;
            } else {
                buffer.text = text.clone();
                buffer.external_state = BufferExternalState::Current;
            }
        }
    }
}

pub(crate) fn upsert_tab(tabs: &mut Vec<EditorBuffer>, buffer: EditorBuffer) {
    if let Some(tab) = tabs.iter_mut().find(|tab| tab.path == buffer.path) {
        *tab = buffer;
    } else {
        tabs.push(buffer);
    }
}

pub(crate) fn editor_buffer(
    session: &ProjectSession,
    relative_path: &Utf8Path,
) -> Option<EditorBuffer> {
    editor_buffer_at_root(session.source.project_root(), relative_path)
}

fn editor_buffer_at_root(root: &Utf8Path, relative_path: &Utf8Path) -> Option<EditorBuffer> {
    let disk_path = root.join(relative_path);
    let text = fs::read_to_string(&disk_path).ok()?;
    Some(EditorBuffer {
        path: relative_path.to_string(),
        name: relative_path
            .file_name()
            .map(ToString::to_string)
            .unwrap_or_else(|| relative_path.to_string()),
        text,
        dirty: false,
        external_state: BufferExternalState::Current,
    })
}

pub(crate) fn recovery_editor_buffer(
    recovery: &ProjectRecovery,
    relative_path: &Utf8Path,
) -> Option<EditorBuffer> {
    let path = super::absolute_root_path(&recovery.root, relative_path)?;
    path.is_file()
        .then(|| editor_buffer_at_root(&recovery.root, relative_path))
        .flatten()
}

pub(crate) fn editor_buffer_for_path(
    session: &ProjectSession,
    relative_path: &Utf8Path,
) -> Option<EditorBuffer> {
    let path = super::absolute_project_path(session, relative_path)?;
    if !path.is_file() {
        return None;
    }
    editor_buffer(session, relative_path)
}

pub(crate) fn restored_active_buffers(
    session: &ProjectSession,
    restore: Option<&PersistedProjectSession>,
) -> Option<(Vec<EditorBuffer>, String)> {
    let restore = restore?;
    let mut buffers = Vec::new();
    for path in &restore.tabs {
        let relative_path = Utf8Path::new(path);
        if let Some(buffer) = editor_buffer_for_path(session, relative_path) {
            buffers.push(buffer);
        }
    }
    if buffers.is_empty() {
        return None;
    }
    let active_file = restore
        .active_file
        .as_ref()
        .filter(|path| buffers.iter().any(|buffer| &buffer.path == *path))
        .cloned()
        .unwrap_or_else(|| buffers[0].path.clone());
    Some((buffers, active_file))
}

pub(crate) fn descriptor_for_path(
    session: &ProjectSession,
    relative_path: &Utf8Path,
) -> Option<DocumentDescriptor> {
    super::absolute_project_path(session, relative_path)
        .and_then(|_| session.source.document_for_workspace_path(relative_path))
        .and_then(|document_id| session.source.documents.get(&document_id))
        .map(|document| document_descriptor(relative_path, document))
        .or_else(|| {
            super::absolute_project_path(session, relative_path)
                .is_some_and(|path| path.is_file())
                .then(|| empty_document_descriptor(relative_path))
        })
}

pub(crate) fn recovery_descriptor_for_path(
    recovery: &ProjectRecovery,
    relative_path: &Utf8Path,
) -> Option<DocumentDescriptor> {
    recovery
        .documents
        .get(relative_path)
        .map(|document| {
            let objects = document
                .objects
                .iter()
                .filter(|object| {
                    object.kind != SourceObjectKind::Sequence || object.sequence.is_some()
                })
                .map(|object| DocumentObjectDescriptor {
                    key: object.key.clone(),
                    kind: ObjectKind::from(&object.kind),
                })
                .collect::<Vec<_>>();
            DocumentDescriptor {
                path: relative_path.to_string(),
                available_views: available_views(&objects),
                default_object_keys: default_object_keys(&objects),
                objects,
            }
        })
        .or_else(|| {
            super::absolute_root_path(&recovery.root, relative_path)
                .is_some_and(|path| path.is_file())
                .then(|| empty_document_descriptor(relative_path))
        })
}

fn document_descriptor(path: &Utf8Path, document: &SourceDocument) -> DocumentDescriptor {
    let objects = document
        .objects()
        .iter()
        .map(|object| DocumentObjectDescriptor {
            key: object.id().to_string(),
            kind: ObjectKind::from(object.kind()),
        })
        .collect::<Vec<_>>();
    let available_views = available_views(&objects);
    let default_object_keys = default_object_keys(&objects);
    DocumentDescriptor {
        path: path.to_string(),
        objects,
        available_views,
        default_object_keys,
    }
}

fn empty_document_descriptor(path: &Utf8Path) -> DocumentDescriptor {
    DocumentDescriptor {
        path: path.to_string(),
        objects: Vec::new(),
        available_views: vec![DocumentViewId::Text],
        default_object_keys: Vec::new(),
    }
}

fn available_views(objects: &[DocumentObjectDescriptor]) -> Vec<DocumentViewId> {
    let mut views = vec![DocumentViewId::Text];
    for object in objects {
        let view = object.kind.document_view();
        if let Some(view) = view
            && !views.contains(&view)
        {
            views.push(view);
        }
    }
    views
}

fn default_object_keys(objects: &[DocumentObjectDescriptor]) -> Vec<DocumentDefaultObjectKey> {
    objects
        .iter()
        .filter_map(|object| {
            let view = object.kind.document_view()?;
            Some(DocumentDefaultObjectKey {
                view,
                object_key: object.key.clone(),
            })
        })
        .collect()
}
