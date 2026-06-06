import { listen } from "@tauri-apps/api/event";
import { create } from "zustand";
import { commands } from "./api";
import type { AppSnapshotDto } from "./bindings";

type SnapshotApplySource = "event" | "command" | "hydrate";

type AppStore = {
  snapshot: AppSnapshotDto | null;
  error: string | null;
  localText: string;
  setSnapshot: (snapshot: AppSnapshotDto, source?: SnapshotApplySource) => void;
  setError: (error: string | null) => void;
  setLocalText: (text: string) => void;
  hydrate: () => Promise<void>;
};

export const useAppStore = create<AppStore>((set) => ({
  snapshot: null,
  error: null,
  localText: "",
  setSnapshot: (snapshot, source = "command") => {
    set({
      snapshot,
      localText: snapshot.activeBuffer?.text ?? ""
    });
    void source;
  },
  setError: (error) => {
    set({ error });
  },
  setLocalText: (localText) => {
    set({ localText });
  },
  hydrate: async () => {
    const snapshot = await commands.getSnapshot();
    useAppStore.getState().setSnapshot(snapshot, "hydrate");
    set({ error: null });
  }
}));

export async function subscribeToSnapshots() {
  const disposeSnapshots = await listen<AppSnapshotDto>("app_snapshot_changed", (event) => {
    useAppStore.getState().setSnapshot(event.payload, "event");
  });
  return () => {
    disposeSnapshots();
  };
}

export async function runSnapshotCommand(command: () => Promise<AppSnapshotDto>) {
  try {
    const snapshot = await command();
    useAppStore.getState().setSnapshot(snapshot, "command");
    useAppStore.getState().setError(null);
    return snapshot;
  } catch (error) {
    useAppStore.getState().setError(String(error));
    throw error;
  }
}
