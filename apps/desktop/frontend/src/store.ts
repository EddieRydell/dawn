import { create } from "zustand";
import { commands, setGuiEditResultHandler } from "./api";
import type { AppSnapshot, GuiDocument, GuiDocumentRequest, GuiEditResult } from "./types";

type SnapshotApplySource = "event" | "command" | "hydrate";

type AppStore = {
  snapshot: AppSnapshot | null;
  guiRequest: GuiDocumentRequest | null;
  guiDocument: GuiDocument | null;
  error: string | null;
  localText: string;
  setSnapshot: (snapshot: AppSnapshot, source?: SnapshotApplySource) => void;
  setGuiRequest: (request: GuiDocumentRequest | null) => void;
  setGuiDocument: (document: GuiDocument | null) => void;
  applyGuiEditResult: (result: GuiEditResult) => void;
  setError: (error: string | null) => void;
  setLocalText: (text: string) => void;
  hydrate: () => Promise<void>;
};

export const useAppStore = create<AppStore>((set) => ({
  snapshot: null,
  guiRequest: null,
  guiDocument: null,
  error: null,
  localText: "",
  setSnapshot: (snapshot, source = "command") => {
    set({
      snapshot,
      localText: snapshot.activeBuffer?.text ?? ""
    });
    void source;
  },
  setGuiRequest: (guiRequest) => {
    set({ guiRequest });
  },
  setGuiDocument: (guiDocument) => {
    set({ guiDocument });
  },
  applyGuiEditResult: (result) => {
    set({
      snapshot: result.snapshot,
      guiDocument: result.document,
      localText: result.snapshot.activeBuffer?.text ?? ""
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
    set({ error: null });
  }
}));

setGuiEditResultHandler((result) => {
  useAppStore.getState().applyGuiEditResult(result);
  useAppStore.getState().setError(null);
});

export function subscribeToSnapshots(): Promise<() => void> {
  return Promise.resolve(() => {});
}

export async function runSnapshotCommand(command: () => Promise<AppSnapshot>) {
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
