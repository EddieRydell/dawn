use std::collections::{BTreeMap, BTreeSet};

use camino::Utf8PathBuf;
use dawn_project_io::{ProjectSession, source_document_text as generated_source_document_text};

use crate::dto::{AppSnapshot, BufferExternalState, EditorBuffer};

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
