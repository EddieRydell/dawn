use std::{cell::RefCell, collections::HashMap, rc::Rc};

use crate::ui::components::canvas::CanvasState;

pub mod fixture;
pub mod layout;

#[derive(Clone)]
pub struct EditorGuiUiState {
    layout_canvases: Rc<RefCell<HashMap<String, CanvasState>>>,
}

impl EditorGuiUiState {
    pub fn new() -> Self {
        Self {
            layout_canvases: Rc::new(RefCell::new(HashMap::new())),
        }
    }

    pub fn layout_canvas(&self, path: &str, object_key: &str) -> CanvasState {
        let key = format!("{path}#{object_key}");
        if let Some(state) = self.layout_canvases.borrow().get(&key).cloned() {
            return state;
        }
        let state = CanvasState::new();
        self.layout_canvases.borrow_mut().insert(key, state.clone());
        state
    }
}
