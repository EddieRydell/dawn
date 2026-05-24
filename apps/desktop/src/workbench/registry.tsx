import type { FunctionComponent } from "react";
import { PreviewPanel } from "../panels/PreviewPanel";
import { ProjectPanel } from "../panels/ProjectPanel";
import { SourceEditorPanel } from "../panels/SourceEditorPanel";
import type { PanelId } from "./panelIds";

export type PanelDefinition = {
  id: PanelId;
  title: string;
  component: FunctionComponent;
  preferredWidth?: number;
  preferredHeight?: number;
  minimumWidth?: number;
  minimumHeight?: number;
};

export const panelRegistry: Record<PanelId, PanelDefinition> = {
  project: {
    id: "project",
    title: "Project",
    component: ProjectPanel,
    preferredWidth: 280,
    minimumWidth: 220
  },
  editor: {
    id: "editor",
    title: "Editor",
    component: SourceEditorPanel,
    minimumWidth: 360,
    minimumHeight: 240
  },
  preview: {
    id: "preview",
    title: "Preview",
    component: PreviewPanel,
    preferredWidth: 360,
    minimumWidth: 280,
    minimumHeight: 220
  }
};

export const dockComponents = Object.fromEntries(
  Object.values(panelRegistry).map((definition) => [definition.id, definition.component])
) as Record<PanelId, FunctionComponent>;

export const registeredPanels = Object.values(panelRegistry);
