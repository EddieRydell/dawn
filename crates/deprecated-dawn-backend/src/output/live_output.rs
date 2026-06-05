use std::collections::HashMap;
use std::net::{SocketAddr, UdpSocket};

use dawn_language::analysis::ProjectAnalysis;

use crate::output::controller_output::{
    build_output_plan, encode_e131_data_packet, ControllerOutputPlan, ControllerUniverseFrame,
};
use crate::runtime::contracts::{Event, RuntimeResult};
use crate::RenderedFrame;

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

#[derive(Debug, Default)]
pub struct LiveOutputCore {
    enabled: bool,
    socket: Option<UdpSocket>,
    plan: Option<ControllerOutputPlan>,
    sequence_counters: HashMap<UniverseSequenceKey, u8>,
    readout: LiveOutputReadout,
}

impl LiveOutputCore {
    pub fn enabled(&self) -> bool {
        self.enabled
    }

    pub fn set_enabled(&mut self, enabled: bool, analysis: Option<&ProjectAnalysis>) {
        if enabled {
            match self.enable(analysis) {
                Ok(()) => {}
                Err(error) => {
                    self.enabled = true;
                    self.socket = None;
                    self.plan = None;
                    self.readout = LiveOutputReadout {
                        enabled: true,
                        status: LiveOutputReadoutStatus::Error,
                        active_universe_count: 0,
                        last_error: Some(error),
                    };
                }
            }
        } else {
            self.disable();
        }
    }

    pub fn send_frame(&mut self, analysis: Option<&ProjectAnalysis>, frame: &RenderedFrame) {
        if !self.enabled {
            return;
        }
        let Some(analysis) = analysis else {
            self.set_error("project analysis is not available".to_string(), 0);
            return;
        };
        let plan = match build_output_plan(analysis) {
            Ok(plan) => plan,
            Err(error) => {
                self.set_error(error.to_string(), 0);
                return;
            }
        };
        let active_universe_count = plan.active_universe_count();
        self.plan = Some(plan);
        let Some(plan) = self.plan.clone() else {
            self.set_error("live output plan is not available".to_string(), 0);
            return;
        };
        let output_frame = frame.clone().into();
        match self.send_buffers(plan.frame_buffers(&output_frame)) {
            Ok(()) => {
                self.readout = LiveOutputReadout {
                    enabled: true,
                    status: LiveOutputReadoutStatus::Sending,
                    active_universe_count,
                    last_error: None,
                };
            }
            Err(error) => self.set_error(error, active_universe_count),
        }
    }

    pub fn sync_readout(&mut self, readout: LiveOutputReadout) {
        self.enabled = readout.enabled;
        self.readout = readout;
    }

    pub fn readout(&self) -> LiveOutputReadout {
        self.readout.clone()
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

    fn enable(&mut self, analysis: Option<&ProjectAnalysis>) -> Result<(), String> {
        let analysis = analysis.ok_or_else(|| "project analysis is not available".to_string())?;
        let plan = build_output_plan(analysis).map_err(|error| error.to_string())?;
        let active_universe_count = plan.active_universe_count();
        let socket = UdpSocket::bind("0.0.0.0:0").map_err(|error| error.to_string())?;
        self.enabled = true;
        self.socket = Some(socket);
        self.plan = Some(plan);
        self.sequence_counters.clear();
        self.readout = LiveOutputReadout {
            enabled: true,
            status: LiveOutputReadoutStatus::Ready,
            active_universe_count,
            last_error: None,
        };
        Ok(())
    }

    fn disable(&mut self) {
        let last_error = self
            .plan
            .clone()
            .and_then(|plan| self.send_buffers(plan.blackout_buffers()).err());
        self.enabled = false;
        self.socket = None;
        self.plan = None;
        self.sequence_counters.clear();
        self.readout = LiveOutputReadout {
            enabled: false,
            status: LiveOutputReadoutStatus::Disabled,
            active_universe_count: 0,
            last_error,
        };
    }

    fn set_error(&mut self, error: String, active_universe_count: usize) {
        self.readout = LiveOutputReadout {
            enabled: true,
            status: LiveOutputReadoutStatus::Error,
            active_universe_count,
            last_error: Some(error),
        };
    }

    fn send_buffers(&mut self, buffers: Vec<ControllerUniverseFrame>) -> Result<(), String> {
        if buffers.is_empty() {
            return Ok(());
        }
        if self.socket.is_none() {
            self.socket = Some(UdpSocket::bind("0.0.0.0:0").map_err(|error| error.to_string())?);
        }
        for buffer in buffers {
            let destination = buffer
                .destination
                .ok_or_else(|| "live output destination is not available".to_string())?;
            let key = UniverseSequenceKey {
                destination,
                universe: buffer.universe,
            };
            let sequence = self.sequence_counters.entry(key).or_insert(0);
            let packet = encode_e131_data_packet(&buffer, *sequence);
            *sequence = sequence.wrapping_add(1);
            let socket = self
                .socket
                .as_ref()
                .ok_or_else(|| "live output socket is not available".to_string())?;
            socket
                .send_to(&packet, destination)
                .map_err(|error| error.to_string())?;
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct UniverseSequenceKey {
    destination: SocketAddr,
    universe: u16,
}
