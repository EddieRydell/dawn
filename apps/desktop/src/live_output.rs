use std::collections::HashMap;
use std::net::{SocketAddr, UdpSocket};
use std::sync::Arc;

use dawn_app_core::app_model::{LiveOutputSnapshot, LiveOutputStatus};
use dawn_app_core::controller_output::{
    build_output_plan, encode_e131_data_packet, ControllerOutputPlan,
};
use dawn_app_core::output_runtime::{OutputGeometryModel, RenderedOutputFrame};
use dawn_project::DawnProject;

#[derive(Debug, Default)]
pub(crate) struct LiveOutputRuntime {
    enabled: bool,
    socket: Option<UdpSocket>,
    plan: Option<ControllerOutputPlan>,
    plan_project: Option<Arc<DawnProject>>,
    plan_geometry_id: Option<String>,
    sequence_counters: HashMap<UniverseSequenceKey, u8>,
    sent_frame_count: u64,
    sent_packet_count: u64,
    snapshot: LiveOutputSnapshot,
}

impl LiveOutputRuntime {
    pub(crate) fn snapshot(&self) -> LiveOutputSnapshot {
        self.snapshot.clone()
    }

    pub(crate) fn enabled(&self) -> bool {
        self.enabled
    }

    pub(crate) fn set_enabled(
        &mut self,
        enabled: bool,
        project: Option<Arc<DawnProject>>,
    ) -> LiveOutputSnapshot {
        if enabled {
            match self.enable(project) {
                Ok(()) => {}
                Err(error) => {
                    self.enabled = true;
                    self.socket = None;
                    self.plan = None;
                    self.plan_project = None;
                    self.plan_geometry_id = None;
                    self.snapshot = LiveOutputSnapshot {
                        enabled: true,
                        status: LiveOutputStatus::Error,
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
        project: Option<Arc<DawnProject>>,
        geometry: &OutputGeometryModel,
        frame: &RenderedOutputFrame,
    ) -> LiveOutputSnapshot {
        if !self.enabled {
            return self.snapshot();
        }
        let Some(project) = project else {
            self.set_error("project is not available".to_string(), 0);
            return self.snapshot();
        };
        let active_universe_count = match self.ensure_plan(project, geometry) {
            Ok(active_universe_count) => active_universe_count,
            Err(error) => {
                self.set_error(error, 0);
                return self.snapshot();
            }
        };
        let Some(plan) = self.plan.as_ref() else {
            self.set_error("live output plan is not available".to_string(), 0);
            return self.snapshot();
        };
        let buffers = plan.frame_buffers(frame);
        match self.send_buffers(buffers) {
            Ok(packet_count) => {
                self.record_send(packet_count);
                self.snapshot = LiveOutputSnapshot {
                    enabled: true,
                    status: LiveOutputStatus::Sending,
                    active_universe_count,
                    last_error: None,
                };
            }
            Err(error) => self.set_error(error, active_universe_count),
        }
        self.snapshot()
    }

    fn ensure_plan(
        &mut self,
        project: Arc<DawnProject>,
        geometry: &OutputGeometryModel,
    ) -> Result<usize, String> {
        if self
            .plan_project
            .as_ref()
            .is_none_or(|cached| !Arc::ptr_eq(cached, &project))
            || self.plan_geometry_id.as_deref() != Some(geometry.geometry_id.as_str())
        {
            let plan = match build_output_plan(&project, geometry) {
                Ok(plan) => plan,
                Err(error) => {
                    self.plan = None;
                    self.plan_project = None;
                    self.plan_geometry_id = None;
                    return Err(error.to_string());
                }
            };
            self.plan = Some(plan);
            self.plan_project = Some(project.clone());
            self.plan_geometry_id = Some(geometry.geometry_id.clone());
        }
        let Some(plan) = self.plan.as_ref() else {
            return Err("live output plan is not available".to_string());
        };
        Ok(plan.active_universe_count())
    }

    fn enable(&mut self, project: Option<Arc<DawnProject>>) -> Result<(), String> {
        let project = project.ok_or_else(|| "project is not available".to_string())?;
        let geometry =
            OutputGeometryModel::from_project(&project).map_err(|error| error.to_string())?;
        let plan = build_output_plan(&project, &geometry).map_err(|error| error.to_string())?;
        let active_universe_count = plan.active_universe_count();
        let socket = UdpSocket::bind("0.0.0.0:0").map_err(|error| error.to_string())?;
        self.enabled = true;
        self.socket = Some(socket);
        self.plan = Some(plan);
        self.plan_project = Some(project);
        self.plan_geometry_id = Some(geometry.geometry_id.clone());
        self.sequence_counters.clear();
        self.sent_frame_count = 0;
        self.sent_packet_count = 0;
        self.snapshot = LiveOutputSnapshot {
            enabled: true,
            status: LiveOutputStatus::Ready,
            active_universe_count,
            last_error: None,
        };
        Ok(())
    }

    fn disable(&mut self) {
        let last_error = self
            .plan
            .as_ref()
            .map(|plan| plan.blackout_buffers())
            .and_then(|buffers| self.send_buffers(buffers).err());
        self.enabled = false;
        self.socket = None;
        self.plan = None;
        self.plan_project = None;
        self.plan_geometry_id = None;
        self.sequence_counters.clear();
        self.sent_frame_count = 0;
        self.sent_packet_count = 0;
        self.snapshot = LiveOutputSnapshot {
            enabled: false,
            status: LiveOutputStatus::Disabled,
            active_universe_count: 0,
            last_error,
        };
    }

    fn set_error(&mut self, error: String, active_universe_count: usize) {
        self.snapshot = LiveOutputSnapshot {
            enabled: true,
            status: LiveOutputStatus::Error,
            active_universe_count,
            last_error: Some(error),
        };
    }

    fn record_send(&mut self, packet_count: usize) {
        self.sent_frame_count = self.sent_frame_count.saturating_add(1);
        self.sent_packet_count = self
            .sent_packet_count
            .saturating_add(packet_count.min(u64::MAX as usize) as u64);
    }

    fn send_buffers(
        &mut self,
        buffers: Vec<dawn_app_core::controller_output::ControllerUniverseFrame>,
    ) -> Result<usize, String> {
        if buffers.is_empty() {
            return Ok(0);
        }
        if self.socket.is_none() {
            self.socket = Some(UdpSocket::bind("0.0.0.0:0").map_err(|error| error.to_string())?);
        }
        let packet_count = buffers.len();
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
        Ok(packet_count)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct UniverseSequenceKey {
    destination: SocketAddr,
    universe: u16,
}

#[cfg(test)]
mod tests {
    use std::path::Path;
    use std::sync::Arc;

    use dawn_app_core::workspace::WorkspaceService;

    use super::LiveOutputRuntime;

    #[test]
    fn output_plan_is_reused_until_project_handle_changes() {
        let project_path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../examples/thirty-output-controller/project.dawn");
        let mut workspace = WorkspaceService::default();
        workspace
            .open_project(
                std::fs::canonicalize(project_path)
                    .expect("thirty output controller project path should exist"),
            )
            .expect("thirty output controller project should open");
        let result = workspace.load_project();
        assert!(result.diagnostics.is_empty(), "{:?}", result.diagnostics);
        let project = Arc::new(
            result
                .project
                .expect("thirty output controller should load"),
        );
        let geometry = dawn_app_core::output_runtime::OutputGeometryModel::from_project(&project)
            .expect("project geometry should build");
        let mut runtime = LiveOutputRuntime::default();

        let active_universes = runtime
            .ensure_plan(project.clone(), &geometry)
            .expect("project should build a live-output plan");
        let cached = runtime
            .plan_project
            .as_ref()
            .expect("plan project should be cached")
            .clone();
        let reused_active_universes = runtime
            .ensure_plan(project.clone(), &geometry)
            .expect("same project handle should reuse the cached plan");

        assert_eq!(active_universes, reused_active_universes);
        assert!(Arc::ptr_eq(
            runtime
                .plan_project
                .as_ref()
                .expect("plan project should remain cached"),
            &cached
        ));

        let new_project = Arc::new((*project).clone());
        let new_geometry =
            dawn_app_core::output_runtime::OutputGeometryModel::from_project(&new_project)
                .expect("new project geometry should build");
        runtime
            .ensure_plan(new_project.clone(), &new_geometry)
            .expect("new project handle should rebuild the live-output plan");

        assert!(Arc::ptr_eq(
            runtime
                .plan_project
                .as_ref()
                .expect("plan project should be updated"),
            &new_project
        ));
    }
}
