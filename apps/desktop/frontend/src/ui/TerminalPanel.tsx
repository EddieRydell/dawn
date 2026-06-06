import { Channel } from "@tauri-apps/api/core";
import { FitAddon } from "@xterm/addon-fit";
import { Terminal } from "@xterm/xterm";
import "@xterm/xterm/css/xterm.css";
import { Plus, Power, TerminalSquare, X } from "lucide-react";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { commands, type TerminalEventDto, type TerminalPanelLayoutDto, type TerminalProfileDto } from "../api";
import { useAppStore } from "../store";
import type { AppSnapshotDto } from "../bindings";

const MIN_PANEL_WIDTH = 300;
const MAX_PANEL_WIDTH = 900;
const DEFAULT_COLS = 100;
const DEFAULT_ROWS = 30;

type TerminalTab = {
  key: number;
  profile: TerminalProfileDto;
  sessionId: number | null;
  status: "starting" | "running" | "exited" | "error";
  error: string | null;
};

type TerminalViewProps = {
  tab: TerminalTab;
  active: boolean;
  projectRoot: string;
  onStarted: (key: number, sessionId: number) => void;
  onEvent: (key: number, event: TerminalEventDto) => void;
};

let nextTerminalKey = 1;

export function TerminalPanel({ snapshot }: { snapshot: AppSnapshotDto }) {
  const layout = snapshot.terminalPanelLayout;
  const initialProfiles = snapshot.projectRoot !== null && layout.visible ? restoredProfiles(layout) : [];
  const [tabs, setTabs] = useState<TerminalTab[]>(() => makeTabs(initialProfiles));
  const [activeIndex, setActiveIndex] = useState(clampActiveIndex(layout.activeTabIndex, initialProfiles.length));
  const [width, setWidth] = useState(layout.widthPx);
  const tabsRef = useRef<TerminalTab[]>([]);

  useEffect(() => {
    tabsRef.current = tabs;
  }, [tabs]);

  useEffect(() => {
    const onTerminalOpen = () => {
      if (tabsRef.current.length > 0) return;
      const nextTabs = makeTabs(restoredProfiles(layout));
      setTabs(nextTabs);
      setActiveIndex(clampActiveIndex(layout.activeTabIndex, nextTabs.length));
    };
    window.addEventListener("dawn:terminal-open", onTerminalOpen);
    return () => {
      window.removeEventListener("dawn:terminal-open", onTerminalOpen);
    };
  }, [layout]);

  const persist = useCallback(
    async (next: Partial<TerminalPanelLayoutDto> & { tabProfiles?: TerminalProfileDto[] }) => {
      const currentTabs = tabsRef.current;
      const tabProfiles = next.tabProfiles ?? currentTabs.map((tab) => tab.profile);
      const clampedActive = clampActiveIndex(next.activeTabIndex ?? activeIndex, tabProfiles.length);
      try {
        const updated = await commands.setTerminalPanelLayout({
          visible: next.visible ?? layout.visible,
          widthPx: next.widthPx ?? width,
          activeTabIndex: clampedActive,
          tabProfiles
        });
        useAppStore.getState().setSnapshot(updated);
        useAppStore.getState().setError(null);
      } catch (error) {
        useAppStore.getState().setError(String(error));
      }
    },
    [activeIndex, layout.visible, width]
  );

  const setVisible = (visible: boolean) => {
    if (visible && tabsRef.current.length === 0) {
      const nextTabs = makeTabs(restoredProfiles(layout));
      setTabs(nextTabs);
      setActiveIndex(clampActiveIndex(layout.activeTabIndex, nextTabs.length));
    }
    void persist({ visible });
  };

  const addTab = (profile: TerminalProfileDto) => {
    const nextTabs = [
      ...tabsRef.current,
      { key: nextTerminalKey++, profile, sessionId: null, status: "starting" as const, error: null }
    ];
    setTabs(nextTabs);
    const nextActive = nextTabs.length - 1;
    setActiveIndex(nextActive);
    void persist({ visible: true, activeTabIndex: nextActive, tabProfiles: nextTabs.map((tab) => tab.profile) });
  };

  const closeTab = (index: number) => {
    const closing = tabsRef.current[index];
    if (closing?.sessionId !== null && closing?.sessionId !== undefined) {
      void commands.killTerminalSession(closing.sessionId);
    }
    const nextTabs = tabsRef.current.filter((_, candidate) => candidate !== index);
    const nextActive = clampActiveIndex(activeIndex >= index ? activeIndex - 1 : activeIndex, nextTabs.length);
    setTabs(nextTabs);
    setActiveIndex(nextActive);
    void persist({ activeTabIndex: nextActive, tabProfiles: nextTabs.map((tab) => tab.profile) });
  };

  const onStarted = useCallback((key: number, sessionId: number) => {
    setTabs((current) =>
      current.map((tab) => (tab.key === key ? { ...tab, sessionId, status: "running", error: null } : tab))
    );
  }, []);

  const onEvent = useCallback((key: number, event: TerminalEventDto) => {
    if (event.type === "exited") {
      setTabs((current) => current.map((tab) => (tab.key === key ? { ...tab, status: "exited" } : tab)));
    }
    if (event.type === "error") {
      setTabs((current) =>
        current.map((tab) => (tab.key === key ? { ...tab, status: "error", error: event.message } : tab))
      );
    }
  }, []);

  const activeTab = tabs[activeIndex] ?? null;
  const projectRoot = snapshot.projectRoot;

  const resizeStyle = useMemo(() => ({ width: `${width}px` }), [width]);

  if (projectRoot === null) return null;

  if (!layout.visible) {
    return (
      <button
        className="terminal-rail-button"
        title="Terminal"
        onClick={() => {
          setVisible(true);
        }}
      >
        <TerminalSquare size={16} />
      </button>
    );
  }

  return (
    <aside className="terminal-panel" style={resizeStyle}>
      <div
        className="terminal-resizer"
        onPointerDown={(event) => {
          beginPanelResize(event, width, setWidth, persist);
        }}
      />
      <div className="terminal-tabs">
        {tabs.map((tab, index) => (
          <button
            key={tab.key}
            className={`terminal-tab ${index === activeIndex ? "active" : ""}`}
            onClick={() => {
              setActiveIndex(index);
              void persist({ activeTabIndex: index });
            }}
          >
            <span>{profileLabel(tab.profile)}</span>
            <span className={`terminal-tab-status ${tab.status}`} />
            <X
              size={13}
              onClick={(event) => {
                event.stopPropagation();
                closeTab(index);
              }}
            />
          </button>
        ))}
        <button
          className="terminal-icon-button"
          title="New PowerShell"
          onClick={() => {
            addTab("powerShell");
          }}
        >
          <Plus size={14} />
        </button>
        <button
          className="terminal-text-button"
          onClick={() => {
            addTab("cmd");
          }}
        >
          cmd
        </button>
        <button
          className="terminal-icon-button terminal-hide"
          title="Hide Terminal"
          onClick={() => {
            setVisible(false);
          }}
        >
          <Power size={14} />
        </button>
      </div>
      {tabs.length === 0 ? (
        <div className="terminal-empty">
          <button
            onClick={() => {
              addTab("powerShell");
            }}
          >
            New PowerShell
          </button>
          <button
            onClick={() => {
              addTab("cmd");
            }}
          >
            New cmd
          </button>
        </div>
      ) : (
        <div className="terminal-stack">
          {tabs.map((tab) => (
            <TerminalView
              key={tab.key}
              tab={tab}
              active={tab.key === activeTab?.key}
              projectRoot={projectRoot}
              onStarted={onStarted}
              onEvent={onEvent}
            />
          ))}
          {activeTab?.error !== null && activeTab?.error !== undefined && activeTab.error !== "" && (
            <div className="terminal-error">{activeTab.error}</div>
          )}
        </div>
      )}
    </aside>
  );
}

function TerminalView({ tab, active, projectRoot, onStarted, onEvent }: TerminalViewProps) {
  const hostRef = useRef<HTMLDivElement | null>(null);
  const terminalRef = useRef<Terminal | null>(null);
  const fitRef = useRef<FitAddon | null>(null);
  const sessionIdRef = useRef<number | null>(null);

  useEffect(() => {
    const terminal = new Terminal({
      cursorBlink: true,
      scrollback: 5000,
      fontFamily: "Cascadia Mono, Consolas, monospace",
      fontSize: 12,
      theme: {
        background: "#111214",
        foreground: "#ebe7df",
        cursor: "#fffaf0",
        selectionBackground: "#31543f",
        black: "#17181b",
        red: "#df6b6b",
        green: "#6abf8a",
        yellow: "#f0c46b",
        blue: "#6b9fdf",
        magenta: "#c783d9",
        cyan: "#6bc7c7",
        white: "#ebe7df",
        brightBlack: "#626873",
        brightRed: "#ff8585",
        brightGreen: "#8ee0aa",
        brightYellow: "#ffd98c",
        brightBlue: "#8bbcff",
        brightMagenta: "#dfa2ef",
        brightCyan: "#8ee8e8",
        brightWhite: "#fffaf0"
      }
    });
    const fit = new FitAddon();
    const channel = new Channel<TerminalEventDto>();
    terminal.loadAddon(fit);
    terminalRef.current = terminal;
    fitRef.current = fit;

    channel.onmessage = (event) => {
      if (event.type === "output") {
        terminal.write(event.data);
      } else {
        onEvent(tab.key, event);
      }
    };

    const inputDisposable = terminal.onData((data) => {
      const sessionId = sessionIdRef.current;
      if (sessionId !== null) {
        void commands.writeTerminalInput(sessionId, data);
      }
    });

    const host = hostRef.current;
    if (host) {
      terminal.open(host);
      fit.fit();
      const dimensions = fit.proposeDimensions();
      void commands
        .createTerminalSession(tab.profile, dimensions?.cols ?? DEFAULT_COLS, dimensions?.rows ?? DEFAULT_ROWS, channel)
        .then((sessionId) => {
          sessionIdRef.current = sessionId;
          onStarted(tab.key, sessionId);
        })
        .catch((error: unknown) => {
          onEvent(tab.key, { type: "error", sessionId: 0, message: String(error) });
        });
    }

    const observer = new ResizeObserver(() => {
      fit.fit();
      const sessionId = sessionIdRef.current;
      const dimensions = fit.proposeDimensions();
      if (sessionId !== null && dimensions) {
        void commands.resizeTerminalSession(sessionId, dimensions.cols, dimensions.rows);
      }
    });
    if (host) observer.observe(host);

    return () => {
      observer.disconnect();
      inputDisposable.dispose();
      const sessionId = sessionIdRef.current;
      if (sessionId !== null) {
        void commands.killTerminalSession(sessionId);
      }
      terminal.dispose();
    };
  }, [onEvent, onStarted, projectRoot, tab.key, tab.profile]);

  useEffect(() => {
    if (!active) return;
    fitRef.current?.fit();
    terminalRef.current?.focus();
  }, [active]);

  return <div ref={hostRef} className={`terminal-host ${active ? "active" : ""}`} />;
}

function beginPanelResize(
  event: React.PointerEvent<HTMLDivElement>,
  initialWidth: number,
  setWidth: (width: number) => void,
  persist: (next: Partial<TerminalPanelLayoutDto>) => Promise<void>
) {
  event.currentTarget.setPointerCapture(event.pointerId);
  const startX = event.clientX;
  const onMove = (moveEvent: PointerEvent) => {
    setWidth(clampWidth(initialWidth - (moveEvent.clientX - startX)));
  };
  const onUp = (upEvent: PointerEvent) => {
    const nextWidth = clampWidth(initialWidth - (upEvent.clientX - startX));
    setWidth(nextWidth);
    void persist({ widthPx: nextWidth });
    window.removeEventListener("pointermove", onMove);
    window.removeEventListener("pointerup", onUp);
  };
  window.addEventListener("pointermove", onMove);
  window.addEventListener("pointerup", onUp);
}

function profileLabel(profile: TerminalProfileDto) {
  return profile === "powerShell" ? "PowerShell" : "cmd";
}

function restoredProfiles(layout: TerminalPanelLayoutDto) {
  return layout.tabProfiles.length > 0 ? layout.tabProfiles : (["powerShell"] satisfies TerminalProfileDto[]);
}

function makeTabs(profiles: TerminalProfileDto[]): TerminalTab[] {
  return profiles.map((profile) => ({
    key: nextTerminalKey++,
    profile,
    sessionId: null,
    status: "starting",
    error: null
  }));
}

function clampWidth(width: number) {
  return Math.min(MAX_PANEL_WIDTH, Math.max(MIN_PANEL_WIDTH, width));
}

function clampActiveIndex(index: number, tabCount: number) {
  if (tabCount === 0) return 0;
  return Math.max(0, Math.min(tabCount - 1, Math.trunc(index)));
}
