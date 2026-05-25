pub mod components;
pub mod editor;
pub mod inspector;
pub mod project_tree;
pub mod theme;
pub mod window;
pub mod workbench;

use std::rc::Rc;

use floem::reactive::RwSignal;

use crate::actions::AppAction;
use crate::app_model::AppSnapshot;

pub type UiSnapshot = RwSignal<AppSnapshot>;
pub type UiDispatch = Rc<dyn Fn(AppAction)>;
