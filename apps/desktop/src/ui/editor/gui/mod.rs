use std::collections::HashMap;

use floem::reactive::{RwSignal, SignalUpdate, SignalWith};

use crate::ui::components::canvas::CanvasState;

pub mod fixture;
pub mod layout;

#[derive(Clone, Copy)]
pub struct EditorGuiUiState {
    layout_canvases: RwSignal<HashMap<String, CanvasState>>,
}

impl EditorGuiUiState {
    pub fn new() -> Self {
        Self {
            layout_canvases: RwSignal::new(HashMap::new()),
        }
    }

    pub fn layout_canvas(&self, path: &str, object_key: &str) -> CanvasState {
        let key = format!("{path}#{object_key}");
        if let Some(state) = self
            .layout_canvases
            .with_untracked(|canvases| canvases.get(&key).copied())
        {
            return state;
        }
        let state = CanvasState::new();
        self.layout_canvases.update(|canvases| {
            canvases.insert(key, state);
        });
        state
    }
}
