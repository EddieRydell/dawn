pub mod components;
pub mod editor;
pub mod fonts;
pub mod inspector;
pub mod preview_window;
pub mod project_tree;
pub mod theme;
pub mod window;
pub mod workbench;

use std::rc::Rc;

use floem::reactive::RwSignal;

use crate::actions::AppAction;
use crate::app_model::AppSnapshot;
use crate::preview_session::PreviewSnapshot;

pub type UiSnapshot = RwSignal<AppSnapshot>;
pub type UiPreviewSnapshot = RwSignal<PreviewSnapshot>;
pub type UiDispatch = Rc<dyn Fn(AppAction)>;
