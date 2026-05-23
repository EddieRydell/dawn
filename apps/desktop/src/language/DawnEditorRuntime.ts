import * as monaco from "monaco-editor";
import type { OpenEditor } from "../store/workbenchStore";
import type { LanguageProblem } from "../types";
import { DawnLspClient, ensureDawnLanguageRegistered, isDawnFile } from "./dawnLanguageClient";

type DawnEditorProject = {
  root: string;
  languageServiceUrl: string;
};

type RuntimeCallbacks = {
  onContentChanged: (path: string, content: string) => void;
  onProblemsChanged: (problems: LanguageProblem[]) => void;
  onError: (message: string) => void;
  saveFile: () => Promise<void>;
  activateNextEditor: (direction: 1 | -1) => void;
};

export class DawnEditorRuntime {
  private editor: monaco.editor.IStandaloneCodeEditor | undefined;
  private container: HTMLElement | undefined;
  private project: DawnEditorProject | null = null;
  private lspClient: DawnLspClient | undefined;
  private models = new Map<string, monaco.editor.ITextModel>();
  private modelDisposables = new Map<string, monaco.IDisposable>();
  private syncingModels = new Set<string>();
  private openEditors: OpenEditor[] = [];
  private activeFile: string | null = null;
  private started = false;
  private disposed = false;

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
    void this.startLanguageClient();
  }

  async setProject(project: DawnEditorProject | null): Promise<void> {
    const sameProject =
      this.project?.root === project?.root &&
      this.project?.languageServiceUrl === project?.languageServiceUrl;
    if (sameProject) return;

    this.project = project;
    await this.disposeLanguageClient();

    if (!project) {
      this.callbacks.onProblemsChanged([]);
      this.editor?.setModel(null);
      this.disposeModels();
      return;
    }

    await this.startLanguageClient();
  }

  syncOpenFiles(openEditors: OpenEditor[]): void {
    this.openEditors = openEditors;
    if (!this.editor) return;

    const openPaths = new Set(openEditors.map((editor) => editor.path));
    for (const editor of openEditors) {
      const model = this.ensureModel(editor);
      if (model.getValue() !== editor.content && !editor.dirty) {
        this.syncingModels.add(editor.path);
        model.setValue(editor.content);
        this.syncingModels.delete(editor.path);
      }
    }

    for (const [path, model] of this.models) {
      if (openPaths.has(path)) continue;
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

  revealProblem(problem: LanguageProblem): void {
    const editor = this.editor;
    if (!editor || problem.path !== this.activeFile) return;

    const range = new monaco.Range(problem.line, problem.column, problem.endLine, problem.endColumn);
    editor.setSelection(range);
    editor.revealRangeInCenter(range);
    editor.focus();
  }

  async dispose(): Promise<void> {
    this.disposed = true;
    await this.disposeLanguageClient();
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
    const languageId = isDawnFile(openEditor.path) ? "dawn" : "plaintext";
    const existing = this.models.get(openEditor.path);
    if (existing) {
      monaco.editor.setModelLanguage(existing, languageId);
      if (languageId === "dawn") {
        this.lspClient?.openModel(existing);
      }
      return existing;
    }

    const uri = monaco.Uri.file(openEditor.path);
    const model = monaco.editor.getModel(uri) ?? monaco.editor.createModel(openEditor.content, languageId, uri);
    monaco.editor.setModelLanguage(model, languageId);
    this.models.set(openEditor.path, model);
    this.modelDisposables.set(
      openEditor.path,
      model.onDidChangeContent(() => {
        if (this.syncingModels.has(openEditor.path)) return;
        this.callbacks.onContentChanged(openEditor.path, model.getValue());
        this.lspClient?.scheduleChange(model);
      })
    );
    if (languageId === "dawn") {
      this.lspClient?.openModel(model);
    }
    return model;
  }

  private async startLanguageClient() {
    if (!this.project || !this.editor || this.lspClient || this.disposed) return;

    const project = this.project;
    const client = new DawnLspClient({
      url: project.languageServiceUrl,
      projectRoot: project.root,
      onDiagnostics: this.callbacks.onProblemsChanged,
      onError: this.callbacks.onError
    });

    try {
      await client.start();
      const stillCurrent =
        this.project?.root === project.root &&
        this.project?.languageServiceUrl === project.languageServiceUrl;
      if (!stillCurrent || this.disposed || this.lspClient) {
        await client.dispose();
        return;
      }
      this.lspClient = client;
      for (const model of this.models.values()) {
        client.openModel(model);
      }
    } catch (error) {
      await client.dispose();
      this.callbacks.onError(error instanceof Error ? error.message : String(error));
    }
  }

  private async disposeLanguageClient() {
    const client = this.lspClient;
    if (!client) return;
    this.lspClient = undefined;
    await client.dispose().catch((error) => {
      this.callbacks.onError(error instanceof Error ? error.message : String(error));
    });
  }

  private disposeModel(path: string, model: monaco.editor.ITextModel) {
    this.lspClient?.closeModel(model);
    this.modelDisposables.get(path)?.dispose();
    this.modelDisposables.delete(path);
    model.dispose();
    this.models.delete(path);
  }

  private disposeModels() {
    for (const [path, model] of [...this.models]) {
      this.disposeModel(path, model);
    }
    this.modelDisposables.clear();
    this.models.clear();
  }
}
