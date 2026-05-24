import * as monaco from "monaco-editor";
import type { LanguageProblem } from "../generated/bindings";
import type { OpenEditor } from "../store/workbenchStore";
import { dawnLanguageIdForPath, ensureDawnLanguageRegistered } from "./dawnLanguage";

type DawnEditorProject = {
  root: string;
};

type RuntimeCallbacks = {
  onContentChanged: (path: string, content: string) => void;
  onProblemsChanged: (problems: LanguageProblem[]) => void;
  saveFile: () => Promise<void>;
  activateNextEditor: (direction: 1 | -1) => void;
};

export class DawnEditorRuntime {
  private editor: monaco.editor.IStandaloneCodeEditor | undefined;
  private container: HTMLElement | undefined;
  private project: DawnEditorProject | null = null;
  private models = new Map<string, monaco.editor.ITextModel>();
  private modelPathKeys = new Map<string, string>();
  private modelDisposables = new Map<string, monaco.IDisposable>();
  private syncingModels = new Set<string>();
  private openEditors: OpenEditor[] = [];
  private problems: LanguageProblem[] = [];
  private activeFile: string | null = null;
  private started = false;

  constructor(private readonly callbacks: RuntimeCallbacks) {}

  start(container: HTMLElement): void {
    if (this.started) return;
    this.started = true;
    this.container = container;

    ensureDawnLanguageRegistered();

    this.editor = monaco.editor.create(container, {
      automaticLayout: true,
      minimap: { enabled: false },
      fontSize: 13,
      scrollBeyondLastLine: false,
      tabSize: 2,
      theme: "dawn-dark"
    });

    this.editor.addCommand(monaco.KeyMod.CtrlCmd | monaco.KeyCode.KeyS, () => {
      void this.callbacks.saveFile();
    });
    this.editor.addCommand(monaco.KeyMod.CtrlCmd | monaco.KeyCode.Tab, () => {
      this.callbacks.activateNextEditor(1);
    });
    this.editor.addCommand(monaco.KeyMod.CtrlCmd | monaco.KeyMod.Shift | monaco.KeyCode.Tab, () => {
      this.callbacks.activateNextEditor(-1);
    });

    this.syncOpenFiles(this.openEditors);
    this.setActiveFile(this.activeFile);
  }

  async setProject(project: DawnEditorProject | null): Promise<void> {
    const sameProject = this.project?.root === project?.root;
    if (sameProject) return;

    this.project = project;

    if (!project) {
      this.callbacks.onProblemsChanged([]);
      this.editor?.setModel(null);
      this.setProblems([]);
      this.disposeModels();
    }
  }

  syncOpenFiles(openEditors: OpenEditor[]): void {
    this.openEditors = openEditors;
    if (!this.editor) return;

    const openPaths = new Set(openEditors.map((editor) => editor.path));
    const openPathKeys = new Set(openEditors.map((editor) => pathKey(editor.path)));
    for (const editor of openEditors) {
      const model = this.ensureModel(editor);
      if (model.getValue() !== editor.content && !editor.dirty) {
        this.syncingModels.add(editor.path);
        model.setValue(editor.content);
        this.syncingModels.delete(editor.path);
      }
    }

    for (const [path, model] of this.models) {
      if (openPaths.has(path) || openPathKeys.has(pathKey(path))) continue;
      this.disposeModel(path, model);
    }

    if (this.activeFile && !openPaths.has(this.activeFile)) {
      this.editor.setModel(null);
    } else if (this.activeFile) {
      this.attachActiveModel(false);
    }
  }

  setActiveFile(path: string | null): void {
    this.activeFile = path;
    if (!this.editor) return;

    if (!path) {
      this.editor.setModel(null);
      return;
    }

    this.attachActiveModel(true);
  }

  setProblems(problems: LanguageProblem[]): void {
    this.problems = problems;
    this.syncProblemMarkers();
  }

  revealProblem(problem: LanguageProblem): void {
    const editor = this.editor;
    if (!editor || problem.path !== this.activeFile) return;

    const range = new monaco.Range(problem.line, problem.column, problem.endLine, problem.endColumn);
    editor.setSelection(range);
    editor.revealRangeInCenter(range);
    editor.focus();
  }

  layout(): void {
    this.editor?.layout();
  }

  async dispose(): Promise<void> {
    this.setProblems([]);
    this.editor?.dispose();
    this.editor = undefined;
    this.disposeModels();
    this.container = undefined;
  }

  private attachActiveModel(focus: boolean): void {
    if (!this.editor || !this.activeFile) return;

    const openEditor = this.openEditors.find((editor) => editor.path === this.activeFile);
    if (!openEditor) {
      this.editor.setModel(null);
      return;
    }

    const model = this.ensureModel(openEditor);
    if (this.editor.getModel() !== model) {
      this.editor.setModel(model);
    }
    if (focus) {
      this.editor.focus();
    }
  }

  private ensureModel(openEditor: OpenEditor) {
    const languageId = dawnLanguageIdForPath(openEditor.path);
    const existing = this.models.get(openEditor.path);
    if (existing) {
      monaco.editor.setModelLanguage(existing, languageId);
      return existing;
    }

    const uri = monaco.Uri.file(openEditor.path);
    const model = monaco.editor.getModel(uri) ?? monaco.editor.createModel(openEditor.content, languageId, uri);
    monaco.editor.setModelLanguage(model, languageId);
    this.models.set(openEditor.path, model);
    this.modelPathKeys.set(openEditor.path, pathKey(openEditor.path));
    this.modelDisposables.set(
      openEditor.path,
      model.onDidChangeContent(() => {
        if (this.syncingModels.has(openEditor.path)) return;
        this.callbacks.onContentChanged(openEditor.path, model.getValue());
      })
    );
    this.applyProblemMarkers(openEditor.path, model);
    return model;
  }

  private disposeModel(path: string, model: monaco.editor.ITextModel) {
    monaco.editor.setModelMarkers(model, "dawn-analysis", []);
    this.modelDisposables.get(path)?.dispose();
    this.modelDisposables.delete(path);
    model.dispose();
    this.models.delete(path);
    this.modelPathKeys.delete(path);
  }

  private disposeModels() {
    for (const [path, model] of [...this.models]) {
      this.disposeModel(path, model);
    }
    this.modelDisposables.clear();
    this.models.clear();
  }

  private syncProblemMarkers(): void {
    for (const [path, model] of this.models) {
      this.applyProblemMarkers(path, model);
    }
  }

  private applyProblemMarkers(path: string, model: monaco.editor.ITextModel): void {
    const modelPathKey = this.modelPathKeys.get(path) ?? pathKey(path);
    const markers = this.problems
      .filter((problem) => pathKey(problem.path) === modelPathKey)
      .map((problem) => markerFromProblem(problem, model));
    monaco.editor.setModelMarkers(model, "dawn-analysis", markers);
  }
}

function pathKey(path: string): string {
  return path.replace(/^\\\\\?\\/, "").replace(/\\/g, "/").toLowerCase();
}

function markerFromProblem(
  problem: LanguageProblem,
  model: monaco.editor.ITextModel
): monaco.editor.IMarkerData {
  const lineCount = model.getLineCount();
  const startLineNumber = clamp(problem.line, 1, lineCount);
  const endLineNumber = clamp(problem.endLine, startLineNumber, lineCount);
  const startLineMaxColumn = model.getLineMaxColumn(startLineNumber);
  const endLineMaxColumn = model.getLineMaxColumn(endLineNumber);
  const startColumn = clamp(problem.column, 1, startLineMaxColumn);
  const endColumn = clamp(
    Math.max(problem.endColumn, startLineNumber === endLineNumber ? startColumn + 1 : 1),
    1,
    Math.max(endLineMaxColumn, 2)
  );

  return {
    severity: markerSeverity(problem.severity),
    message: problem.message,
    source: problem.source ?? "dawn-analysis",
    code: problem.code,
    startLineNumber,
    startColumn,
    endLineNumber,
    endColumn
  };
}

function clamp(value: number, min: number, max: number): number {
  return Math.min(Math.max(value, min), max);
}

function markerSeverity(severity: LanguageProblem["severity"]): monaco.MarkerSeverity {
  switch (severity) {
    case "Error":
      return monaco.MarkerSeverity.Error;
    case "Warning":
      return monaco.MarkerSeverity.Warning;
  }
}
