use std::sync::Mutex;

use crate::dto::{
    AppSnapshot, AudioPlaybackStatus, LiveOutputSnapshot, SequenceTransportSnapshot,
    SequenceTransportState,
};

pub struct DesktopState {
    snapshot: Mutex<AppSnapshot>,
}

impl DesktopState {
    pub fn new() -> Self {
        Self {
            snapshot: Mutex::new(empty_snapshot()),
        }
    }

    pub fn snapshot(&self) -> AppSnapshot {
        match self.snapshot.lock() {
            Ok(snapshot) => snapshot.clone(),
            Err(poisoned) => poisoned.into_inner().clone(),
        }
    }

    pub fn update_snapshot(&self, update: impl FnOnce(&mut AppSnapshot)) -> AppSnapshot {
        match self.snapshot.lock() {
            Ok(mut snapshot) => {
                update(&mut snapshot);
                snapshot.clone()
            }
            Err(poisoned) => {
                let mut snapshot = poisoned.into_inner();
                update(&mut snapshot);
                snapshot.clone()
            }
        }
    }
}

impl Default for DesktopState {
    fn default() -> Self {
        Self::new()
    }
}

fn empty_snapshot() -> AppSnapshot {
    AppSnapshot {
        project_root: None,
        project_tree_visible: true,
        project_entries: Vec::new(),
        tabs: Vec::new(),
        active_file: None,
        active_buffer: None,
        active_document_descriptor: None,
        active_gui_document: None,
        diagnostics: Vec::new(),
        status: "Ready".to_string(),
        sequence_transport: SequenceTransportSnapshot {
            source_label: "No sequence".to_string(),
            source_key: None,
            render_generation: 0,
            render_dirty_revision: 0,
            transport_state: SequenceTransportState::Stopped,
            render_updating: false,
            position_seconds: 0.0,
            home_seconds: 0.0,
            duration_seconds: 0.0,
            audio: None,
            clock_source: "none".to_string(),
            audio_playback_status: AudioPlaybackStatus::None,
            geometry_identity: String::new(),
            status: "Idle".to_string(),
        },
        live_output: LiveOutputSnapshot {
            enabled: false,
            status: "Disabled".to_string(),
            active_universe_count: 0,
            last_error: None,
        },
    }
}
