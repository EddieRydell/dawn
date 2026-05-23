import * as monaco from "monaco-editor";
import type { LanguageProblem } from "../types";
import { dawnMonarch } from "./generated/dawnMonarch";

const DAWN_LANGUAGE_ID = "dawn";
const DAWN_YAML_LANGUAGE_ID = "dawn-yaml";
const DAWN_JSONL_LANGUAGE_ID = "dawn-jsonl";
const DAWN_LANGUAGE_IDS = [DAWN_LANGUAGE_ID, DAWN_YAML_LANGUAGE_ID, DAWN_JSONL_LANGUAGE_ID];
const MARKER_OWNER = "dawn-lsp";
const CHANGE_DEBOUNCE_MS = 120;
const TOKEN_LEGEND = [
  "keyword",
  "comment",
  "string",
  "number",
  "operator",
  "type",
  "class",
  "function",
  "method",
  "parameter",
  "variable"
];

let dawnLanguageRegistered = false;

type JsonRpcMessage = {
  jsonrpc: "2.0";
  id?: number;
  method?: string;
  params?: unknown;
  result?: unknown;
  error?: { code: number; message: string; data?: unknown };
};

type PendingRequest = {
  resolve: (value: unknown) => void;
  reject: (error: Error) => void;
};

type LspPosition = {
  line: number;
  character: number;
};

type LspRange = {
  start: LspPosition;
  end: LspPosition;
};

type LspLocation = {
  uri: string;
  range: LspRange;
};

type LspDiagnostic = {
  range: LspRange;
  severity?: number;
  code?: string | number;
  source?: string;
  message: string;
};

type LspCompletionItem = {
  label: string;
  kind?: number;
  detail?: string;
  documentation?: unknown;
  insertText?: string;
};

type LspDocumentSymbol = {
  name: string;
  detail?: string;
  kind: number;
  range: LspRange;
  selectionRange: LspRange;
  children?: LspDocumentSymbol[];
};

type LspTextDocumentIdentifier = {
  uri: string;
};

type DawnLspClientOptions = {
  url: string;
  projectRoot: string;
  onDiagnostics: (problems: LanguageProblem[]) => void;
  onError: (message: string) => void;
};

export function ensureDawnLanguageRegistered() {
  for (const id of DAWN_LANGUAGE_IDS) {
    if (!monaco.languages.getLanguages().some((language) => language.id === id)) {
      monaco.languages.register({
        id,
        extensions: id === DAWN_LANGUAGE_ID ? [".dawn"] : [],
        aliases: ["Dawn", "dawn"]
      });
    }
  }
  if (!dawnLanguageRegistered) {
    monaco.languages.setMonarchTokensProvider(DAWN_LANGUAGE_ID, dawnMonarch);
    monaco.languages.setMonarchTokensProvider(DAWN_YAML_LANGUAGE_ID, dawnYamlMonarch);
    monaco.languages.setMonarchTokensProvider(DAWN_JSONL_LANGUAGE_ID, dawnJsonlMonarch);
    monaco.editor.defineTheme("dawn-dark", {
      base: "vs-dark",
      inherit: true,
      rules: [
        { token: "identifier.predefined", foreground: "7dd3fc" },
        { token: "type", foreground: "c4b5fd" },
        { token: "number.hex", foreground: "f0abfc" }
      ],
      colors: {
        "editor.background": "#101317"
      }
    });
    dawnLanguageRegistered = true;
  }
}

export function isDawnFile(path: string) {
  return path.endsWith(".dawn");
}

export function dawnLanguageIdForPath(path: string) {
  if (path.endsWith(".effect.dawn")) return DAWN_LANGUAGE_ID;
  if (path.endsWith(".events.dawn")) return DAWN_JSONL_LANGUAGE_ID;
  if (path.endsWith(".dawn")) return DAWN_YAML_LANGUAGE_ID;
  return "plaintext";
}

export class DawnLspClient {
  private socket: WebSocket | undefined;
  private nextId = 1;
  private pending = new Map<number, PendingRequest>();
  private providerDisposables: monaco.IDisposable[] = [];
  private openedDocuments = new Map<string, { version: number; model: monaco.editor.ITextModel }>();
  private changeTimers = new Map<string, ReturnType<typeof setTimeout>>();
  private diagnostics = new Map<string, LanguageProblem[]>();
  private initializePromise: Promise<void> | undefined;
  private disposed = false;

  constructor(private readonly options: DawnLspClientOptions) {}

  async start(): Promise<void> {
    if (this.initializePromise) return this.initializePromise;

    this.initializePromise = new Promise<void>((resolve, reject) => {
      const socket = new WebSocket(this.options.url);
      this.socket = socket;

      socket.addEventListener("open", () => {
        void this.initialize()
          .then(() => {
            if (this.disposed) return;
            this.registerProviders();
            resolve();
          })
          .catch((error) => {
            reject(asError(error));
          });
      });
      socket.addEventListener("message", (event) => this.handleMessage(event.data));
      socket.addEventListener("error", () => {
        const error = new Error("Dawn language service WebSocket failed");
        this.options.onError(error.message);
        reject(error);
      });
      socket.addEventListener("close", () => {
        if (!this.disposed) {
          this.options.onError("Dawn language service disconnected");
        }
        this.rejectPending(new Error("Dawn language service disconnected"));
      });
    });

    return this.initializePromise;
  }

  openModel(model: monaco.editor.ITextModel): void {
    if (!isDawnModel(model) || this.openedDocuments.has(model.uri.toString())) return;

    const uri = model.uri.toString();
    this.openedDocuments.set(uri, { version: 1, model });
    this.notify("textDocument/didOpen", {
      textDocument: {
        uri,
        languageId: DAWN_LANGUAGE_ID,
        version: 1,
        text: model.getValue()
      }
    });
  }

  scheduleChange(model: monaco.editor.ITextModel): void {
    const uri = model.uri.toString();
    const document = this.openedDocuments.get(uri);
    if (!document) return;

    document.version += 1;
    const version = document.version;
    const existing = this.changeTimers.get(uri);
    if (existing) clearTimeout(existing);

    this.changeTimers.set(
      uri,
      setTimeout(() => {
        this.changeTimers.delete(uri);
        this.notify("textDocument/didChange", {
          textDocument: { uri, version },
          contentChanges: [{ text: model.getValue() }]
        });
      }, CHANGE_DEBOUNCE_MS)
    );
  }

  closeModel(model: monaco.editor.ITextModel): void {
    const uri = model.uri.toString();
    const timer = this.changeTimers.get(uri);
    if (timer) clearTimeout(timer);
    this.changeTimers.delete(uri);
    if (this.openedDocuments.delete(uri)) {
      this.notify("textDocument/didClose", { textDocument: { uri } });
    }
    this.clearMarkers(uri);
  }

  clearAllMarkers(): void {
    for (const uri of this.diagnostics.keys()) {
      this.clearMarkers(uri);
    }
    for (const { model } of this.openedDocuments.values()) {
      monaco.editor.setModelMarkers(model, MARKER_OWNER, []);
    }
    this.diagnostics.clear();
    this.options.onDiagnostics([]);
  }

  async dispose(): Promise<void> {
    this.disposed = true;
    for (const timer of this.changeTimers.values()) {
      clearTimeout(timer);
    }
    this.changeTimers.clear();
    for (const disposable of this.providerDisposables) {
      disposable.dispose();
    }
    this.providerDisposables = [];
    this.clearAllMarkers();

    if (this.socket?.readyState === WebSocket.OPEN) {
      try {
        await this.request("shutdown", null);
        this.notify("exit", null);
      } catch {
        this.notify("exit", null);
      }
    }
    this.socket?.close();
    this.socket = undefined;
    this.rejectPending(new Error("Dawn language service disposed"));
  }

  private async initialize(): Promise<void> {
    const rootUri = monaco.Uri.file(this.options.projectRoot).toString();
    await this.request("initialize", {
      processId: null,
      rootUri,
      workspaceFolders: [{ uri: rootUri, name: "Dawn Project" }],
      capabilities: {
        textDocument: {
          completion: { completionItem: { snippetSupport: false } },
          hover: {},
          definition: {},
          documentSymbol: {},
          semanticTokens: {
            dynamicRegistration: false,
            requests: { full: true, range: false },
            tokenTypes: TOKEN_LEGEND,
            tokenModifiers: [],
            formats: ["relative"],
            overlappingTokenSupport: false,
            multilineTokenSupport: true
          }
        },
        workspace: { workspaceFolders: true }
      },
      initializationOptions: {}
    });
    this.notify("initialized", {});
  }

  private registerProviders(): void {
    this.providerDisposables.push(
      ...DAWN_LANGUAGE_IDS.map((languageId) => monaco.languages.registerCompletionItemProvider(languageId, {
        triggerCharacters: [".", "/", "\"", "<"],
        provideCompletionItems: async (model, position) => {
          const result = await this.request("textDocument/completion", {
            textDocument: textDocument(model),
            position: toLspPosition(position)
          });
          const items = Array.isArray(result)
            ? result
            : isRecord(result) && Array.isArray(result.items)
              ? result.items
              : [];
          const range = model.getWordUntilPosition(position);
          const replacementRange = new monaco.Range(
            position.lineNumber,
            range.startColumn,
            position.lineNumber,
            range.endColumn
          );
          return {
            suggestions: items.map((item) => completionItem(item as LspCompletionItem, replacementRange))
          };
        }
      })),
      ...DAWN_LANGUAGE_IDS.map((languageId) => monaco.languages.registerHoverProvider(languageId, {
        provideHover: async (model, position) => {
          const hover = await this.request("textDocument/hover", {
            textDocument: textDocument(model),
            position: toLspPosition(position)
          });
          if (!isRecord(hover)) return null;
          return {
            range: isRange(hover.range) ? toMonacoRange(hover.range) : undefined,
            contents: hoverContents(hover.contents)
          };
        }
      })),
      ...DAWN_LANGUAGE_IDS.map((languageId) => monaco.languages.registerDefinitionProvider(languageId, {
        provideDefinition: async (model, position) => {
          const definition = await this.request("textDocument/definition", {
            textDocument: textDocument(model),
            position: toLspPosition(position)
          });
          return definitionLocations(definition);
        }
      })),
      ...DAWN_LANGUAGE_IDS.map((languageId) => monaco.languages.registerDocumentSymbolProvider(languageId, {
        provideDocumentSymbols: async (model) => {
          const result = await this.request("textDocument/documentSymbol", {
            textDocument: textDocument(model)
          });
          if (!Array.isArray(result)) return [];
          return result.map((symbol) => documentSymbol(symbol as LspDocumentSymbol));
        }
      })),
      ...DAWN_LANGUAGE_IDS.map((languageId) => monaco.languages.registerDocumentSemanticTokensProvider(languageId, {
        getLegend: () => ({ tokenTypes: TOKEN_LEGEND, tokenModifiers: [] }),
        provideDocumentSemanticTokens: async (model) => {
          const result = await this.request("textDocument/semanticTokens/full", {
            textDocument: textDocument(model)
          });
          const data = isRecord(result) && Array.isArray(result.data) ? result.data : [];
          return { data: new Uint32Array(data.flatMap(semanticTokenData)) };
        },
        releaseDocumentSemanticTokens: () => undefined
      }))
    );
  }

  private request(method: string, params: unknown): Promise<unknown> {
    const socket = this.socket;
    if (!socket || socket.readyState !== WebSocket.OPEN) {
      return Promise.reject(new Error("Dawn language service is not connected"));
    }

    const id = this.nextId++;
    const message: JsonRpcMessage = { jsonrpc: "2.0", id, method, params };
    return new Promise((resolve, reject) => {
      this.pending.set(id, { resolve, reject });
      socket.send(JSON.stringify(message));
    });
  }

  private notify(method: string, params: unknown): void {
    const socket = this.socket;
    if (!socket || socket.readyState !== WebSocket.OPEN) return;
    socket.send(JSON.stringify({ jsonrpc: "2.0", method, params }));
  }

  private handleMessage(data: unknown): void {
    const text = typeof data === "string" ? data : "";
    let message: JsonRpcMessage;
    try {
      message = JSON.parse(text) as JsonRpcMessage;
    } catch {
      return;
    }

    if (typeof message.id === "number") {
      const pending = this.pending.get(message.id);
      if (!pending) return;
      this.pending.delete(message.id);
      if (message.error) {
        pending.reject(new Error(message.error.message));
      } else {
        pending.resolve(message.result);
      }
      return;
    }

    if (message.method === "textDocument/publishDiagnostics" && isRecord(message.params)) {
      this.publishDiagnostics(message.params);
    }
  }

  private publishDiagnostics(params: Record<string, unknown>): void {
    const uri = typeof params.uri === "string" ? params.uri : "";
    const diagnostics = Array.isArray(params.diagnostics) ? params.diagnostics : [];
    const model = monaco.editor.getModel(monaco.Uri.parse(uri));
    const markers = diagnostics.map((diagnostic) => markerData(diagnostic as LspDiagnostic));
    if (model) {
      monaco.editor.setModelMarkers(model, MARKER_OWNER, markers);
    }

    const path = pathFromUri(uri);
    const problems = diagnostics
      .map((diagnostic) => languageProblem(path, diagnostic as LspDiagnostic))
      .sort(compareProblems);
    if (problems.length === 0) {
      this.diagnostics.delete(uri);
    } else {
      this.diagnostics.set(uri, problems);
    }
    this.options.onDiagnostics([...this.diagnostics.values()].flat().sort(compareProblems));
  }

  private clearMarkers(uri: string): void {
    const model = monaco.editor.getModel(monaco.Uri.parse(uri));
    if (model) {
      monaco.editor.setModelMarkers(model, MARKER_OWNER, []);
    }
    this.diagnostics.delete(uri);
    this.options.onDiagnostics([...this.diagnostics.values()].flat().sort(compareProblems));
  }

  private rejectPending(error: Error): void {
    for (const pending of this.pending.values()) {
      pending.reject(error);
    }
    this.pending.clear();
  }
}

function isDawnModel(model: monaco.editor.ITextModel): boolean {
  return DAWN_LANGUAGE_IDS.includes(model.getLanguageId()) && model.uri.scheme === "file";
}

const dawnYamlMonarch: monaco.languages.IMonarchLanguage = {
  tokenizer: {
    root: [
      [/#.*$/, "comment"],
      [/^[ \t-]*([A-Za-z_][A-Za-z0-9_]*)(?=\s*:)/, "keyword"],
      [/"([^"\\]|\\.)*"/, "string"],
      [/'[^']*'/, "string"],
      [/#[0-9A-Fa-f]{6}\b/, "number.hex"],
      [/\b\d+(\.\d+)?s?\b/, "number"],
      [/\b(true|false|null)\b/, "keyword"],
      [/[{}\[\],:]/, "delimiter"]
    ]
  }
};

const dawnJsonlMonarch: monaco.languages.IMonarchLanguage = {
  tokenizer: {
    root: [
      [/"([^"\\]|\\.)*"(?=\s*:)/, "keyword"],
      [/"([^"\\]|\\.)*"/, "string"],
      [/#(?:[0-9A-Fa-f]{6}|[0-9A-Fa-f]{3})\b/, "number.hex"],
      [/\b-?\d+(\.\d+)?\b/, "number"],
      [/\b(true|false|null)\b/, "keyword"],
      [/[{}\[\],:]/, "delimiter"]
    ]
  }
};

function textDocument(model: monaco.editor.ITextModel): LspTextDocumentIdentifier {
  return { uri: model.uri.toString() };
}

function toLspPosition(position: monaco.Position): LspPosition {
  return {
    line: position.lineNumber - 1,
    character: position.column - 1
  };
}

function toMonacoRange(range: LspRange): monaco.Range {
  return new monaco.Range(
    range.start.line + 1,
    range.start.character + 1,
    range.end.line + 1,
    range.end.character + 1
  );
}

function markerData(diagnostic: LspDiagnostic): monaco.editor.IMarkerData {
  const range = diagnostic.range;
  const startLineNumber = range.start.line + 1;
  const startColumn = range.start.character + 1;
  const endLineNumber = range.end.line + 1;
  const endColumn = Math.max(range.end.character + 1, startColumn + 1);
  return {
    severity: markerSeverity(diagnostic.severity),
    message: diagnostic.message,
    source: diagnostic.source,
    code: diagnostic.code === undefined ? undefined : String(diagnostic.code),
    startLineNumber,
    startColumn,
    endLineNumber,
    endColumn
  };
}

function languageProblem(path: string, diagnostic: LspDiagnostic): LanguageProblem {
  return {
    path,
    message: diagnostic.message,
    severity: severityLabel(diagnostic.severity),
    source: diagnostic.source,
    code: diagnostic.code === undefined ? undefined : String(diagnostic.code),
    line: diagnostic.range.start.line + 1,
    column: diagnostic.range.start.character + 1,
    endLine: diagnostic.range.end.line + 1,
    endColumn: diagnostic.range.end.character + 1
  };
}

function markerSeverity(severity: number | undefined): monaco.MarkerSeverity {
  switch (severity) {
    case 1:
      return monaco.MarkerSeverity.Error;
    case 2:
      return monaco.MarkerSeverity.Warning;
    case 3:
      return monaco.MarkerSeverity.Info;
    case 4:
      return monaco.MarkerSeverity.Hint;
    default:
      return monaco.MarkerSeverity.Info;
  }
}

function severityLabel(severity: number | undefined): LanguageProblem["severity"] {
  switch (severity) {
    case 1:
      return "Error";
    case 2:
      return "Warning";
    case 4:
      return "Hint";
    case 3:
    default:
      return "Info";
  }
}

function completionItem(
  item: LspCompletionItem,
  range: monaco.IRange
): monaco.languages.CompletionItem {
  return {
    label: item.label,
    kind: completionKind(item.kind),
    detail: item.detail,
    documentation: markdown(item.documentation),
    insertText: item.insertText ?? item.label,
    range
  };
}

function completionKind(kind: number | undefined): monaco.languages.CompletionItemKind {
  switch (kind) {
    case 3:
      return monaco.languages.CompletionItemKind.Function;
    case 6:
      return monaco.languages.CompletionItemKind.Variable;
    case 7:
      return monaco.languages.CompletionItemKind.Class;
    case 9:
      return monaco.languages.CompletionItemKind.Module;
    case 14:
      return monaco.languages.CompletionItemKind.Keyword;
    case 17:
      return monaco.languages.CompletionItemKind.File;
    case 25:
      return monaco.languages.CompletionItemKind.TypeParameter;
    default:
      return monaco.languages.CompletionItemKind.Text;
  }
}

function hoverContents(contents: unknown): monaco.IMarkdownString[] {
  if (Array.isArray(contents)) {
    return contents.flatMap(hoverContents);
  }
  const value = markdown(contents);
  return value ? [value] : [];
}

function markdown(value: unknown): monaco.IMarkdownString | undefined {
  if (typeof value === "string") return { value };
  if (isRecord(value)) {
    if (typeof value.value === "string") return { value: value.value };
    if (typeof value.language === "string" && typeof value.value === "string") {
      return { value: `\`\`\`${value.language}\n${value.value}\n\`\`\`` };
    }
  }
  return undefined;
}

function definitionLocations(value: unknown): monaco.languages.Definition {
  if (!value) return [];
  if (Array.isArray(value)) {
    return value.flatMap((item) => definitionLocations(item) as monaco.languages.Location[]);
  }
  if (isLocation(value)) {
    return [{ uri: monaco.Uri.parse(value.uri), range: toMonacoRange(value.range) }];
  }
  if (isRecord(value) && typeof value.targetUri === "string" && isRange(value.targetRange)) {
    return [{ uri: monaco.Uri.parse(value.targetUri), range: toMonacoRange(value.targetRange) }];
  }
  return [];
}

function documentSymbol(symbol: LspDocumentSymbol): monaco.languages.DocumentSymbol {
  return {
    name: symbol.name,
    detail: symbol.detail ?? "",
    kind: symbolKind(symbol.kind),
    tags: [],
    range: toMonacoRange(symbol.range),
    selectionRange: toMonacoRange(symbol.selectionRange),
    children: symbol.children?.map(documentSymbol) ?? []
  };
}

function symbolKind(kind: number): monaco.languages.SymbolKind {
  switch (kind) {
    case 1:
      return monaco.languages.SymbolKind.File;
    case 2:
      return monaco.languages.SymbolKind.Module;
    case 5:
      return monaco.languages.SymbolKind.Class;
    case 6:
      return monaco.languages.SymbolKind.Method;
    case 12:
      return monaco.languages.SymbolKind.Function;
    case 13:
      return monaco.languages.SymbolKind.Variable;
    default:
      return monaco.languages.SymbolKind.Object;
  }
}

function semanticTokenData(token: unknown): number[] {
  if (typeof token === "number") return [token];
  if (isRecord(token)) {
    const values = [
      token.deltaLine,
      token.deltaStart,
      token.length,
      token.tokenType,
      token.tokenModifiersBitset
    ];
    if (values.every((value) => typeof value === "number")) {
      return values as number[];
    }
  }
  return [];
}

function isLocation(value: unknown): value is LspLocation {
  return isRecord(value) && typeof value.uri === "string" && isRange(value.range);
}

function isRange(value: unknown): value is LspRange {
  return (
    isRecord(value) &&
    isPosition(value.start) &&
    isPosition(value.end)
  );
}

function isPosition(value: unknown): value is LspPosition {
  return (
    isRecord(value) &&
    typeof value.line === "number" &&
    typeof value.character === "number"
  );
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null;
}

function pathFromUri(uri: string): string {
  try {
    return monaco.Uri.parse(uri).fsPath;
  } catch {
    return uri;
  }
}

function compareProblems(left: LanguageProblem, right: LanguageProblem): number {
  return (
    left.path.localeCompare(right.path) ||
    left.line - right.line ||
    left.column - right.column ||
    left.message.localeCompare(right.message)
  );
}

function asError(error: unknown): Error {
  return error instanceof Error ? error : new Error(String(error));
}
