import { create } from "zustand";
import { listen } from "@tauri-apps/api/event";
import { commands, setGuiEditResultHandler } from "./api";
import { effectiveEditorViewMode } from "./editorViewMode";
import type { AppSnapshot, AudioTransportSnapshot, GuiDocument, GuiDocumentRequest, GuiEditResult, LiveOutputSnapshot, ProjectRestoreState } from "./types";

type SnapshotApplySource = "event" | "command" | "hydrate";

type AppStore = {
  snapshot: AppSnapshot | null;
  restoreState: ProjectRestoreState | null;
  guiRequest: GuiDocumentRequest | null;
  guiDocument: GuiDocument | null;
  guiResetRevision: number;
  compositionGraphEditing: boolean;
  error: string | null;
  localText: string;
  setSnapshot: (snapshot: AppSnapshot, source?: SnapshotApplySource, preserveLocalText?: boolean) => void;
  setRestoreState: (restoreState: ProjectRestoreState | null) => void;
  setGuiRequest: (request: GuiDocumentRequest | null) => void;
  setGuiDocument: (document: GuiDocument | null) => void;
  resetGuiLocalState: () => void;
  setCompositionGraphEditing: (editing: boolean) => void;
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
  compositionGraphEditing: false,
  error: null,
  localText: "",
  setSnapshot: (snapshot, source = "command", preserveLocalText = false) => {
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
      if (!preserveLocalText && snapshot.activeBuffer !== null && effectiveEditorViewMode(snapshot) === "text") {
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
    set((current) => ({
      guiResetRevision: current.guiResetRevision + 1,
      compositionGraphEditing: false
    }));
  },
  setCompositionGraphEditing: (compositionGraphEditing) => {
    set({ compositionGraphEditing });
  },
  applyGuiEditResult: (result) => {
    set((current) => {
      const audioTransport = mergeAudioTransport(
        current.snapshot?.audioTransport ?? null,
        result.snapshot.audioTransport,
        "command"
      );
      const nextState: Partial<AppStore> = {
        snapshot: {
          ...result.snapshot,
          audioTransport
        },
        guiDocument: result.document
      };
      if (result.snapshot.activeBuffer !== null && effectiveEditorViewMode(result.snapshot) === "text") {
        nextState.localText = result.snapshot.activeBuffer.text;
      }
      return nextState;
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
    }),
    listen<LiveOutputSnapshot>("live_output_changed", (event) => {
      const current = useAppStore.getState().snapshot;
      if (current === null || event.payload.generation < current.liveOutput.generation) return;
      useAppStore.getState().setSnapshot({ ...current, liveOutput: event.payload }, "event");
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
