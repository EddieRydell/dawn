use crate::output::live_output::LiveOutputReadout;
use crate::RenderedFrame;

use super::AppBackend;

impl AppBackend {
    pub(super) fn sync_live_output_readout(&mut self, readout: LiveOutputReadout) {
        self.live_output.sync_readout(readout);
    }

    pub(super) fn live_output_readout(&self) -> LiveOutputReadout {
        self.live_output.readout()
    }

    pub(super) fn live_output_enabled(&self) -> bool {
        self.live_output.enabled()
    }

    pub(super) fn set_live_output_enabled_command(&mut self, enabled: bool) {
        self.live_output
            .set_enabled(enabled, self.workspace.analysis());
    }

    pub(super) fn send_live_output_frame(&mut self, frame: &RenderedFrame) -> bool {
        let previous = self.live_output.readout();
        self.live_output
            .send_frame(self.workspace.analysis(), frame);
        self.live_output.readout() != previous
    }
}
