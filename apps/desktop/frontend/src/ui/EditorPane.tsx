import { defaultKeymap } from "@codemirror/commands";
import { cpp } from "@codemirror/lang-cpp";
import { yaml } from "@codemirror/lang-yaml";
import { HighlightStyle, syntaxHighlighting } from "@codemirror/language";
import { linter, setDiagnostics, type Diagnostic } from "@codemirror/lint";
import { EditorState, type Extension } from "@codemirror/state";
import { EditorView, keymap, ViewUpdate } from "@codemirror/view";
import { tags } from "@lezer/highlight";
import { X } from "lucide-react";
import { useCallback, useEffect, useRef, useState, type PointerEvent } from "react";
import { commands } from "../api";
import type { AppSnapshotDto, ProjectDiagnosticDto, TextRangeDto } from "../bindings";
import { commandRegistry } from "../commandRegistry";
import { runSnapshotCommand, useAppStore } from "../store";
import { GuiEditor, SequenceTransportControls } from "./GuiEditor";

export function EditorPane({ snapshot }: { snapshot: AppSnapshotDto }) {
  const { localText, setLocalText } = useAppStore();
  const editorHost = useRef<HTMLDivElement | null>(null);
  const view = useRef<EditorView | null>(null);
  const [editorView, setEditorView] = useState<EditorView | null>(null);
  const [editorSignal, setEditorSignal] = useState(0);
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
    if (viewMode !== "text") {
      view.current?.destroy();
      view.current = null;
      return;
    }
    if (!editorHost.current || view.current) return;
    const nextView = new EditorView({
      parent: editorHost.current,
      state: createState(
        latestLocalText.current,
        activePath,
        (update) => {
          if (update.docChanged || update.viewportChanged || update.geometryChanged) {
            setEditorSignal((signal) => signal + 1);
          }
          if (update.docChanged && !applyingExternalText.current) {
            const text = update.state.doc.toString();
            setLocalText(text);
            scheduleAutosave(text);
          }
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
    view.current = nextView;
    let disposed = false;
    window.requestAnimationFrame(() => {
      if (disposed) return;
      setEditorView(nextView);
      setEditorSignal((signal) => signal + 1);
    });
    return () => {
      disposed = true;
      view.current?.destroy();
      view.current = null;
      window.requestAnimationFrame(() => {
        setEditorView(null);
      });
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
      {viewMode === "gui" ? (
        <GuiEditor snapshot={snapshot} />
      ) : (
        <div className="editor-scrollbar-shell">
          <div ref={editorHost} className="editor-host" />
          <EditorScrollbar
            activePath={activePath}
            diagnostics={snapshot.diagnostics}
            editorSignal={editorSignal}
            projectRoot={snapshot.projectRoot}
            view={editorView}
          />
        </div>
      )}
    </section>
  );
}

type ScrollbarMetrics = {
  scrollTop: number;
  clientHeight: number;
  scrollHeight: number;
  railHeight: number;
  thumbTop: number;
  thumbHeight: number;
  scrollable: boolean;
};

type ScrollbarMarker = {
  id: string;
  severity: "error" | "warning";
  from: number;
  line: number;
  column: number;
  message: string;
  code: string;
  topPercent: number;
};

function EditorScrollbar({
  activePath,
  diagnostics,
  editorSignal,
  projectRoot,
  view
}: {
  activePath: string | null;
  diagnostics: ProjectDiagnosticDto[];
  editorSignal: number;
  projectRoot: string | null;
  view: EditorView | null;
}) {
  const railRef = useRef<HTMLDivElement | null>(null);
  const dragRef = useRef<{ pointerId: number; startY: number; startScrollTop: number } | null>(null);
  const frameRef = useRef<number | null>(null);
  const [metrics, setMetrics] = useState<ScrollbarMetrics>(() => emptyScrollbarMetrics());

  const measure = useCallback(() => {
    if (frameRef.current !== null) return;
    frameRef.current = window.requestAnimationFrame(() => {
      frameRef.current = null;
      setMetrics(readScrollbarMetrics(view, railRef.current));
    });
  }, [view]);

  useEffect(() => {
    measure();
  }, [activePath, diagnostics, editorSignal, measure, projectRoot]);

  useEffect(() => {
    if (view === null) {
      return;
    }
    const scrollDOM = view.scrollDOM;
    const observer = new ResizeObserver(measure);
    scrollDOM.addEventListener("scroll", measure, { passive: true });
    observer.observe(scrollDOM);
    if (railRef.current !== null) observer.observe(railRef.current);
    measure();
    return () => {
      scrollDOM.removeEventListener("scroll", measure);
      observer.disconnect();
      if (frameRef.current !== null) {
        window.cancelAnimationFrame(frameRef.current);
        frameRef.current = null;
      }
    };
  }, [editorSignal, measure, view]);

  const markers = editorDiagnosticMarkers(diagnostics, activePath, projectRoot, view);

  const scrollToRatio = useCallback(
    (ratio: number) => {
      if (view === null) return;
      const scrollDOM = view.scrollDOM;
      const maxScrollTop = Math.max(0, scrollDOM.scrollHeight - scrollDOM.clientHeight);
      setScrollTop(scrollDOM, clamp(ratio, 0, 1) * maxScrollTop);
      measure();
    },
    [measure, view]
  );

  const handleTrackPointerDown = useCallback(
    (event: PointerEvent<HTMLDivElement>) => {
      if (event.button !== 0 || view === null || railRef.current === null) return;
      const rect = railRef.current.getBoundingClientRect();
      scrollToRatio((event.clientY - rect.top) / Math.max(1, rect.height));
    },
    [scrollToRatio, view]
  );

  const handleThumbPointerDown = useCallback(
    (event: PointerEvent<HTMLDivElement>) => {
      if (event.button !== 0 || view === null || !metrics.scrollable) return;
      event.preventDefault();
      event.stopPropagation();
      event.currentTarget.setPointerCapture(event.pointerId);
      dragRef.current = {
        pointerId: event.pointerId,
        startY: event.clientY,
        startScrollTop: view.scrollDOM.scrollTop
      };
    },
    [metrics.scrollable, view]
  );

  const handleThumbPointerMove = useCallback(
    (event: PointerEvent<HTMLDivElement>) => {
      if (view === null || dragRef.current === null || dragRef.current.pointerId !== event.pointerId) return;
      const maxScrollTop = Math.max(0, metrics.scrollHeight - metrics.clientHeight);
      const maxThumbTop = Math.max(1, metrics.railHeight - metrics.thumbHeight);
      const deltaScroll = ((event.clientY - dragRef.current.startY) / maxThumbTop) * maxScrollTop;
      setScrollTop(view.scrollDOM, clamp(dragRef.current.startScrollTop + deltaScroll, 0, maxScrollTop));
      measure();
    },
    [measure, metrics.clientHeight, metrics.railHeight, metrics.scrollHeight, metrics.thumbHeight, view]
  );

  const endDrag = useCallback((event: PointerEvent<HTMLDivElement>) => {
    if (dragRef.current === null || dragRef.current.pointerId !== event.pointerId) return;
    dragRef.current = null;
    event.currentTarget.releasePointerCapture(event.pointerId);
  }, []);

  const jumpToMarker = useCallback(
    (marker: ScrollbarMarker) => {
      if (view === null) return;
      view.dispatch({
        selection: { anchor: marker.from },
        effects: EditorView.scrollIntoView(marker.from, { y: "center" })
      });
      view.focus();
      measure();
    },
    [measure, view]
  );

  return (
    <div className="editor-scrollbar" aria-hidden={view === null}>
      <div ref={railRef} className="editor-scrollbar-rail" onPointerDown={handleTrackPointerDown}>
        {markers.map((marker) => (
          <button
            key={marker.id}
            type="button"
            className={`editor-scrollbar-marker ${marker.severity}`}
            style={{ top: `${marker.topPercent}%` }}
            onClick={(event) => {
              event.stopPropagation();
              jumpToMarker(marker);
            }}
            onPointerDown={(event) => {
              event.stopPropagation();
            }}
            aria-label={`${marker.severity} at ${marker.line}:${marker.column}: ${marker.message}`}
          >
            <span className="editor-scrollbar-tooltip">
              <span className="editor-scrollbar-tooltip-location">
                {marker.line}:{marker.column}
              </span>
              <span className="editor-scrollbar-tooltip-message">{marker.message}</span>
              {marker.code.length > 0 && <span className="editor-scrollbar-tooltip-code">{marker.code}</span>}
            </span>
          </button>
        ))}
        <div
          className={`editor-scrollbar-thumb ${metrics.scrollable ? "" : "disabled"}`}
          style={{ height: `${metrics.thumbHeight}px`, transform: `translateY(${metrics.thumbTop}px)` }}
          onPointerDown={handleThumbPointerDown}
          onPointerMove={handleThumbPointerMove}
          onPointerUp={endDrag}
          onPointerCancel={endDrag}
        />
      </div>
    </div>
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
        ...defaultKeymap
      ]),
      EditorView.updateListener.of(onUpdate),
      EditorView.theme({
        "&": { height: "100%", backgroundColor: "#17181b", color: "#ebe7df" },
        ".cm-scroller": {
          fontFamily: "Consolas, 'SFMono-Regular', monospace",
          fontSize: "13px",
          scrollbarWidth: "none"
        },
        ".cm-scroller::-webkit-scrollbar": { display: "none" },
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
          backgroundColor: "#202226",
          boxShadow: "0 10px 26px rgb(0 0 0 / 38%)",
          color: "#ebe7df",
          overflow: "hidden"
        },
        ".cm-tooltip-lint .cm-diagnostic": {
          maxWidth: "420px",
          borderLeft: "0",
          backgroundColor: "#202226",
          padding: "7px 9px",
          color: "#ebe7df",
          fontFamily: "Inter, ui-sans-serif, system-ui, sans-serif",
          fontSize: "12px",
          lineHeight: "1.35"
        },
        ".cm-tooltip-lint .cm-diagnostic-error": { backgroundColor: "#261b1e" },
        ".cm-tooltip-lint .cm-diagnostic-warning": { backgroundColor: "#292319" },
        ".cm-tooltip-lint .cm-diagnosticText": {
          color: "#ebe7df"
        },
        ".cm-tooltip-lint .cm-diagnostic-message": {
          color: "#ebe7df",
          overflowWrap: "anywhere"
        },
        ".cm-tooltip-lint .cm-diagnostic-location, .cm-tooltip-lint .cm-diagnostic-code": { display: "none" }
      })
    ]
  });
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

function editorDiagnosticMarkers(
  diagnostics: ProjectDiagnosticDto[],
  activePath: string | null,
  projectRoot: string | null,
  view: EditorView | null
): ScrollbarMarker[] {
  if (activePath === null || view === null) return [];
  const contentHeight = Math.max(1, view.contentHeight);
  return diagnostics.flatMap((diagnostic, index) => {
    if (!samePath(diagnostic.path, activePath, projectRoot)) return [];
    const range = diagnostic.range !== null ? rangeToOffsets(diagnostic.range, view) : pointDiagnosticRange(view);
    if (range === null) return [];
    const line = view.state.doc.lineAt(range.from);
    const block = view.lineBlockAt(range.from);
    return [
      {
        id: `${diagnostic.path}:${index}:${range.from}:${diagnostic.code}`,
        severity: diagnostic.severity,
        from: range.from,
        line: line.number,
        column: range.from - line.from + 1,
        message: diagnostic.message,
        code: diagnostic.code.trim(),
        topPercent: clamp((block.top / contentHeight) * 100, 0, 100)
      }
    ];
  });
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

function emptyScrollbarMetrics(): ScrollbarMetrics {
  return {
    scrollTop: 0,
    clientHeight: 0,
    scrollHeight: 0,
    railHeight: 0,
    thumbTop: 0,
    thumbHeight: 0,
    scrollable: false
  };
}

function readScrollbarMetrics(view: EditorView | null, rail: HTMLElement | null): ScrollbarMetrics {
  if (view === null || rail === null) return emptyScrollbarMetrics();
  const scrollDOM = view.scrollDOM;
  const railHeight = rail.clientHeight;
  const scrollTop = scrollDOM.scrollTop;
  const clientHeight = scrollDOM.clientHeight;
  const scrollHeight = scrollDOM.scrollHeight;
  const scrollable = scrollHeight > clientHeight + 1;
  const thumbHeight = scrollable ? Math.max(28, (clientHeight / scrollHeight) * railHeight) : railHeight;
  const maxScrollTop = Math.max(1, scrollHeight - clientHeight);
  const maxThumbTop = Math.max(0, railHeight - thumbHeight);
  return {
    scrollTop,
    clientHeight,
    scrollHeight,
    railHeight,
    thumbTop: scrollable ? (scrollTop / maxScrollTop) * maxThumbTop : 0,
    thumbHeight,
    scrollable
  };
}

function setScrollTop(scrollDOM: HTMLElement, scrollTop: number): void {
  scrollDOM.scrollTop = scrollTop;
}

function clamp(value: number, min: number, max: number): number {
  return Math.min(Math.max(value, min), max);
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
