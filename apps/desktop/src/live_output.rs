use std::collections::HashMap;
use std::net::{SocketAddr, UdpSocket};

use dawn_app_runtime::controller_output::{
    build_output_plan, encode_e131_data_packet, ControllerOutputPlan,
};
use dawn_app_runtime::domain::ProjectIndexSnapshot;
use dawn_app_runtime::domain::{OutputReadout, OutputReadoutStatus};
use dawn_app_runtime::output_runtime::OutputFrame;

#[derive(Debug, Default)]
pub(crate) struct LiveOutputRuntime {
    enabled: bool,
    socket: Option<UdpSocket>,
    plan: Option<ControllerOutputPlan>,
    sequence_counters: HashMap<UniverseSequenceKey, u8>,
    snapshot: OutputReadout,
}

impl LiveOutputRuntime {
    pub(crate) fn snapshot(&self) -> OutputReadout {
        self.snapshot.clone()
    }

    pub(crate) fn enabled(&self) -> bool {
        self.enabled
    }

    pub(crate) fn set_enabled(
        &mut self,
        enabled: bool,
        analysis: Option<&ProjectIndexSnapshot>,
    ) -> OutputReadout {
        if enabled {
            match self.enable(analysis) {
                Ok(()) => {}
                Err(error) => {
                    self.enabled = true;
                    self.socket = None;
                    self.plan = None;
                    self.snapshot = OutputReadout {
                        enabled: true,
                        status: OutputReadoutStatus::Error,
                        active_universe_count: 0,
                        last_error: Some(error),
                    };
                }
            }
        } else {
            self.disable();
        }
        self.snapshot()
    }

    pub(crate) fn send_frame(
        &mut self,
        analysis: Option<&ProjectIndexSnapshot>,
        frame: &OutputFrame,
    ) -> OutputReadout {
        if !self.enabled {
            return self.snapshot();
        }
        let Some(analysis) = analysis else {
            self.set_error("project analysis is not available".to_string(), 0);
            return self.snapshot();
        };
        let plan = match build_output_plan(analysis) {
            Ok(plan) => plan,
            Err(error) => {
                self.set_error(error.to_string(), 0);
                return self.snapshot();
            }
        };
        let active_universe_count = plan.active_universe_count();
        self.plan = Some(plan);
        let Some(plan) = self.plan.clone() else {
            self.set_error("live output plan is not available".to_string(), 0);
            return self.snapshot();
        };
        match self.send_buffers(plan.frame_buffers(frame)) {
            Ok(()) => {
                self.snapshot = OutputReadout {
                    enabled: true,
                    status: OutputReadoutStatus::Sending,
                    active_universe_count,
                    last_error: None,
                };
            }
            Err(error) => self.set_error(error, active_universe_count),
        }
        self.snapshot()
    }

    fn enable(&mut self, analysis: Option<&ProjectIndexSnapshot>) -> Result<(), String> {
        let analysis = analysis.ok_or_else(|| "project analysis is not available".to_string())?;
        let plan = build_output_plan(analysis).map_err(|error| error.to_string())?;
        let active_universe_count = plan.active_universe_count();
        let socket = UdpSocket::bind("0.0.0.0:0").map_err(|error| error.to_string())?;
        self.enabled = true;
        self.socket = Some(socket);
        self.plan = Some(plan);
        self.sequence_counters.clear();
        self.snapshot = OutputReadout {
            enabled: true,
            status: OutputReadoutStatus::Ready,
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
        self.snapshot = OutputReadout {
            enabled: false,
            status: OutputReadoutStatus::Disabled,
            active_universe_count: 0,
            last_error,
        };
    }

    fn set_error(&mut self, error: String, active_universe_count: usize) {
        self.snapshot = OutputReadout {
            enabled: true,
            status: OutputReadoutStatus::Error,
            active_universe_count,
            last_error: Some(error),
        };
    }

    fn send_buffers(
        &mut self,
        buffers: Vec<dawn_app_runtime::controller_output::ControllerUniverseFrame>,
    ) -> Result<(), String> {
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
