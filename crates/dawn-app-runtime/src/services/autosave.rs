use std::collections::BTreeSet;

use dawn_language::path::Utf8PathBuf;

use crate::runtime::contracts::{Event, Revision, RuntimeResult, SelfWriteTag};

#[derive(Debug, Clone)]
pub enum AutosaveCommand {
    TagSelfWrite {
        path: Utf8PathBuf,
        revision: Revision,
    },
    CompleteWrite {
        tag: SelfWriteTag,
    },
}

#[derive(Debug, Default, Clone)]
pub struct AutosaveCore {
    next_nonce: u64,
    active_tags: BTreeSet<(Utf8PathBuf, Revision, u64)>,
    revision: Revision,
}

impl AutosaveCore {
    pub fn contains_tag(&self, tag: &SelfWriteTag) -> bool {
        self.active_tags
            .contains(&(tag.path.clone(), tag.revision, tag.nonce))
    }

    pub fn handle(&mut self, command: AutosaveCommand) -> RuntimeResult<Vec<Event>> {
        match command {
            AutosaveCommand::TagSelfWrite { path, revision } => {
                let tag = SelfWriteTag {
                    path: path.clone(),
                    revision,
                    nonce: self.next_nonce,
                };
                self.next_nonce = self.next_nonce.saturating_add(1);
                self.active_tags
                    .insert((tag.path.clone(), tag.revision, tag.nonce));
                self.revision = revision;
                Ok(vec![Event::AutosaveTagged {
                    path,
                    tag,
                    revision,
                }])
            }
            AutosaveCommand::CompleteWrite { tag } => {
                self.active_tags
                    .remove(&(tag.path.clone(), tag.revision, tag.nonce));
                Ok(Vec::new())
            }
        }
    }
}
