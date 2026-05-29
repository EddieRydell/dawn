import { listen } from "@tauri-apps/api/event";
import { create } from "zustand";
import { commands } from "./api";
import type { AppSnapshotDto } from "./bindings";

type AppStore = {
  snapshot: AppSnapshotDto | null;
  error: string | null;
  localText: string;
  setSnapshot: (snapshot: AppSnapshotDto) => void;
  setError: (error: string | null) => void;
  setLocalText: (text: string) => void;
  hydrate: () => Promise<void>;
};

export const useAppStore = create<AppStore>((set) => ({
  snapshot: null,
  error: null,
  localText: "",
  setSnapshot: (snapshot) => {
    set({
      snapshot,
      localText: snapshot.activeBuffer?.text ?? ""
    });
  },
  setError: (error) => {
    set({ error });
  },
  setLocalText: (localText) => {
    set({ localText });
  },
  hydrate: async () => {
    const snapshot = await commands.getSnapshot();
    set({ snapshot, localText: snapshot.activeBuffer?.text ?? "", error: null });
  }
}));

export async function subscribeToSnapshots() {
  const disposeSnapshots = await listen<AppSnapshotDto>("app_snapshot_changed", (event) => {
    useAppStore.getState().setSnapshot(event.payload);
  });
  return () => {
    disposeSnapshots();
  };
}

export async function runSnapshotCommand(command: () => Promise<AppSnapshotDto>) {
  try {
    const snapshot = await command();
    useAppStore.getState().setSnapshot(snapshot);
    useAppStore.getState().setError(null);
    return snapshot;
  } catch (error) {
    useAppStore.getState().setError(String(error));
    throw error;
  }
}
