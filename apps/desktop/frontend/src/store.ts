import { listen } from "@tauri-apps/api/event";
import { create } from "zustand";
import { commands } from "./api";
import type {
  ActiveDocumentReadModelDto,
  DiagnosticsReadModelDto,
  EditorReadModelDto,
  LiveOutputReadModelDto,
  PrefsReadModelDto,
  PreviewReadModelDto,
  RuntimeCommandResultDto,
  RuntimeReadModelsDto,
  StatusReadModelDto,
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

type RuntimeSlices = RuntimeReadModelsDto;

type AppStore = {
  runtimeSlices: RuntimeSlices | null;
  runtimeState: RuntimeUiState | null;
  error: string | null;
  localText: string;
  setRuntimeSlices: (runtimeSlices: RuntimeSlices) => void;
  updateWorkspace: (workspace: WorkspaceReadModelDto) => void;
  updateEditor: (editor: EditorReadModelDto) => void;
  updateActiveDocument: (activeDocument: ActiveDocumentReadModelDto) => void;
  updateDiagnostics: (diagnostics: DiagnosticsReadModelDto) => void;
  updatePreview: (preview: PreviewReadModelDto) => void;
  updateLiveOutput: (liveOutput: LiveOutputReadModelDto) => void;
  updateStatus: (status: StatusReadModelDto) => void;
  updatePrefs: (prefs: PrefsReadModelDto) => void;
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
    set({
      runtimeSlices,
      runtimeState: composeRuntimeState(runtimeSlices),
      localText: runtimeSlices.editor.activeBuffer?.text ?? ""
    });
  },
  updateWorkspace: (workspace) => {
    updateSlice(set, get, { workspace });
  },
  updateEditor: (editor) => {
    updateSlice(set, get, { editor });
  },
  updateActiveDocument: (activeDocument) => {
    updateSlice(set, get, { activeDocument });
  },
  updateDiagnostics: (diagnostics) => {
    updateSlice(set, get, { diagnostics });
  },
  updatePreview: (preview) => {
    updateSlice(set, get, { preview });
  },
  updateLiveOutput: (liveOutput) => {
    updateSlice(set, get, { liveOutput });
  },
  updateStatus: (status) => {
    updateSlice(set, get, { status });
  },
  updatePrefs: (prefs) => {
    updateSlice(set, get, { prefs });
  },
  setError: (error) => {
    set({ error });
  },
  setLocalText: (localText) => {
    set({ localText });
  },
  hydrate: async () => {
    const runtimeSlices = await commands.getRuntimeReadModels();
    set({
      runtimeSlices,
      runtimeState: composeRuntimeState(runtimeSlices),
      localText: runtimeSlices.editor.activeBuffer?.text ?? "",
      error: null
    });
  }
}));

export async function subscribeToRuntimeState() {
  const disposers = await Promise.all([
    listen<WorkspaceReadModelDto>("runtime_workspace_changed", (event) => {
      useAppStore.getState().updateWorkspace(event.payload);
    }),
    listen<EditorReadModelDto>("runtime_editor_changed", (event) => {
      useAppStore.getState().updateEditor(event.payload);
    }),
    listen<ActiveDocumentReadModelDto>("runtime_active_document_changed", (event) => {
      useAppStore.getState().updateActiveDocument(event.payload);
    }),
    listen<DiagnosticsReadModelDto>("runtime_diagnostics_changed", (event) => {
      useAppStore.getState().updateDiagnostics(event.payload);
    }),
    listen<PreviewReadModelDto>("runtime_preview_changed", (event) => {
      useAppStore.getState().updatePreview(event.payload);
    }),
    listen<LiveOutputReadModelDto>("runtime_live_output_changed", (event) => {
      useAppStore.getState().updateLiveOutput(event.payload);
    }),
    listen<StatusReadModelDto>("runtime_status_changed", (event) => {
      useAppStore.getState().updateStatus(event.payload);
    }),
    listen<PrefsReadModelDto>("runtime_prefs_changed", (event) => {
      useAppStore.getState().updatePrefs(event.payload);
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
    if (isRuntimeCommandResult(result) && result === "changed") {
      useAppStore.getState().setRuntimeSlices(await commands.getRuntimeReadModels());
    }
    useAppStore.getState().setError(null);
    return result;
  } catch (error) {
    useAppStore.getState().setError(String(error));
    throw error;
  }
}

function updateSlice(
  set: (partial: Partial<AppStore>) => void,
  get: () => AppStore,
  update: Partial<RuntimeSlices>
) {
  const current = get().runtimeSlices;
  if (current === null) {
    return;
  }
  const runtimeSlices = { ...current, ...update };
  const previousActiveText = current.editor.activeBuffer?.text;
  const nextActiveText = runtimeSlices.editor.activeBuffer?.text;
  set({
    runtimeSlices,
    runtimeState: composeRuntimeState(runtimeSlices),
    ...(previousActiveText !== nextActiveText ? { localText: nextActiveText ?? "" } : {})
  });
}

function composeRuntimeState(runtimeSlices: RuntimeSlices): RuntimeUiState {
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

function isRuntimeCommandResult(value: unknown): value is RuntimeCommandResultDto {
  return value === "changed" || value === "unchanged";
}
