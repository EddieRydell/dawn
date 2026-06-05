mod active_document;
pub(crate) mod file_events;
mod project;
mod project_index;
mod sequence_edit;
mod session;
mod starter_project;

pub use active_document::{build_active_gui_document, ActiveGuiDocument};
pub use project::ProjectWorkspace;
pub use session::{project_root_label_for_path, WorkspaceSession};
pub(crate) use starter_project::{create_starter_project, STARTER_SEQUENCE_PATH};
