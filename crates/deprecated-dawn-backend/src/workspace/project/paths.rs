use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::path::Path;

use dawn_language::fs::{WorkspaceEntry, WorkspaceEntryKind, WorkspaceFs};

pub(super) fn content_hash(content: &str) -> u64 {
    let mut hasher = DefaultHasher::new();
    content.hash(&mut hasher);
    hasher.finish()
}

pub(super) fn no_project() -> String {
    "no project open".to_string()
}

pub(super) fn list_project_entries(fs: &WorkspaceFs) -> Result<Vec<WorkspaceEntry>, String> {
    let mut entries = fs.list_entries().map_err(|error| error.to_string())?;
    entries.sort_by(|left, right| {
        (left.kind != WorkspaceEntryKind::Directory, &left.path)
            .cmp(&(right.kind != WorkspaceEntryKind::Directory, &right.path))
    });
    Ok(entries)
}

pub(super) fn validate_file_name(name: &str) -> Result<(), String> {
    if name.trim().is_empty() {
        return Err("name cannot be empty".to_string());
    }
    if name == "." || name == ".." {
        return Err("name cannot be . or ..".to_string());
    }
    if name.contains('/') || name.contains('\\') {
        return Err("name cannot contain path separators".to_string());
    }
    Ok(())
}

pub(super) fn file_name_with_default_extension(name: &str) -> Result<String, String> {
    validate_file_name(name)?;
    let path = Path::new(name);
    if path.extension().is_none() {
        Ok(format!("{name}.dawn"))
    } else {
        Ok(name.to_string())
    }
}
