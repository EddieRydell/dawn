use camino::{Utf8Path, Utf8PathBuf};

/// Qualified identity for an object declared in a Dawn source document.
///
/// Object keys are only unique inside their declaring document. Keeping both
/// parts in the domain model prevents import aliases and same-named objects in
/// different documents from collapsing into one global string namespace. Source
/// loaders validate object keys before constructing domain IDs.
#[derive(Clone, Debug, Eq, PartialEq, Hash, Ord, PartialOrd)]
pub struct SourceIdentity {
    document: Utf8PathBuf,
    object: String,
}

impl SourceIdentity {
    pub fn new(document: Utf8PathBuf, object: String) -> Self {
        Self { document, object }
    }

    pub fn document(&self) -> &Utf8Path {
        &self.document
    }

    pub fn object(&self) -> &str {
        &self.object
    }
}
