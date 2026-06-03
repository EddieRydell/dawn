use crate::contracts::{Event, RuntimeResult};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LiveOutputReadout {
    pub enabled: bool,
    pub status: LiveOutputReadoutStatus,
    pub active_universe_count: usize,
    pub last_error: Option<String>,
}

impl Default for LiveOutputReadout {
    fn default() -> Self {
        Self {
            enabled: false,
            status: LiveOutputReadoutStatus::Disabled,
            active_universe_count: 0,
            last_error: None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LiveOutputReadoutStatus {
    Disabled,
    Ready,
    Sending,
    Error,
}

impl LiveOutputReadoutStatus {
    pub fn label(self) -> &'static str {
        match self {
            Self::Disabled => "Disabled",
            Self::Ready => "Ready",
            Self::Sending => "Sending",
            Self::Error => "Error",
        }
    }
}

pub type LiveOutputSnapshot = LiveOutputReadout;
pub type LiveOutputStatus = LiveOutputReadoutStatus;

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
