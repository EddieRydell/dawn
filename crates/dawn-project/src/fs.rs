use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use cap_std::fs::Dir;

use crate::path::ProjectPath;

#[derive(Clone)]
pub struct ProjectFs {
    root: Arc<Dir>,
}

impl fmt::Debug for ProjectFs {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.debug_struct("ProjectFs").finish_non_exhaustive()
    }
}

impl ProjectFs {
    pub fn open_ambient(root: impl AsRef<Path>) -> Result<Self, std::io::Error> {
        Ok(Self {
            root: Arc::new(Dir::open_ambient_dir(root, cap_std::ambient_authority())?),
        })
    }

    pub fn read_to_string(&self, path: &ProjectPath) -> Result<String, std::io::Error> {
        self.root.read_to_string(path.as_path())
    }

    pub fn write(
        &self,
        path: &ProjectPath,
        content: impl AsRef<[u8]>,
    ) -> Result<(), std::io::Error> {
        self.root.write(path.as_path(), content)
    }

    pub fn create_file(
        &self,
        path: &ProjectPath,
        content: impl AsRef<[u8]>,
    ) -> Result<(), std::io::Error> {
        let mut options = cap_std::fs::OpenOptions::new();
        options.write(true).create_new(true);
        let mut file = self.root.open_with(path.as_path(), &options)?;
        std::io::Write::write_all(&mut file, content.as_ref())
    }

    pub fn create_dir(&self, path: &ProjectPath) -> Result<(), std::io::Error> {
        self.root.create_dir(path.as_path())
    }

    pub fn delete_path(&self, path: &ProjectPath) -> Result<(), std::io::Error> {
        if self.is_dir(path) {
            self.root.remove_dir_all(path.as_path())
        } else {
            self.root.remove_file(path.as_path())
        }
    }

    pub fn rename(&self, from: &ProjectPath, to: &ProjectPath) -> Result<(), std::io::Error> {
        self.root.rename(from.as_path(), &self.root, to.as_path())
    }

    pub fn exists(&self, path: &ProjectPath) -> bool {
        self.root.metadata(path.as_path()).is_ok()
    }

    pub fn is_dir(&self, path: &ProjectPath) -> bool {
        self.root
            .metadata(path.as_path())
            .is_ok_and(|metadata| metadata.is_dir())
    }

    pub fn is_file(&self, path: &ProjectPath) -> bool {
        self.root
            .metadata(path.as_path())
            .is_ok_and(|metadata| metadata.is_file())
    }

    pub fn list_entries(&self) -> Result<Vec<ProjectFsEntry>, std::io::Error> {
        let mut entries = Vec::new();
        self.list_entries_inner(&ProjectPath::root(), &mut entries)?;
        entries.sort_by(|left, right| left.path.cmp(&right.path));
        Ok(entries)
    }

    fn list_entries_inner(
        &self,
        parent: &ProjectPath,
        entries: &mut Vec<ProjectFsEntry>,
    ) -> Result<(), std::io::Error> {
        let dir_path = if parent.is_root() {
            Path::new(".")
        } else {
            parent.as_path()
        };
        let dir = self.root.open_dir(dir_path)?;
        for entry in dir.entries()? {
            let entry = entry?;
            let file_name = entry.file_name();
            let path = parent.join(PathBuf::from(file_name)).map_err(|message| {
                std::io::Error::new(std::io::ErrorKind::InvalidInput, message)
            })?;
            let metadata = entry.metadata()?;
            let kind = if metadata.is_dir() {
                ProjectFsEntryKind::Directory
            } else {
                ProjectFsEntryKind::File
            };
            entries.push(ProjectFsEntry {
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
pub struct ProjectFsEntry {
    pub path: ProjectPath,
    pub kind: ProjectFsEntryKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProjectFsEntryKind {
    Directory,
    File,
}
