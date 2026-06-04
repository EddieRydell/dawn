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

type RuntimeSlices = AppSnapshotDto;

type AppRuntimeChangedEvent = {
  snapshot: AppSnapshotDto;
  changedSlices: unknown[];
};

type AppStore = {
  runtimeSlices: RuntimeSlices | null;
  runtimeState: RuntimeUiState | null;
  error: string | null;
  localText: string;
  setRuntimeSlices: (runtimeSlices: RuntimeSlices) => void;
  setError: (error: string | null) => void;
  setLocalText: (text: string) => void;
  hydrate: () => Promise<void>;
};

export const useAppStore = create<AppStore>((set, get) => ({
  runtimeSlices: null,
  runtimeState: null,
  error: null,
  localText: "",
  setRuntimeSlices: (runtimeSlices) => {
    const previousActiveText = get().runtimeSlices?.editor.activeBuffer?.text;
    const nextActiveText = runtimeSlices.editor.activeBuffer?.text;
    set({
      runtimeSlices,
      runtimeState: composeruntimeState(runtimeSlices),
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
    const runtimeSlices = await commands.getAppSnapshot();
    set({
      runtimeSlices,
      runtimeState: composeruntimeState(runtimeSlices),
      localText: runtimeSlices.editor.activeBuffer?.text ?? "",
      error: null
    });
  }
}));

export async function subscribeToruntimeState() {
  const disposers = await Promise.all([
    listen<AppRuntimeChangedEvent>("app_runtime_changed", (event) => {
      useAppStore.getState().setRuntimeSlices(event.payload.snapshot);
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

function composeruntimeState(runtimeSlices: RuntimeSlices): RuntimeUiState {
  return {
    projectRoot: runtimeSlices.workspace.projectRoot,
    projectTreeVisible: runtimeSlices.workspace.projectTreeVisible,
    projectEntries: runtimeSlices.workspace.projectEntries,
    tabs: runtimeSlices.editor.tabs,
    activeFile: runtimeSlices.editor.activeFile,
    activeBuffer: runtimeSlices.editor.activeBuffer,
    activeDocumentDescriptor: runtimeSlices.activeDocument.descriptor,
    activeGuiDocument: runtimeSlices.activeDocument.guiDocument,
    diagnostics: runtimeSlices.diagnostics.diagnostics,
    status: runtimeSlices.status.status,
    preview: runtimeSlices.preview.preview,
    effectPreviewEnabled: runtimeSlices.preview.effectPreviewEnabled,
    liveOutput: runtimeSlices.liveOutput.liveOutput
  };
}
