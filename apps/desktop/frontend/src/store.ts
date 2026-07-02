import { create } from "zustand";
import { listen } from "@tauri-apps/api/event";
import { commands, setGuiEditResultHandler } from "./api";
import { effectiveEditorViewMode } from "./editorViewMode";
import type { AppSnapshot, AudioTransportSnapshot, GuiDocument, GuiDocumentRequest, GuiEditResult, ProjectRestoreState } from "./types";

type SnapshotApplySource = "event" | "command" | "hydrate";

type AppStore = {
  snapshot: AppSnapshot | null;
  restoreState: ProjectRestoreState | null;
  guiRequest: GuiDocumentRequest | null;
  guiDocument: GuiDocument | null;
  guiResetRevision: number;
  error: string | null;
  localText: string;
  setSnapshot: (snapshot: AppSnapshot, source?: SnapshotApplySource) => void;
  setRestoreState: (restoreState: ProjectRestoreState | null) => void;
  setGuiRequest: (request: GuiDocumentRequest | null) => void;
  setGuiDocument: (document: GuiDocument | null) => void;
  resetGuiLocalState: () => void;
  applyGuiEditResult: (result: GuiEditResult) => void;
  setError: (error: string | null) => void;
  setLocalText: (text: string) => void;
  hydrate: () => Promise<void>;
};

export const useAppStore = create<AppStore>((set) => ({
  snapshot: null,
  restoreState: null,
  guiRequest: null,
  guiDocument: null,
  guiResetRevision: 0,
  error: null,
  localText: "",
  setSnapshot: (snapshot, source = "command") => {
    set((current) => {
      const currentSnapshot = current.snapshot;
      const audioTransport = mergeAudioTransport(
        currentSnapshot?.audioTransport ?? null,
        snapshot.audioTransport,
        source
      );
      const nextState: Partial<AppStore> = {
        snapshot: {
          ...snapshot,
          audioTransport
        }
      };
      if (snapshot.activeBuffer !== null && effectiveEditorViewMode(snapshot) === "text") {
        nextState.localText = snapshot.activeBuffer.text;
      }
      return nextState;
    });
    void source;
  },
  setGuiRequest: (guiRequest) => {
    set({ guiRequest });
  },
  setRestoreState: (restoreState) => {
    set({ restoreState });
  },
  setGuiDocument: (guiDocument) => {
    set({ guiDocument });
  },
  resetGuiLocalState: () => {
    set((current) => ({ guiResetRevision: current.guiResetRevision + 1 }));
  },
  applyGuiEditResult: (result) => {
    set((current) => {
      const audioTransport = mergeAudioTransport(
        current.snapshot?.audioTransport ?? null,
        result.snapshot.audioTransport,
        "command"
      );
      return {
        snapshot: {
          ...result.snapshot,
          audioTransport
        },
        guiDocument: result.document
      };
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
    useAppStore.getState().setSnapshot(snapshot, "hydrate");
    const restoreState = await commands.getRestoredViewState();
    useAppStore.getState().setRestoreState(restoreState);
    set({ error: null });
  }
}));

setGuiEditResultHandler((result) => {
  useAppStore.getState().applyGuiEditResult(result);
  useAppStore.getState().setError(null);
});

export function subscribeToSnapshots(): Promise<() => void> {
  return Promise.all([
    listen<AudioTransportSnapshot>("audio_transport_changed", (event) => {
      const current = useAppStore.getState().snapshot;
      if (current === null) return;
      if (event.payload.generation < current.audioTransport.generation) return;
      useAppStore.getState().setSnapshot(
        {
          ...current,
          audioTransport: event.payload
        },
        "event"
      );
    }),
    listen<boolean>("preview_window_changed", (event) => {
      const current = useAppStore.getState().snapshot;
      if (current === null) return;
      useAppStore.getState().setSnapshot(
        {
          ...current,
          previewOpen: event.payload
        },
        "event"
      );
    })
  ]).then((unsubscribes) => () => {
    for (const unsubscribe of unsubscribes) {
      unsubscribe();
    }
  });
}

export async function runSnapshotCommand(command: () => Promise<AppSnapshot>) {
  try {
    const previousRoot = useAppStore.getState().snapshot?.projectRoot ?? null;
    const snapshot = await command();
    useAppStore.getState().setSnapshot(snapshot, "command");
    if (snapshot.projectRoot !== previousRoot) {
      const restoreState = await commands.getRestoredViewState();
      useAppStore.getState().setRestoreState(restoreState);
    }
    useAppStore.getState().setError(null);
    return snapshot;
  } catch (error) {
    useAppStore.getState().setError(String(error));
    throw error;
  }
}

export async function runGuiEditCommand(command: () => Promise<GuiEditResult>) {
  try {
    const previousRoot = useAppStore.getState().snapshot?.projectRoot ?? null;
    const result = await command();
    if (result.snapshot.projectRoot !== previousRoot) {
      const restoreState = await commands.getRestoredViewState();
      useAppStore.getState().setRestoreState(restoreState);
    }
    useAppStore.getState().setError(null);
    return result;
  } catch (error) {
    useAppStore.getState().setError(String(error));
    throw error;
  }
}

function mergeAudioTransport(
  current: AudioTransportSnapshot | null,
  incoming: AudioTransportSnapshot,
  source: SnapshotApplySource
): AudioTransportSnapshot {
  if (current === null) return incoming;
  if (incoming.generation < current.generation) return current;
  if (source === "command" && incoming.generation === current.generation) return current;
  return incoming;
}
