import type { PanelId } from "./panelIds";

type WorkbenchDockBridge = {
  togglePanel: (panelId: PanelId) => void;
  resetLayout: () => void;
};

let bridge: WorkbenchDockBridge | null = null;

export function setWorkbenchDockBridge(nextBridge: WorkbenchDockBridge | null) {
  bridge = nextBridge;
}

export function toggleWorkbenchPanel(panelId: PanelId) {
  bridge?.togglePanel(panelId);
}

export function resetWorkbenchLayout() {
  bridge?.resetLayout();
}
