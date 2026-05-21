import type { SerializedDockview } from "dockview-react";

const storageKey = "donder-layouts";
const version = 2;

type LayoutStore = {
  version: number;
  layouts: Record<string, SerializedDockview>;
};

export function loadDockLayout(layoutKey: string): SerializedDockview | null {
  try {
    const raw = window.localStorage.getItem(storageKey);
    if (!raw) return null;
    const parsed = JSON.parse(raw) as Partial<LayoutStore>;
    if (parsed.version !== version || !parsed.layouts) return null;
    return parsed.layouts[layoutKey] ?? null;
  } catch {
    return null;
  }
}

export function saveDockLayout(layoutKey: string, layout: SerializedDockview) {
  const store = readStore();
  store.layouts[layoutKey] = layout;
  window.localStorage.setItem(storageKey, JSON.stringify(store));
}

export function clearDockLayout(layoutKey: string) {
  const store = readStore();
  delete store.layouts[layoutKey];
  window.localStorage.setItem(storageKey, JSON.stringify(store));
}

function readStore(): LayoutStore {
  try {
    const raw = window.localStorage.getItem(storageKey);
    if (raw) {
      const parsed = JSON.parse(raw) as Partial<LayoutStore>;
      if (parsed.version === version && parsed.layouts) {
        return { version, layouts: parsed.layouts };
      }
    }
  } catch {
    // Ignore corrupt UI state and rebuild the small layout cache.
  }

  return { version, layouts: {} };
}
