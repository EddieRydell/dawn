pub mod dawn_syntax;
pub mod diagnostics;
pub mod editor;
pub mod fixture_viewer;
pub mod geometry;
pub mod layout_viewer;
pub mod preview;
pub mod project_tree;
pub mod shell;
pub mod theme;
pub mod workbench;

use std::rc::Rc;

use floem::reactive::RwSignal;

use crate::actions::AppAction;
use crate::app_model::AppSnapshot;

pub type UiSnapshot = RwSignal<AppSnapshot>;
pub type UiDispatch = Rc<dyn Fn(AppAction)>;
