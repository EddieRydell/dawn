use std::fmt;
use std::path::Path;
use std::sync::Arc;

use camino::{Utf8Path, Utf8PathBuf};

use crate::path::{lexical_normalize, utf8_path};

#[derive(Clone)]
pub struct WorkspaceFs {
    root: Arc<Utf8PathBuf>,
}

impl fmt::Debug for WorkspaceFs {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("WorkspaceFs")
            .field("root", &self.root)
            .finish()
    }
}

impl WorkspaceFs {
    pub fn open(root: impl AsRef<Path>) -> Result<Self, std::io::Error> {
        let root = utf8_path(root)
            .map_err(|message| std::io::Error::new(std::io::ErrorKind::InvalidInput, message))?;
        Ok(Self {
            root: Arc::new(lexical_normalize(&root)),
        })
    }

    pub fn root(&self) -> &Utf8Path {
        &self.root
    }

    pub fn resolve(&self, path: &Utf8Path) -> Utf8PathBuf {
        if path.is_absolute() {
            path.to_path_buf()
        } else {
            self.root.join(path)
        }
    }

    pub fn read_to_string(&self, path: &Utf8Path) -> Result<String, std::io::Error> {
        std::fs::read_to_string(self.resolve(path).as_std_path())
    }

    pub fn write(&self, path: &Utf8Path, content: impl AsRef<[u8]>) -> Result<(), std::io::Error> {
        std::fs::write(self.resolve(path).as_std_path(), content)
    }

    pub fn create_file(
        &self,
        path: &Utf8Path,
        content: impl AsRef<[u8]>,
    ) -> Result<(), std::io::Error> {
        let path = self.resolve(path);
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create_new(true);
        let mut file = options.open(path.as_std_path())?;
        std::io::Write::write_all(&mut file, content.as_ref())
    }

    pub fn create_dir(&self, path: &Utf8Path) -> Result<(), std::io::Error> {
        std::fs::create_dir(self.resolve(path).as_std_path())
    }

    pub fn delete_path(&self, path: &Utf8Path) -> Result<(), std::io::Error> {
        let path = self.resolve(path);
        if path.is_dir() {
            std::fs::remove_dir_all(path.as_std_path())
        } else {
            std::fs::remove_file(path.as_std_path())
        }
    }

    pub fn rename(&self, from: &Utf8Path, to: &Utf8Path) -> Result<(), std::io::Error> {
        std::fs::rename(
            self.resolve(from).as_std_path(),
            self.resolve(to).as_std_path(),
        )
    }

    pub fn exists(&self, path: &Utf8Path) -> bool {
        self.resolve(path).exists()
    }

    pub fn is_dir(&self, path: &Utf8Path) -> bool {
        self.resolve(path).is_dir()
    }

    pub fn is_file(&self, path: &Utf8Path) -> bool {
        self.resolve(path).is_file()
    }

    pub fn list_entries(&self) -> Result<Vec<WorkspaceEntry>, std::io::Error> {
        let mut entries = Vec::new();
        self.list_entries_inner(Utf8Path::new(""), &mut entries)?;
        entries.sort_by(|left, right| left.path.cmp(&right.path));
        Ok(entries)
    }

    fn list_entries_inner(
        &self,
        parent: &Utf8Path,
        entries: &mut Vec<WorkspaceEntry>,
    ) -> Result<(), std::io::Error> {
        let dir_path = self.resolve(parent);
        for entry in std::fs::read_dir(dir_path.as_std_path())? {
            let entry = entry?;
            let file_name = entry.file_name().into_string().map_err(|name| {
                std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    format!("path is not valid UTF-8: {}", name.to_string_lossy()),
                )
            })?;
            let path = if parent.as_str().is_empty() {
                Utf8PathBuf::from(file_name)
            } else {
                parent.join(file_name)
            };
            let metadata = entry.metadata()?;
            let kind = if metadata.is_dir() {
                WorkspaceEntryKind::Directory
            } else {
                WorkspaceEntryKind::File
            };
            entries.push(WorkspaceEntry {
                path: path.clone(),
                kind,
            });
            if metadata.is_dir() {
                self.list_entries_inner(&path, entries)?;
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspaceEntry {
    pub path: Utf8PathBuf,
    pub kind: WorkspaceEntryKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkspaceEntryKind {
    Directory,
    File,
}
