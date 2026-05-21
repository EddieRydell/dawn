import { invoke } from "@tauri-apps/api/core";
import { create } from "zustand";
import type { FileOperationState, FrameSummary, ProjectState } from "../types";
import type { PanelId } from "../workbench/panelIds";

type PanelVisibility = Record<PanelId, boolean>;

type WorkbenchState = {
  projectPath: string;
  projectState: ProjectState | null;
  activeFile: string | null;
  fileContent: string;
  dirty: boolean;
  activeSequence: string | null;
  time: number;
  playing: boolean;
  frame: FrameSummary | null;
  status: string;
  panelVisibility: PanelVisibility;
  setProjectPath: (path: string) => void;
  setFileContent: (content: string) => void;
  setTime: (time: number) => void;
  setPlaying: (playing: boolean) => void;
  togglePlayback: () => void;
  setStatus: (status: string) => void;
  setPanelVisible: (panelId: PanelId, visible: boolean) => void;
  setPanelVisibility: (visibility: Partial<PanelVisibility>) => void;
  openProject: (path?: string) => Promise<void>;
  openFile: (path: string) => Promise<void>;
  saveFile: () => Promise<void>;
  runCheck: () => Promise<void>;
  renderFrame: () => Promise<void>;
  renamePath: (path: string, newName: string) => Promise<void>;
  movePaths: (paths: string[], newParent: string) => Promise<void>;
};

export const sampleProjectPath = "C:\\Users\\eddie\\donder\\target\\smoke-project";

const initialPanelVisibility: PanelVisibility = {
  project: true,
  editor: true,
  preview: true,
  problems: true,
  layout: false,
  output: false
};

export const useWorkbench = create<WorkbenchState>((set, get) => ({
  projectPath: "",
  projectState: null,
  activeFile: null,
  fileContent: "",
  dirty: false,
  activeSequence: null,
  time: 0,
  playing: false,
  frame: null,
  status: "Ready",
  panelVisibility: initialPanelVisibility,
  setProjectPath: (path) => set({ projectPath: path }),
  setFileContent: (content) => set({ fileContent: content, dirty: true }),
  setTime: (time) => set({ time }),
  setPlaying: (playing) => set({ playing, status: playing ? "Playback started" : "Playback paused" }),
  togglePlayback: () => {
    const playing = !get().playing;
    set({ playing, status: playing ? "Playback started" : "Playback paused" });
  },
  setStatus: (status) => set({ status }),
  setPanelVisible: (panelId, visible) =>
    set((state) => ({ panelVisibility: { ...state.panelVisibility, [panelId]: visible } })),
  setPanelVisibility: (visibility) =>
    set((state) => ({ panelVisibility: { ...state.panelVisibility, ...visibility } })),
  openProject: async (pathOverride) => {
    const path = pathOverride ?? get().projectPath;
    if (!path.trim()) {
      set({ status: "Enter a project folder or project.jsonc path." });
      return;
    }

    try {
      set({ status: "Opening project..." });
      const projectState = await invoke<ProjectState>("open_project", { path });
      set({ projectPath: path, projectState, status: `Opened ${projectState.root}` });
      if (projectState.files[0]) {
        await get().openFile(projectState.files[0]);
      } else {
        set({ activeFile: null, fileContent: "", dirty: false, activeSequence: null });
      }
    } catch (error) {
      set({ status: formatError(error) });
    }
  },
  openFile: async (path) => {
    try {
      set({ activeFile: path, status: `Opening ${path}` });
      const fileContent = await invoke<string>("read_file", { path });
      let activeSequence = get().activeSequence;
      if (path.endsWith(".sequence.jsonc")) {
        await invoke("open_sequence", { path });
        activeSequence = path;
      }
      set({ fileContent, dirty: false, activeSequence, status: `Editing ${path}` });
      if (path.endsWith(".sequence.jsonc")) {
        await get().renderFrame();
      }
    } catch (error) {
      set({ status: formatError(error) });
    }
  },
  saveFile: async () => {
    const { activeFile, fileContent } = get();
    if (!activeFile) return;

    try {
      await invoke("write_file", { path: activeFile, content: fileContent });
      set({ dirty: false, status: `Saved ${activeFile}` });
      await get().runCheck();
    } catch (error) {
      set({ status: formatError(error) });
    }
  },
  runCheck: async () => {
    try {
      set({ status: "Checking project..." });
      const projectState = await invoke<ProjectState>("check_project");
      const errors = projectState.diagnostics.filter((diagnostic) => diagnostic.severity === "Error").length;
      const warnings = projectState.diagnostics.length - errors;
      set({
        projectState,
        status: errors || warnings ? `Check complete: ${errors} errors, ${warnings} warnings` : "Check complete: no problems"
      });
    } catch (error) {
      set({ status: formatError(error) });
    }
  },
  renderFrame: async () => {
    const { activeSequence, time } = get();
    if (!activeSequence) {
      set({ frame: null });
      return;
    }

    try {
      set({ frame: await invoke<FrameSummary>("render_frame", { time }) });
    } catch {
      set({ frame: null });
    }
  },
  renamePath: async (path, newName) => {
    try {
      const result = await invoke<FileOperationState>("rename_path", { path, newName });
      applyFileOperationResult(result, set, get);
      set({ status: `Renamed ${path} to ${newName}` });
    } catch (error) {
      set({ status: formatError(error) });
    }
  },
  movePaths: async (paths, newParent) => {
    try {
      const result = await invoke<FileOperationState>("move_paths", { paths, newParent });
      applyFileOperationResult(result, set, get);
      set({ status: result.moved.length ? `Moved ${result.moved.length} item${result.moved.length === 1 ? "" : "s"}` : "Move skipped" });
    } catch (error) {
      set({ status: formatError(error) });
    }
  }
}));

function applyFileOperationResult(
  result: FileOperationState,
  set: (partial: Partial<WorkbenchState>) => void,
  get: () => WorkbenchState
) {
  const { activeFile, activeSequence } = get();
  const activeMove = result.moved.find((move) => move.oldPath === activeFile);
  const sequenceMove = result.moved.find((move) => move.oldPath === activeSequence);

  set({
    projectState: result.project,
    activeFile: activeMove?.newPath ?? activeFile,
    activeSequence: sequenceMove?.newPath ?? activeSequence
  });
}

function formatError(error: unknown) {
  return error instanceof Error ? error.message : String(error);
}
