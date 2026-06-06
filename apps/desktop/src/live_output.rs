use std::collections::HashMap;
use std::net::{SocketAddr, UdpSocket};
use std::sync::Arc;

use dawn_app_core::app_model::{LiveOutputSnapshot, LiveOutputStatus};
use dawn_app_core::controller_output::{
    build_output_plan, encode_e131_data_packet, ControllerOutputPlan,
};
use dawn_app_core::output_runtime::OutputFrame;
use dawn_project::analysis::ProjectAnalysis;

#[derive(Debug, Default)]
pub(crate) struct LiveOutputRuntime {
    enabled: bool,
    socket: Option<UdpSocket>,
    plan: Option<ControllerOutputPlan>,
    plan_analysis: Option<Arc<ProjectAnalysis>>,
    sequence_counters: HashMap<UniverseSequenceKey, u8>,
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
        analysis: Option<Arc<ProjectAnalysis>>,
    ) -> LiveOutputSnapshot {
        if enabled {
            match self.enable(analysis) {
                Ok(()) => {}
                Err(error) => {
                    self.enabled = true;
                    self.socket = None;
                    self.plan = None;
                    self.plan_analysis = None;
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
        analysis: Option<Arc<ProjectAnalysis>>,
        frame: &OutputFrame,
    ) -> LiveOutputSnapshot {
        if !self.enabled {
            return self.snapshot();
        }
        let Some(analysis) = analysis else {
            self.set_error("project analysis is not available".to_string(), 0);
            return self.snapshot();
        };
        let active_universe_count = match self.ensure_plan(analysis) {
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
            Ok(()) => {
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

    fn ensure_plan(&mut self, analysis: Arc<ProjectAnalysis>) -> Result<usize, String> {
        if self
            .plan_analysis
            .as_ref()
            .is_none_or(|cached| !Arc::ptr_eq(cached, &analysis))
        {
            let plan = match build_output_plan(&analysis) {
                Ok(plan) => plan,
                Err(error) => {
                    self.plan = None;
                    self.plan_analysis = None;
                    return Err(error.to_string());
                }
            };
            self.plan = Some(plan);
            self.plan_analysis = Some(analysis.clone());
        }
        let Some(plan) = self.plan.as_ref() else {
            return Err("live output plan is not available".to_string());
        };
        Ok(plan.active_universe_count())
    }

    fn enable(&mut self, analysis: Option<Arc<ProjectAnalysis>>) -> Result<(), String> {
        let analysis = analysis.ok_or_else(|| "project analysis is not available".to_string())?;
        let plan = build_output_plan(&analysis).map_err(|error| error.to_string())?;
        let active_universe_count = plan.active_universe_count();
        let socket = UdpSocket::bind("0.0.0.0:0").map_err(|error| error.to_string())?;
        self.enabled = true;
        self.socket = Some(socket);
        self.plan = Some(plan);
        self.plan_analysis = Some(analysis);
        self.sequence_counters.clear();
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
        self.plan_analysis = None;
        self.sequence_counters.clear();
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

    fn send_buffers(
        &mut self,
        buffers: Vec<dawn_app_core::controller_output::ControllerUniverseFrame>,
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

#[cfg(test)]
mod tests {
    use std::path::Path;
    use std::sync::Arc;

    use dawn_project::analysis::analyze_project;
    use dawn_project::fs::WorkspaceFs;
    use dawn_project::path::utf8_path;

    use super::LiveOutputRuntime;

    #[test]
    fn output_plan_is_reused_until_analysis_handle_changes() {
        let project_path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../examples/thirty-output-controller/project.dawn");
        let root = project_path
            .parent()
            .expect("thirty output controller project should have a parent");
        let fs = WorkspaceFs::open(root).expect("thirty output controller root should open");
        let relative_project_path = utf8_path(
            project_path
                .strip_prefix(root)
                .expect("project path should be under root"),
        )
        .expect("project path should be valid UTF-8");
        let analysis = Arc::new(analyze_project(
            &fs,
            relative_project_path,
            "thirty_output_controller",
        ));
        assert!(
            analysis.diagnostics.is_empty(),
            "{:?}",
            analysis.diagnostics
        );
        let mut runtime = LiveOutputRuntime::default();

        let active_universes = runtime
            .ensure_plan(analysis.clone())
            .expect("analysis should build a live-output plan");
        let cached = runtime
            .plan_analysis
            .as_ref()
            .expect("plan analysis should be cached")
            .clone();
        let reused_active_universes = runtime
            .ensure_plan(analysis.clone())
            .expect("same analysis handle should reuse the cached plan");

        assert_eq!(active_universes, reused_active_universes);
        assert!(Arc::ptr_eq(
            runtime
                .plan_analysis
                .as_ref()
                .expect("plan analysis should remain cached"),
            &cached
        ));

        let new_analysis = Arc::new((*analysis).clone());
        runtime
            .ensure_plan(new_analysis.clone())
            .expect("new analysis handle should rebuild the live-output plan");

        assert!(Arc::ptr_eq(
            runtime
                .plan_analysis
                .as_ref()
                .expect("plan analysis should be updated"),
            &new_analysis
        ));
    }
}
