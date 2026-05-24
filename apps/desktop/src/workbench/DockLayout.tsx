import { DockviewReact, type DockviewApi, type DockviewReadyEvent, type IDockviewPanel } from "dockview-react";
import { useCallback, useEffect, useRef } from "react";
import { useWorkbench } from "../store/workbenchStore";
import { DockHeaderActions, DockTab } from "./DockChrome";
import { setWorkbenchDockBridge } from "./dockBridge";
import { clearDockLayout, loadDockLayout, saveDockLayout } from "./persistence";
import { isPanelId, panelIds, type PanelId } from "./panelIds";
import { dockComponents, panelRegistry } from "./registry";

const layoutKey = "main-v2";

export function DockLayout() {
  const apiRef = useRef<DockviewApi | null>(null);
  const restoringRef = useRef(false);
  const setPanelVisibility = useWorkbench((state) => state.setPanelVisibility);
  const setPanelVisible = useWorkbench((state) => state.setPanelVisible);

  const syncPanelVisibility = useCallback(
    (api: DockviewApi) => {
      const visible = Object.fromEntries(panelIds.map((panelId) => [panelId, Boolean(api.getPanel(panelId))])) as Record<PanelId, boolean>;
      setPanelVisibility(visible);
    },
    [setPanelVisibility]
  );

  const persistLayout = useCallback(
    (api: DockviewApi) => {
      if (!restoringRef.current) {
        saveDockLayout(layoutKey, api.toJSON());
      }
      syncPanelVisibility(api);
    },
    [syncPanelVisibility]
  );

  const onReady = useCallback(
    (event: DockviewReadyEvent) => {
      apiRef.current = event.api;
      restoringRef.current = true;

      const savedLayout = loadDockLayout(layoutKey);
      if (savedLayout) {
        try {
          event.api.fromJSON(savedLayout);
          if (event.api.totalPanels === 0) {
            event.api.clear();
            addDefaultLayout(event.api);
          }
        } catch {
          event.api.clear();
          addDefaultLayout(event.api);
        }
      } else {
        addDefaultLayout(event.api);
      }

      restoringRef.current = false;
      persistLayout(event.api);

      const disposables = [
        event.api.onDidLayoutChange(() => persistLayout(event.api)),
        event.api.onDidAddPanel((panel) => {
          if (isPanelId(panel.id)) setPanelVisible(panel.id, true);
        }),
        event.api.onDidRemovePanel((panel) => {
          if (isPanelId(panel.id)) setPanelVisible(panel.id, false);
        })
      ];

      return () => disposables.forEach((disposable) => disposable.dispose());
    },
    [persistLayout, setPanelVisible]
  );

  useEffect(() => {
    setWorkbenchDockBridge({
      togglePanel: (panelId) => togglePanel(apiRef.current, panelId),
      resetLayout: () => {
        const api = apiRef.current;
        if (!api) return;
        clearDockLayout(layoutKey);
        restoringRef.current = true;
        api.clear();
        addDefaultLayout(api);
        restoringRef.current = false;
        persistLayout(api);
      }
    });

    return () => setWorkbenchDockBridge(null);
  }, [persistLayout]);

  return (
    <div className="dock-layout dockview-theme-dark">
      <DockviewReact
        components={dockComponents}
        defaultTabComponent={DockTab}
        rightHeaderActionsComponent={DockHeaderActions}
        onReady={onReady}
        disableFloatingGroups
      />
    </div>
  );
}

function addDefaultLayout(api: DockviewApi) {
  addPanel(api, "editor");
  addPanel(api, "project", {
    position: { direction: "left", referencePanel: "editor" },
    initialWidth: panelRegistry.project.preferredWidth
  });
  addPanel(api, "preview", {
    position: { direction: "right", referencePanel: "editor" },
    initialWidth: panelRegistry.preview.preferredWidth
  });
}

function togglePanel(api: DockviewApi | null, panelId: PanelId) {
  if (!api) return;
  const panel = api.getPanel(panelId);
  if (panel) {
    api.removePanel(panel);
    return;
  }

  addPanel(api, panelId, getTogglePosition(api, panelId));
}

function getTogglePosition(api: DockviewApi, panelId: PanelId) {
  if (panelId === "project" && api.getPanel("editor")) {
    return { position: { direction: "left" as const, referencePanel: "editor" }, initialWidth: panelRegistry.project.preferredWidth };
  }
  if (panelId === "preview" && api.getPanel("editor")) {
    return { position: { direction: "right" as const, referencePanel: "editor" }, initialWidth: panelRegistry.preview.preferredWidth };
  }
  return {};
}

function addPanel(
  api: DockviewApi,
  panelId: PanelId,
  options: {
    position?: { direction: "left" | "right" | "above" | "below" | "within"; referencePanel: string };
    initialWidth?: number;
    initialHeight?: number;
  } = {}
): IDockviewPanel {
  const definition = panelRegistry[panelId];
  return api.addPanel({
    id: definition.id,
    component: definition.id,
    title: definition.title,
    minimumWidth: definition.minimumWidth,
    minimumHeight: definition.minimumHeight,
    ...options
  });
}
