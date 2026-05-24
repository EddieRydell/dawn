export const panelIds = ["project", "editor", "preview", "layout", "output"] as const;

export type PanelId = (typeof panelIds)[number];

export function isPanelId(value: string): value is PanelId {
  return (panelIds as readonly string[]).includes(value);
}
