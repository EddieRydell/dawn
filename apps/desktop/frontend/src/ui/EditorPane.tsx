import { defaultKeymap } from "@codemirror/commands";
import { cpp } from "@codemirror/lang-cpp";
import { yaml } from "@codemirror/lang-yaml";
import { HighlightStyle, syntaxHighlighting } from "@codemirror/language";
import { lintGutter, lintKeymap, linter, openLintPanel, setDiagnostics, type Diagnostic } from "@codemirror/lint";
import { EditorState, type Extension } from "@codemirror/state";
import { EditorView, keymap, ViewPlugin, ViewUpdate } from "@codemirror/view";
import { tags } from "@lezer/highlight";
import { X } from "lucide-react";
import { useEffect, useRef } from "react";
import { commands } from "../api";
import type { AppSnapshotDto, ProjectDiagnosticDto, TextRangeDto } from "../bindings";
import { commandRegistry } from "../commandRegistry";
import { runSnapshotCommand, useAppStore } from "../store";
import { OPEN_ACTIVE_EDITOR_DIAGNOSTICS_EVENT } from "../uiEvents";
import { GuiEditor, SequenceTransportControls } from "./GuiEditor";

export function EditorPane({ snapshot }: { snapshot: AppSnapshotDto }) {
  const { localText, setLocalText } = useAppStore();
  const editorHost = useRef<HTMLDivElement | null>(null);
  const view = useRef<EditorView | null>(null);
  const latestLocalText = useRef(localText);
  const applyingExternalText = useRef(false);
  const activePath = snapshot.activeBuffer?.path ?? null;
  const viewMode = snapshot.activeBuffer?.viewMode ?? "text";
  const activeSequenceDocument =
    viewMode === "gui" && snapshot.activeGuiDocument?.type === "sequence" ? snapshot.activeGuiDocument.document : null;

  useEffect(() => {
    latestLocalText.current = localText;
  }, [localText]);

  useEffect(() => {
    const onOpenDiagnostics = () => {
      if (view.current === null || viewMode !== "text") return;
      openLintPanel(view.current);
    };
    window.addEventListener(OPEN_ACTIVE_EDITOR_DIAGNOSTICS_EVENT, onOpenDiagnostics);
    return () => {
      window.removeEventListener(OPEN_ACTIVE_EDITOR_DIAGNOSTICS_EVENT, onOpenDiagnostics);
    };
  }, [viewMode]);

  useEffect(() => {
    if (viewMode !== "text") {
      view.current?.destroy();
      view.current = null;
      return;
    }
    if (!editorHost.current || view.current) return;
    view.current = new EditorView({
      parent: editorHost.current,
      state: createState(
        latestLocalText.current,
        activePath,
        (update) => {
          if (!update.docChanged) return;
          if (applyingExternalText.current) return;
          const text = update.state.doc.toString();
          setLocalText(text);
          scheduleAutosave(text);
        },
        async (text, redo) => {
          window.clearTimeout(autosaveTimer);
          if (!redo) {
            await runSnapshotCommand(() => commands.updateActiveText(text));
            await runSnapshotCommand(commands.undoActiveEdit);
          } else {
            await runSnapshotCommand(commands.redoActiveEdit);
          }
        }
      )
    });
    return () => {
      view.current?.destroy();
      view.current = null;
    };
  }, [activePath, setLocalText, viewMode]);

  useEffect(() => {
    if (!view.current) return;
    if (viewMode !== "text") return;
    const current = view.current.state.doc.toString();
    if (current !== localText) {
      applyingExternalText.current = true;
      view.current.dispatch({
        changes: { from: 0, to: current.length, insert: localText }
      });
      applyingExternalText.current = false;
    }
  }, [activePath, localText, viewMode]);

  useEffect(() => {
    if (!view.current) return;
    if (viewMode !== "text") return;
    const diagnostics = editorDiagnostics(snapshot.diagnostics, activePath, snapshot.projectRoot, view.current);
    view.current.dispatch(setDiagnostics(view.current.state, diagnostics));
  }, [activePath, snapshot.diagnostics, snapshot.projectRoot, viewMode]);

  if (snapshot.tabs.length === 0) {
    return (
      <section className="editor-shell empty-editor">
        <span>{snapshot.projectRoot !== null ? "Open a Dawn file from the project tree." : "Open a project to start."}</span>
      </section>
    );
  }

  return (
    <section className="editor-shell">
      <div className="tab-strip">
        {snapshot.tabs.map((tab) => (
          <button
            key={tab.path}
            className={`tab ${tab.path === snapshot.activeFile ? "active" : ""}`}
            onClick={() => void runSnapshotCommand(() => commands.setActiveFile(tab.path))}
          >
            <span>{tab.name}</span>
            {tab.dirty && <span className="dirty-dot" />}
            <X
              size={14}
              onClick={(event) => {
                event.stopPropagation();
                void runSnapshotCommand(() => commands.closeFile(tab.path));
              }}
            />
          </button>
        ))}
      </div>
      <div className="editor-toolbar">
        {activeSequenceDocument !== null && (
          <SequenceTransportControls document={activeSequenceDocument} preview={snapshot.preview} />
        )}
        <div className="segmented-control">
          <button
            className={viewMode === "text" ? "active" : ""}
            onClick={() => void runSnapshotCommand(() => commands.setActiveViewMode("text"))}
          >
            Text
          </button>
          <button
            className={viewMode === "gui" ? "active" : ""}
            onClick={() => void runSnapshotCommand(() => commands.setActiveViewMode("gui"))}
          >
            GUI
          </button>
        </div>
      </div>
      {viewMode === "gui" ? <GuiEditor snapshot={snapshot} /> : <div ref={editorHost} className="editor-host" />}
    </section>
  );
}

let autosaveTimer: number | undefined;

function scheduleAutosave(text: string) {
  window.clearTimeout(autosaveTimer);
  autosaveTimer = window.setTimeout(() => {
    void runSnapshotCommand(() => commands.updateActiveText(text));
  }, 450);
}

function squiggleDataUri(color: string): string {
  const svg = `<svg xmlns="http://www.w3.org/2000/svg" width="6" height="3" viewBox="0 0 6 3"><path d="M0 2.2 Q1.5 .8 3 2.2 T6 2.2" fill="none" stroke="${color}" stroke-width=".8"/></svg>`;
  return `url("data:image/svg+xml,${encodeURIComponent(svg)}")`;
}

const minLintPanelHeight = 96;
const maxLintPanelHeight = 420;
let lintPanelHeight = 260;

function createState(
  text: string,
  path: string | null,
  onUpdate: (update: ViewUpdate) => void,
  onHistoryCommand: (text: string, redo: boolean) => Promise<void>
) {
  return EditorState.create({
    doc: text,
    extensions: [
      languageForPath(path),
      syntaxHighlighting(dawnHighlightStyle),
      linter(null, { autoPanel: false }),
      lintGutter(),
      resizableLintPanel(),
      keymap.of([
        {
          key: "Mod-s",
          run: () => {
            void commandRegistry["file.save"].run();
            return true;
          }
        },
        {
          key: "Mod-z",
          run: (view) => {
            void onHistoryCommand(view.state.doc.toString(), false);
            return true;
          }
        },
        {
          key: "Mod-Shift-z",
          run: () => {
            void onHistoryCommand("", true);
            return true;
          }
        },
        ...defaultKeymap,
        ...lintKeymap
      ]),
      EditorView.updateListener.of(onUpdate),
      EditorView.theme({
        "&": { height: "100%", backgroundColor: "#17181b", color: "#ebe7df" },
        ".cm-scroller": { fontFamily: "Consolas, 'SFMono-Regular', monospace", fontSize: "13px" },
        ".cm-content": { caretColor: "#6abf8a" },
        ".cm-cursor": { borderLeftColor: "#6abf8a" },
        ".cm-selectionBackground": { backgroundColor: "#31543f !important" },
        ".cm-gutters": { backgroundColor: "#1d1f23", color: "#77736d", borderRight: "1px solid #373b42" },
        ".cm-lintRange": {
          backgroundPosition: "left calc(100% - 1px)",
          backgroundRepeat: "repeat-x",
          paddingBottom: "0",
          textDecoration: "none"
        },
        ".cm-lintRange-error": { backgroundImage: squiggleDataUri("#df6b6b") },
        ".cm-lintRange-warning": { backgroundImage: squiggleDataUri("#e3a84f") },
        ".cm-lintRange-active": { backgroundColor: "transparent" },
        ".cm-tooltip.cm-tooltip-lint": {
          border: "1px solid #454a53",
          borderRadius: "5px",
          backgroundColor: "#151619",
          boxShadow: "0 10px 26px rgb(0 0 0 / 38%)",
          color: "#ebe7df"
        },
        ".cm-tooltip-lint .cm-diagnostic": {
          maxWidth: "420px",
          borderLeft: "0",
          padding: "7px 9px",
          color: "#ebe7df",
          fontFamily: "Inter, ui-sans-serif, system-ui, sans-serif",
          fontSize: "12px",
          lineHeight: "1.35"
        },
        ".cm-tooltip-lint .cm-diagnostic-location, .cm-tooltip-lint .cm-diagnostic-code": { display: "none" },
        ".cm-gutter-lint": { width: "18px" },
        ".cm-gutter-lint .cm-gutterElement": {
          display: "grid",
          placeItems: "center",
          padding: "0"
        },
        ".cm-lint-marker": {
          width: "3px",
          height: "14px",
          borderRadius: "999px",
          backgroundImage: "none !important",
          content: '"" !important',
          boxShadow: "0 0 0 1px #17181b"
        },
        ".cm-lint-marker-error": { backgroundColor: "#df6b6b" },
        ".cm-lint-marker-warning": { backgroundColor: "#e3a84f" },
        ".cm-panel.cm-panel-lint": {
          position: "relative",
          borderTop: "1px solid #373b42",
          backgroundColor: "#181a1e",
          color: "#d8d2c9",
          fontFamily: "Inter, ui-sans-serif, system-ui, sans-serif",
          fontSize: "12px",
          paddingTop: "9px"
        },
        ".cm-panel.cm-panel-lint::before": {
          content: '""',
          position: "absolute",
          top: "0",
          left: "0",
          right: "0",
          height: "9px",
          borderBottom: "1px solid #242830",
          background:
            "linear-gradient(to bottom, #22262d, #1a1d22), linear-gradient(to right, transparent, #575d68, transparent)",
          backgroundSize: "100% 100%, 120px 1px",
          backgroundPosition: "0 0, center 4px",
          backgroundRepeat: "no-repeat",
          cursor: "ns-resize"
        },
        ".cm-panel.cm-panel-lint ul": {
          height: `${lintPanelHeight}px`,
          minHeight: `${minLintPanelHeight}px`,
          maxHeight: `${maxLintPanelHeight}px`,
          margin: "0",
          padding: "6px 0",
          overflowY: "auto"
        },
        ".cm-panel.cm-panel-lint .cm-diagnostic": {
          display: "grid",
          gridTemplateColumns: "72px 112px minmax(0, 1fr) auto",
          alignItems: "start",
          gap: "10px",
          minHeight: "30px",
          margin: "0",
          borderLeftWidth: "3px",
          borderLeftStyle: "solid",
          padding: "7px 38px 7px 10px",
          color: "#d8d2c9",
          whiteSpace: "normal"
        },
        ".cm-panel.cm-panel-lint .cm-diagnostic-error": { borderLeftColor: "#df6b6b" },
        ".cm-panel.cm-panel-lint .cm-diagnostic-warning": { borderLeftColor: "#e3a84f" },
        ".cm-panel.cm-panel-lint .cm-diagnostic-info": { borderLeftColor: "#8f9298" },
        ".cm-panel.cm-panel-lint .cm-diagnostic[aria-selected]": {
          backgroundColor: "#252931",
          color: "#fffaf0"
        },
        ".cm-panel.cm-panel-lint .cm-diagnosticText": {
          display: "contents"
        },
        ".cm-panel.cm-panel-lint .cm-diagnosticText::before": {
          fontWeight: "700",
          textTransform: "uppercase",
          letterSpacing: "0",
          color: "#a8a29a"
        },
        ".cm-panel.cm-panel-lint .cm-diagnostic-error .cm-diagnosticText::before": {
          content: '"Error"',
          color: "#ff9a9a"
        },
        ".cm-panel.cm-panel-lint .cm-diagnostic-warning .cm-diagnosticText::before": {
          content: '"Warning"',
          color: "#f0c46b"
        },
        ".cm-panel.cm-panel-lint .cm-diagnostic-info .cm-diagnosticText::before": {
          content: '"Info"'
        },
        ".cm-panel.cm-panel-lint .cm-diagnostic-message": {
          minWidth: "0",
          overflowWrap: "anywhere",
          lineHeight: "1.35",
          color: "inherit"
        },
        ".cm-panel.cm-panel-lint .cm-diagnostic-location": {
          color: "#8f9298",
          fontVariantNumeric: "tabular-nums"
        },
        ".cm-panel.cm-panel-lint .cm-diagnostic-code": {
          justifySelf: "end",
          maxWidth: "180px",
          overflow: "hidden",
          color: "#8f9298",
          textOverflow: "ellipsis",
          whiteSpace: "nowrap"
        },
        ".cm-panel.cm-panel-lint .cm-diagnostic-info .cm-diagnosticText": {
          display: "block",
          gridColumn: "1 / -1",
          color: "#8f9298"
        },
        ".cm-panel.cm-panel-lint [name=close]": {
          position: "absolute",
          top: "6px",
          right: "8px",
          width: "22px",
          height: "22px",
          border: "0",
          borderRadius: "4px",
          background: "transparent",
          color: "#a8a29a",
          font: "inherit",
          lineHeight: "1",
          cursor: "pointer"
        },
        ".cm-panel.cm-panel-lint [name=close]:hover": {
          backgroundColor: "#2c3036",
          color: "#ebe7df"
        }
      })
    ]
  });
}

function resizableLintPanel(): Extension {
  return [
    lintPanelHeightSync,
    EditorView.domEventHandlers({
      mousedown(event, view) {
        const panel = lintPanelFromTarget(event.target);
        if (panel === null) return false;
        const rect = panel.getBoundingClientRect();
        if (event.clientY - rect.top > 9) return false;
        const list = panel.querySelector("ul");
        if (list === null) return false;

        event.preventDefault();
        const startY = event.clientY;
        const startHeight = list.getBoundingClientRect().height;

        const onMouseMove = (moveEvent: MouseEvent) => {
          lintPanelHeight = clampPanelHeight(startHeight + startY - moveEvent.clientY);
          applyLintPanelHeight(view);
        };
        const onMouseUp = () => {
          window.removeEventListener("mousemove", onMouseMove);
          window.removeEventListener("mouseup", onMouseUp);
        };

        window.addEventListener("mousemove", onMouseMove);
        window.addEventListener("mouseup", onMouseUp);
        return true;
      }
    })
  ];
}

const lintPanelHeightSync = ViewPlugin.fromClass(
  class {
    constructor(private readonly view: EditorView) {
      applyLintPanelHeight(view);
    }

    update() {
      applyLintPanelHeight(this.view);
    }
  }
);

function lintPanelFromTarget(target: EventTarget | null): HTMLElement | null {
  if (!(target instanceof Element)) return null;
  return target.closest(".cm-panel-lint");
}

function applyLintPanelHeight(view: EditorView) {
  const list = view.dom.querySelector<HTMLElement>(".cm-panel-lint ul");
  if (list === null) return;
  list.style.height = `${lintPanelHeight}px`;
}

function clampPanelHeight(height: number): number {
  return Math.max(minLintPanelHeight, Math.min(maxLintPanelHeight, height));
}

function editorDiagnostics(
  diagnostics: ProjectDiagnosticDto[],
  activePath: string | null,
  projectRoot: string | null,
  view: EditorView
): Diagnostic[] {
  if (activePath === null) return [];
  return diagnostics.flatMap((diagnostic) => {
    if (!samePath(diagnostic.path, activePath, projectRoot)) return [];
    const range = diagnostic.range !== null ? rangeToOffsets(diagnostic.range, view) : pointDiagnosticRange(view);
    if (range === null) return [];
    return [
      {
        from: range.from,
        to: range.to,
        severity: diagnostic.severity,
        message: diagnostic.message,
        renderMessage: () => renderDiagnosticMessage(diagnostic, range.from, view)
      }
    ];
  });
}

function renderDiagnosticMessage(diagnostic: ProjectDiagnosticDto, from: number, view: EditorView): Node {
  const fragment = document.createDocumentFragment();
  fragment.append(
    textSpan("cm-diagnostic-location", diagnosticLocation(from, view)),
    textSpan("cm-diagnostic-message", diagnostic.message),
    textSpan("cm-diagnostic-code", diagnostic.code.trim())
  );
  return fragment;
}

function textSpan(className: string, text: string): HTMLSpanElement {
  const span = document.createElement("span");
  span.className = className;
  span.textContent = text;
  return span;
}

function diagnosticLocation(from: number, view: EditorView): string {
  const line = view.state.doc.lineAt(from);
  return `${line.number}:${from - line.from + 1}`;
}

function samePath(left: string, right: string, projectRoot: string | null): boolean {
  const normalizedLeft = normalizePath(left);
  const normalizedRight = normalizePath(right);
  if (normalizedLeft === normalizedRight) return true;
  if (projectRoot === null || isAbsolutePath(right)) return false;
  return normalizedLeft === normalizePath(`${projectRoot}/${right}`);
}

function normalizePath(path: string): string {
  return path.replace(/^\/\/\?\//, "").replace(/\\/g, "/").toLowerCase();
}

function isAbsolutePath(path: string): boolean {
  const normalized = normalizePath(path);
  return /^[a-z]:\//.test(normalized) || normalized.startsWith("/");
}

function pointDiagnosticRange(view: EditorView): { from: number; to: number } | null {
  if (view.state.doc.length === 0) return { from: 0, to: 0 };
  const line = view.state.doc.line(1);
  const firstContent = line.text.search(/\S/);
  const from = line.from + (firstContent >= 0 ? firstContent : 0);
  return { from, to: from };
}

function rangeToOffsets(range: TextRangeDto, view: EditorView): { from: number; to: number } | null {
  const from = positionToOffset(range.start.line, range.start.character, view);
  const rawTo = positionToOffset(range.end.line, range.end.character, view);
  if (from === null || rawTo === null) return null;
  let to = Math.max(rawTo, from);
  if (to === from && from < view.state.doc.length) {
    to += 1;
  }
  return to > from ? { from, to } : null;
}

function positionToOffset(line: number, character: number, view: EditorView): number | null {
  if (!Number.isFinite(line) || !Number.isFinite(character) || line < 0 || character < 0) return null;
  const doc = view.state.doc;
  if (doc.lines === 0) return 0;
  const lineNumber = Math.min(Math.floor(line) + 1, doc.lines);
  const docLine = doc.line(lineNumber);
  const offset = Math.min(Math.floor(character), docLine.length);
  return docLine.from + offset;
}

function languageForPath(path: string | null): Extension {
  if (path !== null && path.endsWith(".effect.dawn")) {
    return cpp();
  }
  return yaml();
}

const dawnHighlightStyle = HighlightStyle.define([
  { tag: tags.keyword, color: "#d99adf" },
  { tag: [tags.name, tags.propertyName, tags.attributeName], color: "#87c7ff" },
  { tag: [tags.variableName, tags.definition(tags.variableName)], color: "#ebe7df" },
  { tag: [tags.function(tags.variableName), tags.function(tags.definition(tags.variableName))], color: "#8fd6b5" },
  { tag: [tags.string, tags.special(tags.string)], color: "#e8bf7a" },
  { tag: [tags.number, tags.bool, tags.null], color: "#f09a86" },
  { tag: [tags.operator, tags.punctuation, tags.separator], color: "#a8a29a" },
  { tag: tags.comment, color: "#77736d", fontStyle: "italic" },
  { tag: [tags.typeName, tags.className], color: "#a6d189" },
  { tag: tags.invalid, color: "#ff8f8f" }
]);
