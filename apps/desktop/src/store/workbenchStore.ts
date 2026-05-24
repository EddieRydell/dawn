import { invoke } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";
import { create } from "zustand";
import type { AnalysisState, FileOperationState, FrameSummary, LanguageProblem, ProjectState } from "../types";
import type { PanelId } from "../workbench/panelIds";

type PanelVisibility = Record<PanelId, boolean>;

export type OpenEditor = {
  path: string;
  content: string;
  dirty: boolean;
};

type WorkbenchState = {
  projectState: ProjectState | null;
  languageProblems: LanguageProblem[];
  pendingRevealProblem: LanguageProblem | null;
  activeFile: string | null;
  openEditors: OpenEditor[];
  activeSequence: string | null;
  time: number;
  playing: boolean;
  frame: FrameSummary | null;
  status: string;
  panelVisibility: PanelVisibility;
  setFileContent: (content: string) => void;
  setEditorContent: (path: string, content: string) => void;
  activateFile: (path: string) => void;
  activateNextEditor: (direction: 1 | -1) => void;
  closeFile: (path: string) => Promise<void>;
  setTime: (time: number) => void;
  setPlaying: (playing: boolean) => void;
  togglePlayback: () => void;
  setStatus: (status: string) => void;
  setLanguageProblems: (languageProblems: LanguageProblem[]) => void;
  openProblem: (problem: LanguageProblem) => Promise<void>;
  clearPendingRevealProblem: () => void;
  setPanelVisible: (panelId: PanelId, visible: boolean) => void;
  setPanelVisibility: (visibility: Partial<PanelVisibility>) => void;
  openProjectDialog: () => Promise<void>;
  openProject: (path: string) => Promise<void>;
  closeProject: () => Promise<void>;
  openFile: (path: string) => Promise<void>;
  saveFile: () => Promise<void>;
  reloadFile: () => Promise<void>;
  runCheck: () => Promise<void>;
  renderFrame: () => Promise<void>;
  renamePath: (path: string, newName: string) => Promise<void>;
  movePaths: (paths: string[], newParent: string) => Promise<void>;
};

type WorkbenchSet = (partial: Partial<WorkbenchState> | ((state: WorkbenchState) => Partial<WorkbenchState>)) => void;

const autosaveTimers = new Map<string, ReturnType<typeof setTimeout>>();
const analysisTimers = new Map<string, ReturnType<typeof setTimeout>>();

const initialPanelVisibility: PanelVisibility = {
  project: true,
  editor: true,
  preview: true,
  layout: false,
  output: false
};

export const useWorkbench = create<WorkbenchState>((set, get) => ({
  projectState: null,
  languageProblems: [],
  pendingRevealProblem: null,
  activeFile: null,
  openEditors: [],
  activeSequence: null,
  time: 0,
  playing: false,
  frame: null,
  status: "Ready",
  panelVisibility: initialPanelVisibility,
  setFileContent: (content) => {
    const { activeFile } = get();
    if (!activeFile) return;

    updateEditorContent(activeFile, content, set, get);
  },
  setEditorContent: (path, content) => {
    updateEditorContent(path, content, set, get);
  },
  activateFile: (path) => activateEditor(path, set, get),
  activateNextEditor: (direction) => {
    const { activeFile, openEditors } = get();
    if (openEditors.length === 0) return;

    const currentIndex = Math.max(0, openEditors.findIndex((editor) => editor.path === activeFile));
    const nextIndex = (currentIndex + direction + openEditors.length) % openEditors.length;
    activateEditor(openEditors[nextIndex].path, set, get);
  },
  closeFile: async (path) => {
    const saved = await saveEditor(path, set, get, "Saved");
    if (!saved) return;
    clearAutosave(path);

    const { activeFile, openEditors, activeSequence } = get();
    const closedIndex = openEditors.findIndex((editor) => editor.path === path);
    const nextEditors = openEditors.filter((editor) => editor.path !== path);
    const nextActiveFile = activeFile === path
      ? nextEditors[Math.min(closedIndex, nextEditors.length - 1)]?.path ?? null
      : activeFile;
    const nextActiveSequence = activeSequence === path
      ? isSequenceFile(nextActiveFile) ? nextActiveFile : null
      : activeSequence;

    set({
      openEditors: nextEditors,
      activeFile: nextActiveFile,
      activeSequence: nextActiveSequence,
      status: nextActiveFile ? `Editing ${nextActiveFile}` : "No file open"
    });

    if (isSequenceFile(nextActiveFile)) {
      try {
        await invoke("open_sequence", { path: nextActiveFile });
        await get().renderFrame();
      } catch (error) {
        set({ status: formatError(error) });
      }
    } else if (!nextActiveSequence) {
      set({ frame: null });
    }
  },
  setTime: (time) => set({ time }),
  setPlaying: (playing) => set({ playing, status: playing ? "Playback started" : "Playback paused" }),
  togglePlayback: () => {
    const playing = !get().playing;
    set({ playing, status: playing ? "Playback started" : "Playback paused" });
  },
  setStatus: (status) => set({ status }),
  setLanguageProblems: (languageProblems) => set({ languageProblems }),
  openProblem: async (problem) => {
    set({ pendingRevealProblem: problem });
    await get().openFile(problem.path);
  },
  clearPendingRevealProblem: () => set({ pendingRevealProblem: null }),
  setPanelVisible: (panelId, visible) =>
    set((state) => ({ panelVisibility: { ...state.panelVisibility, [panelId]: visible } })),
  setPanelVisibility: (visibility) =>
    set((state) => ({ panelVisibility: { ...state.panelVisibility, ...visibility } })),
  openProjectDialog: async () => {
    try {
      const selected = await open({
        directory: true,
        multiple: false,
        title: "Open Dawn Project"
      });
      if (typeof selected === "string") {
        await get().openProject(selected);
      }
    } catch (error) {
      set({ status: formatError(error) });
    }
  },
  openProject: async (path) => {
    if (!path.trim()) {
      set({ status: "Enter a project folder or project.dawn path." });
      return;
    }

    try {
      if (!(await flushAutosave(set, get))) return;
      set({ status: "Opening project..." });
      const projectState = await invoke<ProjectState>("open_project", { path });
      set({
        projectState,
        languageProblems: [],
        pendingRevealProblem: null,
        activeFile: null,
        openEditors: [],
        activeSequence: null,
        frame: null,
        status: `Opened ${projectState.root}`
      });
      await runProjectAnalysis(set, get);
      if (projectState.files[0]) {
        await get().openFile(projectState.files[0]);
      } else {
        set({ activeFile: null, openEditors: [], activeSequence: null });
      }
    } catch (error) {
      set({ status: formatError(error) });
    }
  },
  closeProject: async () => {
    if (!(await flushAutosave(set, get))) return;
    clearAnalysis();
    set({
      projectState: null,
      languageProblems: [],
      pendingRevealProblem: null,
      activeFile: null,
      openEditors: [],
      activeSequence: null,
      frame: null,
      playing: false,
      status: "Project closed"
    });
  },
  openFile: async (path) => {
    try {
      const existing = get().openEditors.find((editor) => editor.path === path);
      if (existing) {
        activateEditor(path, set, get);
        return;
      }

      set({ activeFile: path, status: `Opening ${path}` });
      const content = await invoke<string>("read_file", { path });
      let activeSequence = get().activeSequence;
      if (isSequenceFile(path)) {
        await invoke("open_sequence", { path });
        activeSequence = path;
      }
      set((state) => ({
        openEditors: [...state.openEditors, { path, content, dirty: false }],
        activeSequence,
        status: `Editing ${path}`
      }));
      if (isSequenceFile(path)) {
        await get().renderFrame();
      }
    } catch (error) {
      set({ status: formatError(error) });
    }
  },
  saveFile: async () => {
    const { activeFile } = get();
    if (!activeFile) return;
    clearAutosave(activeFile);
    await saveEditor(activeFile, set, get, "Saved");
  },
  reloadFile: async () => {
    const { activeFile } = get();
    if (!activeFile) return;

    try {
      const content = await invoke<string>("read_file", { path: activeFile });
      set((state) => ({
        openEditors: state.openEditors.map((editor) =>
          editor.path === activeFile ? { ...editor, content, dirty: false } : editor
        ),
        status: `Reloaded ${activeFile}`
      }));
      await get().renderFrame();
      await runProjectAnalysis(set, get);
    } catch (error) {
      set({ status: formatError(error) });
    }
  },
  runCheck: async () => {
    try {
      set({ status: "Checking project..." });
      await runProjectAnalysis(set, get);
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
      if (!(await flushAutosave(set, get))) return;
      const result = await invoke<FileOperationState>("rename_path", { path, newName });
      applyFileOperationResult(result, set, get);
      set({ status: `Renamed ${path} to ${newName}` });
      await runProjectAnalysis(set, get);
    } catch (error) {
      set({ status: formatError(error) });
    }
  },
  movePaths: async (paths, newParent) => {
    try {
      if (!(await flushAutosave(set, get))) return;
      const result = await invoke<FileOperationState>("move_paths", { paths, newParent });
      applyFileOperationResult(result, set, get);
      set({ status: result.moved.length ? `Moved ${result.moved.length} item${result.moved.length === 1 ? "" : "s"}` : "Move skipped" });
      await runProjectAnalysis(set, get);
    } catch (error) {
      set({ status: formatError(error) });
    }
  }
}));

function scheduleAutosave(
  path: string,
  set: WorkbenchSet,
  get: () => WorkbenchState
) {
  clearAutosave(path);
  const timer = setTimeout(() => {
    autosaveTimers.delete(path);
    void saveEditor(path, set, get, "Autosaved");
  }, 650);
  autosaveTimers.set(path, timer);
}

function updateEditorContent(
  path: string,
  content: string,
  set: WorkbenchSet,
  get: () => WorkbenchState
) {
  const editor = get().openEditors.find((item) => item.path === path);
  if (!editor || editor.content === content) return;

  set((state) => ({
    openEditors: state.openEditors.map((item) =>
      item.path === path ? { ...item, content, dirty: true } : item
    )
  }));
  scheduleAutosave(path, set, get);
  scheduleAnalysis(path, set, get);
}

function clearAutosave(path?: string) {
  if (path) {
    const timer = autosaveTimers.get(path);
    if (!timer) return;
    clearTimeout(timer);
    autosaveTimers.delete(path);
    return;
  }

  for (const timer of autosaveTimers.values()) {
    clearTimeout(timer);
  }
  autosaveTimers.clear();
}

function scheduleAnalysis(
  path: string,
  set: WorkbenchSet,
  get: () => WorkbenchState
) {
  clearAnalysis(path);
  const timer = setTimeout(() => {
    analysisTimers.delete(path);
    void runProjectAnalysis(set, get, { preserveStatus: true });
  }, 300);
  analysisTimers.set(path, timer);
}

function clearAnalysis(path?: string) {
  if (path) {
    const timer = analysisTimers.get(path);
    if (!timer) return;
    clearTimeout(timer);
    analysisTimers.delete(path);
    return;
  }

  for (const timer of analysisTimers.values()) {
    clearTimeout(timer);
  }
  analysisTimers.clear();
}

async function runProjectAnalysis(
  set: WorkbenchSet,
  get: () => WorkbenchState,
  options: { preserveStatus?: boolean } = {}
) {
  if (!get().projectState) return;

  clearAnalysis();
  const analysis = await invoke<AnalysisState>("analyze_project", {
    overlays: dirtyEditorOverlays(get)
  });
  const errors = analysis.diagnostics.filter((diagnostic) => diagnostic.severity === "Error").length;
  const warnings = analysis.diagnostics.filter((diagnostic) => diagnostic.severity === "Warning").length;
  const status = errors || warnings
    ? `Check complete: ${errors} errors, ${warnings} warnings`
    : `Check complete: ${analysis.reachableFileCount} files, ${analysis.objectCount} objects`;

  set((state) => ({
    languageProblems: analysis.diagnostics,
    projectState: state.projectState
      ? { ...state.projectState, diagnostics: analysis.diagnostics }
      : state.projectState,
    status: options.preserveStatus ? state.status : status
  }));
}

function dirtyEditorOverlays(get: () => WorkbenchState) {
  return get()
    .openEditors
    .filter((editor) => editor.dirty)
    .map((editor) => ({ path: editor.path, content: editor.content }));
}

async function saveEditor(
  path: string,
  set: WorkbenchSet,
  get: () => WorkbenchState,
  savedStatusPrefix: string
) {
  const editor = get().openEditors.find((item) => item.path === path);
  if (!editor?.dirty) return true;

  try {
    await invoke("write_file", { path, content: editor.content });
    set((state) => ({
      openEditors: state.openEditors.map((item) =>
        item.path === path ? { ...item, dirty: false } : item
      ),
      status: `${savedStatusPrefix} ${path}`
    }));
    await runProjectAnalysis(set, get, { preserveStatus: true });
    return true;
  } catch (error) {
    set({ status: formatError(error) });
    return false;
  }
}

async function flushAutosave(
  set: WorkbenchSet,
  get: () => WorkbenchState
) {
  clearAutosave();
  const dirtyEditors = get().openEditors.filter((editor) => editor.dirty);
  for (const editor of dirtyEditors) {
    if (!(await saveEditor(editor.path, set, get, "Saved"))) {
      return false;
    }
  }
  return true;
}

function applyFileOperationResult(
  result: FileOperationState,
  set: WorkbenchSet,
  get: () => WorkbenchState
) {
  const { activeFile, activeSequence } = get();

  set({
    projectState: result.project,
    activeFile: applyMovedPath(activeFile, result.moved),
    activeSequence: applyMovedPath(activeSequence, result.moved),
    openEditors: get().openEditors.map((editor) => ({
      ...editor,
      path: applyMovedPath(editor.path, result.moved) ?? editor.path
    }))
  });
}

function activateEditor(
  path: string,
  set: WorkbenchSet,
  get: () => WorkbenchState
) {
  const editor = get().openEditors.find((item) => item.path === path);
  if (!editor) return;

  let activeSequence = get().activeSequence;
  if (isSequenceFile(path)) {
    activeSequence = path;
    void invoke("open_sequence", { path })
      .then(() => get().renderFrame())
      .catch((error) => set({ status: formatError(error) }));
  }

  set({ activeFile: path, activeSequence, status: `Editing ${path}` });
}

function applyMovedPath(path: string | null, moved: FileOperationState["moved"]) {
  if (!path) return path;
  const move = moved.find((item) => path === item.oldPath || isDescendantPath(path, item.oldPath));
  if (!move) return path;
  return path === move.oldPath ? move.newPath : `${move.newPath}${path.slice(move.oldPath.length)}`;
}

function isDescendantPath(path: string, parent: string) {
  if (!path.startsWith(parent)) return false;
  const separator = path[parent.length];
  return separator === "\\" || separator === "/";
}

function formatError(error: unknown) {
  return error instanceof Error ? error.message : String(error);
}

function isSequenceFile(path: string | null | undefined) {
  return Boolean(path?.endsWith(".sequence.dawn"));
}
