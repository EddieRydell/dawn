use crate::contracts::{Event, RuntimeResult};

#[derive(Debug, Default, Clone)]
pub struct LiveOutputCore {
    pub enabled: bool,
}

impl LiveOutputCore {
    pub fn set_enabled(&mut self, enabled: bool) {
        self.enabled = enabled;
    }

    pub fn consume(&self, event: &Event) -> RuntimeResult<Option<Event>> {
        if !self.enabled {
            return Ok(None);
        }
        match event {
            Event::PreviewFramePublished { .. } => Ok(Some(event.clone())),
            _ => Ok(None),
        }
    }
}
