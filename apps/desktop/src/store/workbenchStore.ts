import { open } from "@tauri-apps/plugin-dialog";
import { create } from "zustand";
import { commands } from "../generated/bindings";
import type {
  DocumentDescriptor,
  FileOperationState,
  FixtureDocument,
  FixtureDocumentEditResponse,
  FrameSummary,
  LanguageProblem,
  LayoutDocument,
  LayoutDocumentEditResponse,
  ProjectState
} from "../generated/bindings";
import type { PanelId } from "../workbench/panelIds";

type PanelVisibility = Record<PanelId, boolean>;
export type EditorViewMode = "text" | "layout" | "fixture";

export type LayoutViewerState = {
  path: string | null;
  status: "idle" | "loading" | "ready" | "error";
  descriptor: DocumentDescriptor | null;
  objectKey: string | null;
  document: LayoutDocument | null;
  selectedFixtureId: string | null;
  highlightedGroup: string | null;
  error: string | null;
};

export type FixtureViewerState = {
  path: string | null;
  status: "idle" | "loading" | "ready" | "error";
  descriptor: DocumentDescriptor | null;
  selectedObjectKey: string | null;
  document: FixtureDocument | null;
  error: string | null;
};

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
  editorViewsByPath: Record<string, EditorViewMode>;
  layoutView: LayoutViewerState;
  fixtureView: FixtureViewerState;
  activeSequence: string | null;
  time: number;
  playing: boolean;
  frame: FrameSummary | null;
  status: string;
  panelVisibility: PanelVisibility;
  setFileContent: (content: string) => void;
  setEditorContent: (path: string, content: string) => void;
  activateFile: (path: string) => void;
  inspectActiveDocument: () => Promise<void>;
  setEditorView: (path: string, view: EditorViewMode) => void;
  selectLayoutObject: (objectKey: string) => void;
  selectLayoutFixture: (fixtureId: string | null) => void;
  selectLayoutGroup: (groupName: string | null) => void;
  refreshLayoutView: (path?: string, objectKey?: string | null) => Promise<void>;
  selectFixtureObject: (objectKey: string | null) => void;
  refreshFixtureView: (path?: string, objectKey?: string | null) => Promise<void>;
  applyLayoutDocumentEdit: (document: LayoutDocument, allowBreakingReferences?: boolean) => Promise<LayoutDocumentEditResponse | null>;
  applyFixtureDocumentEdit: (document: FixtureDocument, allowBreakingReferences?: boolean) => Promise<FixtureDocumentEditResponse | null>;
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
  reloadProjectFromDisk: () => Promise<void>;
  runCheck: () => Promise<void>;
  renderFrame: () => Promise<void>;
  renamePath: (path: string, newName: string) => Promise<void>;
  movePaths: (paths: string[], newParent: string) => Promise<void>;
};

type WorkbenchSet = (partial: Partial<WorkbenchState> | ((state: WorkbenchState) => Partial<WorkbenchState>)) => void;

const autosaveTimers = new Map<string, ReturnType<typeof setTimeout>>();
const analysisTimers = new Map<string, ReturnType<typeof setTimeout>>();
const layoutRefreshTimers = new Map<string, ReturnType<typeof setTimeout>>();
const fixtureRefreshTimers = new Map<string, ReturnType<typeof setTimeout>>();
let layoutRequestSerial = 0;
let layoutEditSerial = 0;
let fixtureRequestSerial = 0;

const initialPanelVisibility: PanelVisibility = {
  project: true,
  editor: true,
  preview: true
};

export const useWorkbench = create<WorkbenchState>((set, get) => ({
  projectState: null,
  languageProblems: [],
  pendingRevealProblem: null,
  activeFile: null,
  openEditors: [],
  editorViewsByPath: {},
  layoutView: emptyLayoutView(),
  fixtureView: emptyFixtureView(),
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
  inspectActiveDocument: async () => {
    const { activeFile } = get();
    if (!activeFile) return;
    await inspectDocumentForPath(activeFile, set, get);
  },
  setEditorView: (path, view) => {
    set((state) => ({
      editorViewsByPath: { ...state.editorViewsByPath, [path]: view }
    }));
    if (view === "layout") {
      void loadLayoutView(path, null, set, get);
    } else if (view === "fixture") {
      void loadFixtureView(path, null, set, get);
    }
  },
  selectLayoutObject: (objectKey) => {
    const { activeFile } = get();
    if (!activeFile) return;
    void loadLayoutView(activeFile, objectKey, set, get);
  },
  selectLayoutFixture: (fixtureId) => {
    set((state) => ({ layoutView: { ...state.layoutView, selectedFixtureId: fixtureId } }));
  },
  selectLayoutGroup: (groupName) => {
    set((state) => ({ layoutView: { ...state.layoutView, highlightedGroup: groupName } }));
  },
  refreshLayoutView: async (path, objectKey) => {
    const targetPath = path ?? get().activeFile;
    if (!targetPath) return;
    await loadLayoutView(targetPath, objectKey ?? null, set, get);
  },
  selectFixtureObject: (objectKey) => {
    const { activeFile } = get();
    if (!activeFile) return;
    void loadFixtureView(activeFile, objectKey, set, get);
  },
  refreshFixtureView: async (path, objectKey) => {
    const targetPath = path ?? get().activeFile;
    if (!targetPath) return;
    await loadFixtureView(targetPath, objectKey ?? null, set, get);
  },
  applyLayoutDocumentEdit: (document, allowBreakingReferences = false) =>
    applyLayoutDocumentEdit(document, allowBreakingReferences, set, get),
  applyFixtureDocumentEdit: (document, allowBreakingReferences = false) =>
    applyFixtureDocumentEdit(document, allowBreakingReferences, set, get),
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
      layoutView: nextActiveFile ? get().layoutView : emptyLayoutView(),
      fixtureView: nextActiveFile ? get().fixtureView : emptyFixtureView(),
      status: nextActiveFile ? `Editing ${nextActiveFile}` : "No file open"
    });
    if (nextActiveFile) {
      void inspectDocumentForPath(nextActiveFile, set, get);
    }

    if (nextActiveFile && isSequenceFile(nextActiveFile)) {
      try {
        await unwrapCommand(commands.openSequence(nextActiveFile));
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
      clearLayoutRefresh();
      clearFixtureRefresh();
      set({ status: "Opening project..." });
      const projectState = await unwrapCommand(commands.openProject(path));
      set({
        projectState,
        languageProblems: [],
        pendingRevealProblem: null,
        activeFile: null,
        openEditors: [],
        editorViewsByPath: {},
        layoutView: emptyLayoutView(),
        fixtureView: emptyFixtureView(),
        activeSequence: null,
        frame: null,
        status: `Opened ${projectState.root}`
      });
      await runProjectAnalysis(set, get);
      if (projectState.files[0]) {
        await get().openFile(projectState.files[0]);
      } else {
        set({ activeFile: null, openEditors: [], activeSequence: null, layoutView: emptyLayoutView(), fixtureView: emptyFixtureView() });
      }
    } catch (error) {
      set({ status: formatError(error) });
    }
  },
  closeProject: async () => {
    if (!(await flushAutosave(set, get))) return;
    clearAnalysis();
    clearLayoutRefresh();
    clearFixtureRefresh();
    set({
      projectState: null,
      languageProblems: [],
      pendingRevealProblem: null,
      activeFile: null,
      openEditors: [],
      editorViewsByPath: {},
      layoutView: emptyLayoutView(),
      fixtureView: emptyFixtureView(),
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
      const content = await unwrapCommand(commands.readFile(path));
      let activeSequence = get().activeSequence;
      if (isSequenceFile(path)) {
        await unwrapCommand(commands.openSequence(path));
        activeSequence = path;
      }
      set((state) => ({
        openEditors: [...state.openEditors, { path, content, dirty: false }],
        editorViewsByPath: { ...state.editorViewsByPath, [path]: state.editorViewsByPath[path] ?? "text" },
        activeSequence,
        status: `Editing ${path}`
      }));
      void inspectDocumentForPath(path, set, get);
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
  reloadProjectFromDisk: async () => {
    const { projectState } = get();
    if (!projectState) return;

    try {
      clearAutosave();
      clearLayoutRefresh();
      clearFixtureRefresh();
      set({ status: "Reloading project..." });
      const nextProjectState = await unwrapCommand(commands.openProject(projectState.root));
      set({
        projectState: nextProjectState,
        languageProblems: [],
        pendingRevealProblem: null,
        activeFile: null,
        openEditors: [],
        editorViewsByPath: {},
        layoutView: emptyLayoutView(),
        fixtureView: emptyFixtureView(),
        activeSequence: null,
        frame: null,
        status: `Reloaded ${nextProjectState.root}`
      });
      await runProjectAnalysis(set, get);
      if (nextProjectState.files[0]) {
        await get().openFile(nextProjectState.files[0]);
      }
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
      set({ frame: await unwrapCommand(commands.renderFrame(time)) });
    } catch {
      set({ frame: null });
    }
  },
  renamePath: async (path, newName) => {
    try {
      if (!(await flushAutosave(set, get))) return;
      const result = await unwrapCommand(commands.renamePath(path, newName));
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
      const result = await unwrapCommand(commands.movePaths(paths, newParent));
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
  get: () => WorkbenchState,
  options: { refreshVisualViews?: boolean } = {}
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
  if (options.refreshVisualViews ?? true) {
    scheduleLayoutRefresh(path, set, get);
    scheduleFixtureRefresh(path, set, get);
  }
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

function scheduleLayoutRefresh(
  path: string,
  set: WorkbenchSet,
  get: () => WorkbenchState
) {
  if (get().activeFile !== path || get().editorViewsByPath[path] !== "layout") return;
  clearLayoutRefresh(path);
  const timer = setTimeout(() => {
    layoutRefreshTimers.delete(path);
    void loadLayoutView(path, get().layoutView.objectKey, set, get);
  }, 350);
  layoutRefreshTimers.set(path, timer);
}

function scheduleFixtureRefresh(
  path: string,
  set: WorkbenchSet,
  get: () => WorkbenchState
) {
  if (get().activeFile !== path || get().editorViewsByPath[path] !== "fixture") return;
  clearFixtureRefresh(path);
  const timer = setTimeout(() => {
    fixtureRefreshTimers.delete(path);
    void loadFixtureView(path, get().fixtureView.selectedObjectKey, set, get);
  }, 350);
  fixtureRefreshTimers.set(path, timer);
}

function clearLayoutRefresh(path?: string) {
  if (path) {
    const timer = layoutRefreshTimers.get(path);
    if (!timer) return;
    clearTimeout(timer);
    layoutRefreshTimers.delete(path);
    return;
  }

  for (const timer of layoutRefreshTimers.values()) {
    clearTimeout(timer);
  }
  layoutRefreshTimers.clear();
}

function clearFixtureRefresh(path?: string) {
  if (path) {
    const timer = fixtureRefreshTimers.get(path);
    if (!timer) return;
    clearTimeout(timer);
    fixtureRefreshTimers.delete(path);
    return;
  }

  for (const timer of fixtureRefreshTimers.values()) {
    clearTimeout(timer);
  }
  fixtureRefreshTimers.clear();
}

async function inspectDocumentForPath(
  path: string,
  set: WorkbenchSet,
  get: () => WorkbenchState
) {
  try {
    const descriptor = await unwrapCommand(commands.inspectDocument(path, dirtyEditorOverlays(get)));
    if (get().activeFile !== path) return;
    set((state) => ({
      layoutView: {
        ...state.layoutView,
        path,
        descriptor,
        error: null,
        status: state.layoutView.path === path ? state.layoutView.status : "idle"
      },
      fixtureView: {
        ...state.fixtureView,
        path,
        descriptor,
        error: null,
        status: state.fixtureView.path === path ? state.fixtureView.status : "idle"
      }
    }));
    if (get().editorViewsByPath[path] === "layout") {
      await loadLayoutView(path, get().layoutView.objectKey, set, get);
    } else if (get().editorViewsByPath[path] === "fixture") {
      await loadFixtureView(path, get().fixtureView.selectedObjectKey, set, get);
    }
  } catch (error) {
    if (get().activeFile !== path) return;
    set((state) => ({
      layoutView: {
        ...state.layoutView,
        path,
        descriptor: null,
        document: null,
        status: "error",
        error: formatError(error)
      },
      fixtureView: {
        ...state.fixtureView,
        path,
        descriptor: null,
        document: null,
        status: "error",
        error: formatError(error)
      }
    }));
  }
}

async function loadLayoutView(
  path: string,
  requestedObjectKey: string | null,
  set: WorkbenchSet,
  get: () => WorkbenchState
) {
  const requestId = ++layoutRequestSerial;
  clearLayoutRefresh(path);

  set((state) => ({
    layoutView: {
      ...state.layoutView,
      path,
      status: "loading",
      error: null,
      document: state.layoutView.path === path ? state.layoutView.document : null
    }
  }));

  try {
    const descriptor = await unwrapCommand(commands.inspectDocument(path, dirtyEditorOverlays(get)));
    if (requestId !== layoutRequestSerial || get().activeFile !== path) return;

    if (!descriptor.availableViews.includes("layout")) {
      set({
        layoutView: {
          path,
          status: "idle",
          descriptor,
          objectKey: null,
          document: null,
          selectedFixtureId: null,
          highlightedGroup: null,
          error: null
        }
      });
      return;
    }

    const layoutObjectKeys = descriptor.objects
      .filter((object) => object.kind === "layout")
      .map((object) => object.key);
    const objectKey = requestedObjectKey && layoutObjectKeys.includes(requestedObjectKey)
      ? requestedObjectKey
      : descriptor.defaultObjectKeys.layout ?? layoutObjectKeys[0] ?? null;
    if (!objectKey) {
      throw new Error("document has no layout object");
    }

    const document = await unwrapCommand(commands.getLayoutDocument(path, objectKey, dirtyEditorOverlays(get)));
    if (requestId !== layoutRequestSerial || get().activeFile !== path) return;

    set((state) => {
      const selectedFixtureId = state.layoutView.selectedFixtureId
        && document.fixtures.some((fixture) => fixture.id === state.layoutView.selectedFixtureId)
        ? state.layoutView.selectedFixtureId
        : null;
      return {
        layoutView: {
          path,
          status: "ready",
          descriptor,
          objectKey,
          document,
          selectedFixtureId,
          highlightedGroup: state.layoutView.highlightedGroup,
          error: null
        }
      };
    });
  } catch (error) {
    if (requestId !== layoutRequestSerial || get().activeFile !== path) return;
    set((state) => ({
      layoutView: {
        ...state.layoutView,
        path,
        status: "error",
        document: null,
        error: formatError(error)
      }
    }));
  }
}

async function loadFixtureView(
  path: string,
  requestedObjectKey: string | null,
  set: WorkbenchSet,
  get: () => WorkbenchState
) {
  const requestId = ++fixtureRequestSerial;
  clearFixtureRefresh(path);

  set((state) => ({
    fixtureView: {
      ...state.fixtureView,
      path,
      status: "loading",
      error: null,
      document: state.fixtureView.path === path ? state.fixtureView.document : null
    }
  }));

  try {
    const descriptor = await unwrapCommand(commands.inspectDocument(path, dirtyEditorOverlays(get)));
    if (requestId !== fixtureRequestSerial || get().activeFile !== path) return;

    if (!descriptor.availableViews.includes("fixture")) {
      set({
        fixtureView: {
          path,
          status: "idle",
          descriptor,
          selectedObjectKey: null,
          document: null,
          error: null
        }
      });
      return;
    }

    const fixtureObjectKeys = descriptor.objects
      .filter((object) => object.kind === "fixture")
      .map((object) => object.key);
    const objectKey = requestedObjectKey && fixtureObjectKeys.includes(requestedObjectKey)
      ? requestedObjectKey
      : descriptor.defaultObjectKeys.fixture ?? fixtureObjectKeys[0] ?? null;
    const document = await unwrapCommand(commands.getFixtureDocument(path, objectKey, dirtyEditorOverlays(get)));
    if (requestId !== fixtureRequestSerial || get().activeFile !== path) return;

    set({
      fixtureView: {
        path,
        status: "ready",
        descriptor,
        selectedObjectKey: document.selectedObjectKey,
        document,
        error: null
      }
    });
  } catch (error) {
    if (requestId !== fixtureRequestSerial || get().activeFile !== path) return;
    set((state) => ({
      fixtureView: {
        ...state.fixtureView,
        path,
        status: "error",
        document: null,
        error: formatError(error)
      }
    }));
  }
}

async function applyLayoutDocumentEdit(
  document: LayoutDocument,
  allowBreakingReferences: boolean,
  set: WorkbenchSet,
  get: () => WorkbenchState
) {
  const editor = get().openEditors.find((item) => item.path === document.path);
  if (!editor) return null;

  const editId = ++layoutEditSerial;
  layoutRequestSerial++;
  clearLayoutRefresh(document.path);

  const previousLayoutView = get().layoutView;
  set((state) => ({
    layoutView: {
      ...state.layoutView,
      path: document.path,
      objectKey: document.objectKey,
      document,
      selectedFixtureId: state.layoutView.selectedFixtureId
        && document.fixtures.some((fixture) => fixture.id === state.layoutView.selectedFixtureId)
        ? state.layoutView.selectedFixtureId
        : null,
      status: "ready",
      error: null
    }
  }));

  try {
    const result = await unwrapCommand(commands.applyLayoutDocumentEdit(
      document.path,
      document.objectKey,
      document,
      editor.content,
      dirtyEditorOverlays(get),
      allowBreakingReferences
    ));
    if (editId !== layoutEditSerial) return null;

    if (result.type === "applied") {
      updateEditorContent(document.path, result.serializedContent, set, get, { refreshVisualViews: false });
      set((state) => ({
        languageProblems: result.analysis.diagnostics,
        layoutView: {
          ...state.layoutView,
          document: result.refreshedDocument,
          selectedFixtureId: state.layoutView.selectedFixtureId
            && result.refreshedDocument.fixtures.some((fixture) => fixture.id === state.layoutView.selectedFixtureId)
            ? state.layoutView.selectedFixtureId
            : null,
          status: "ready",
          error: null
        },
        status: "Layout edit applied"
      }));
    } else {
      set({
        languageProblems: result.diagnostics,
        layoutView: previousLayoutView,
        status: result.message
      });
    }
    return result;
  } catch (error) {
    if (editId === layoutEditSerial) {
      set({ layoutView: previousLayoutView, status: formatError(error) });
    }
    return null;
  }
}

async function applyFixtureDocumentEdit(
  document: FixtureDocument,
  allowBreakingReferences: boolean,
  set: WorkbenchSet,
  get: () => WorkbenchState
) {
  const editor = get().openEditors.find((item) => item.path === document.path);
  if (!editor) return null;
  try {
    const result = await unwrapCommand(commands.applyFixtureDocumentEdit(
      document.path,
      document,
      editor.content,
      dirtyEditorOverlays(get),
      allowBreakingReferences
    ));
    if (result.type === "applied") {
      updateEditorContent(document.path, result.serializedContent, set, get, { refreshVisualViews: false });
      set({
        languageProblems: result.analysis.diagnostics,
        fixtureView: {
          path: document.path,
          status: "ready",
          descriptor: get().fixtureView.descriptor,
          selectedObjectKey: result.refreshedDocument.selectedObjectKey,
          document: result.refreshedDocument,
          error: null
        },
        status: "Fixture edit applied"
      });
    } else {
      set({ languageProblems: result.diagnostics, status: result.message });
    }
    return result;
  } catch (error) {
    set({ status: formatError(error) });
    return null;
  }
}

async function runProjectAnalysis(
  set: WorkbenchSet,
  get: () => WorkbenchState,
  options: { preserveStatus?: boolean } = {}
) {
  if (!get().projectState) return;

  clearAnalysis();
  const analysis = await unwrapCommand(commands.analyzeProject(dirtyEditorOverlays(get)));
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
    await unwrapCommand(commands.writeFile(path, editor.content));
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
    editorViewsByPath: moveEditorViewPaths(get().editorViewsByPath, result.moved),
    layoutView: emptyLayoutView(),
    fixtureView: emptyFixtureView(),
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
    void unwrapCommand(commands.openSequence(path))
      .then(() => get().renderFrame())
      .catch((error) => set({ status: formatError(error) }));
  }

  set({ activeFile: path, activeSequence, status: `Editing ${path}` });
  void inspectDocumentForPath(path, set, get);
}

function moveEditorViewPaths(
  views: Record<string, EditorViewMode>,
  moved: FileOperationState["moved"]
) {
  const next: Record<string, EditorViewMode> = {};
  for (const [path, view] of Object.entries(views)) {
    next[applyMovedPath(path, moved) ?? path] = view;
  }
  return next;
}

function emptyLayoutView(): LayoutViewerState {
  return {
    path: null,
    status: "idle",
    descriptor: null,
    objectKey: null,
    document: null,
    selectedFixtureId: null,
    highlightedGroup: null,
    error: null
  };
}

function emptyFixtureView(): FixtureViewerState {
  return {
    path: null,
    status: "idle",
    descriptor: null,
    selectedObjectKey: null,
    document: null,
    error: null
  };
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

type CommandResult<T, E> = { status: "ok"; data: T } | { status: "error"; error: E };

async function unwrapCommand<T>(command: Promise<CommandResult<T, string>>): Promise<T> {
  const result = await command;
  if (result.status === "error") {
    throw new Error(result.error);
  }
  return result.data;
}
