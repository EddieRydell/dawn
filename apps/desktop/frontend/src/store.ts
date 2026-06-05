import { listen } from "@tauri-apps/api/event";
import { create } from "zustand";
import { commands } from "./api";
import type {
  ActiveDocumentReadModelDto,
  DiagnosticsReadModelDto,
  EditorReadModelDto,
  LiveOutputReadModelDto,
  PreviewReadModelDto,
  AppSnapshotDto,
  RuntimeStatusDto,
  WorkspaceReadModelDto
} from "./bindings";

export type RuntimeUiState = {
  projectRoot: string | null;
  projectTreeVisible: boolean;
  projectEntries: WorkspaceReadModelDto["projectEntries"];
  tabs: EditorReadModelDto["tabs"];
  activeFile: string | null;
  activeBuffer: EditorReadModelDto["activeBuffer"];
  activeDocumentDescriptor: ActiveDocumentReadModelDto["descriptor"];
  activeGuiDocument: ActiveDocumentReadModelDto["guiDocument"];
  diagnostics: DiagnosticsReadModelDto["diagnostics"];
  status: string;
  preview: PreviewReadModelDto["preview"];
  effectPreviewEnabled: boolean;
  liveOutput: LiveOutputReadModelDto["liveOutput"];
};

type BackendSlices = AppSnapshotDto;

type AppBackendChangedEvent = {
  snapshot: AppSnapshotDto;
  changedSlices: unknown[];
};

type AppStore = {
  BackendSlices: BackendSlices | null;
  runtimeState: RuntimeUiState | null;
  error: string | null;
  localText: string;
  setBackendSlices: (BackendSlices: BackendSlices) => void;
  setError: (error: string | null) => void;
  setLocalText: (text: string) => void;
  hydrate: () => Promise<void>;
};

export const useAppStore = create<AppStore>((set, get) => ({
  BackendSlices: null,
  runtimeState: null,
  error: null,
  localText: "",
  setBackendSlices: (BackendSlices) => {
    const previousActiveText = get().BackendSlices?.editor.activeBuffer?.text;
    const nextActiveText = BackendSlices.editor.activeBuffer?.text;
    set({
      BackendSlices,
      runtimeState: composeruntimeState(BackendSlices),
      ...(previousActiveText !== nextActiveText ? { localText: nextActiveText ?? "" } : {})
    });
  },
  setError: (error) => {
    set({ error });
  },
  setLocalText: (localText) => {
    set({ localText });
  },
  hydrate: async () => {
    const BackendSlices = await commands.getAppSnapshot();
    set({
      BackendSlices,
      runtimeState: composeruntimeState(BackendSlices),
      localText: BackendSlices.editor.activeBuffer?.text ?? "",
      error: null
    });
  }
}));

export async function subscribeToruntimeState() {
  const disposers = await Promise.all([
    listen<AppBackendChangedEvent>("app_backend_changed", (event) => {
      useAppStore.getState().setBackendSlices(event.payload.snapshot);
    })
  ]);
  return () => {
    for (const dispose of disposers) {
      dispose();
    }
  };
}

export async function runRuntimeCommand<T>(command: () => Promise<T>) {
  try {
    const result = await command();
    useAppStore.getState().setError(null);
    return result;
  } catch (error) {
    useAppStore.getState().setError(String(error));
    throw error;
  }
}

function composeruntimeState(BackendSlices: BackendSlices): RuntimeUiState {
  return {
    projectRoot: BackendSlices.workspace.projectRoot,
    projectTreeVisible: BackendSlices.workspace.projectTreeVisible,
    projectEntries: BackendSlices.workspace.projectEntries,
    tabs: BackendSlices.editor.tabs,
    activeFile: BackendSlices.editor.activeFile,
    activeBuffer: BackendSlices.editor.activeBuffer,
    activeDocumentDescriptor: BackendSlices.activeDocument.descriptor,
    activeGuiDocument: BackendSlices.activeDocument.guiDocument,
    diagnostics: BackendSlices.diagnostics.diagnostics,
    status: runtimeStatusLabel(BackendSlices.status.status),
    preview: BackendSlices.preview.preview,
    effectPreviewEnabled: BackendSlices.preview.effectPreviewEnabled,
    liveOutput: BackendSlices.liveOutput.liveOutput
  };
}

function runtimeStatusLabel(status: RuntimeStatusDto): string {
  switch (status.type) {
    case "noProjectOpen":
      return "No project open";
    case "saved":
      return "Saved";
    case "message":
      return status.message;
  }
}
