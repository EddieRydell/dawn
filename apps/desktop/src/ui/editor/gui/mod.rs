use std::{cell::RefCell, collections::HashMap, rc::Rc};

use crate::ui::components::canvas::CanvasState;

pub mod fixture;
pub mod layout;
pub mod sequence;

#[derive(Clone)]
pub struct EditorGuiUiState {
    layout_canvases: Rc<RefCell<HashMap<String, CanvasState>>>,
    fixture_canvases: Rc<RefCell<HashMap<String, CanvasState>>>,
    sequence_timelines: Rc<RefCell<HashMap<String, sequence::SequenceTimelineState>>>,
}

impl EditorGuiUiState {
    pub fn new() -> Self {
        Self {
            layout_canvases: Rc::new(RefCell::new(HashMap::new())),
            fixture_canvases: Rc::new(RefCell::new(HashMap::new())),
            sequence_timelines: Rc::new(RefCell::new(HashMap::new())),
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

    pub fn fixture_canvas(&self, path: &str, object_key: &str) -> CanvasState {
        let key = format!("{path}#{object_key}");
        if let Some(state) = self.fixture_canvases.borrow().get(&key).cloned() {
            return state;
        }
        let state = CanvasState::new();
        self.fixture_canvases
            .borrow_mut()
            .insert(key, state.clone());
        state
    }

    pub fn sequence_timeline(
        &self,
        path: &str,
        object_key: &str,
    ) -> sequence::SequenceTimelineState {
        let key = format!("{path}#{object_key}");
        if let Some(state) = self.sequence_timelines.borrow().get(&key).cloned() {
            return state;
        }
        let state = sequence::SequenceTimelineState::new();
        self.sequence_timelines
            .borrow_mut()
            .insert(key, state.clone());
        state
    }
}
