import { getCurrentWindow } from "@tauri-apps/api/window";
import {
  Check,
  Circle,
  FolderOpen,
  LayoutDashboard,
  Pause,
  Play,
  RefreshCw,
  SearchCheck,
  Settings,
  X
} from "lucide-react";
import React, { useCallback, useEffect, useLayoutEffect, useRef, useState } from "react";
import { createPortal } from "react-dom";
import { useWorkbench } from "./store/workbenchStore";
import { DockLayout } from "./workbench/DockLayout";
import { resetWorkbenchLayout, toggleWorkbenchPanel } from "./workbench/dockBridge";
import type { PanelId } from "./workbench/panelIds";
import { registeredPanels } from "./workbench/registry";

export function WorkbenchShell() {
  const [menuOpen, setMenuOpen] = useState<string | null>(null);
  const [maximized, setMaximized] = useState(false);
  const menuRef = useRef<HTMLElement | null>(null);
  const playing = useWorkbench((state) => state.playing);
  const status = useWorkbench((state) => state.status);
  const projectState = useWorkbench((state) => state.projectState);
  const activeFile = useWorkbench((state) => state.activeFile);
  const panelVisibility = useWorkbench((state) => state.panelVisibility);
  const openProjectDialog = useWorkbench((state) => state.openProjectDialog);
  const closeProject = useWorkbench((state) => state.closeProject);
  const reloadProjectFromDisk = useWorkbench((state) => state.reloadProjectFromDisk);
  const runCheck = useWorkbench((state) => state.runCheck);
  const setStatus = useWorkbench((state) => state.setStatus);
  const togglePlayback = useWorkbench((state) => state.togglePlayback);

  useEffect(() => {
    const appWindow = getCurrentWindow();
    void appWindow.isMaximized().then(setMaximized).catch(() => setMaximized(false));
    const unlisten = appWindow.onResized(() => {
      void appWindow.isMaximized().then(setMaximized).catch(() => setMaximized(false));
    });
    return () => {
      void unlisten.then((dispose) => dispose());
    };
  }, []);

  useEffect(() => {
    function closeMenusOnOutsideClick(event: MouseEvent) {
      const target = event.target as HTMLElement;
      if (!menuRef.current?.contains(target) && !target.closest(".menu-popover")) {
        setMenuOpen(null);
      }
    }

    document.addEventListener("mousedown", closeMenusOnOutsideClick);
    return () => document.removeEventListener("mousedown", closeMenusOnOutsideClick);
  }, []);

  const dragWindow = useCallback(
    (event: React.MouseEvent) => {
      if (event.button !== 0) return;
      if ((event.target as HTMLElement).closest("button, input, a, select, textarea, .menu-popover")) return;
      event.preventDefault();
      void getCurrentWindow().startDragging().catch((error) => setStatus(formatError(error)));
    },
    [setStatus]
  );

  const toggleWindowMaximize = useCallback(
    (event: React.MouseEvent) => {
      if ((event.target as HTMLElement).closest("button, input, a, select, textarea, .menu-popover")) return;
      void getCurrentWindow().toggleMaximize().catch((error) => setStatus(formatError(error)));
    },
    [setStatus]
  );

  async function minimizeWindow() {
    try {
      await getCurrentWindow().minimize();
    } catch (error) {
      setStatus(formatError(error));
    }
  }

  async function toggleMaximizeWindow() {
    try {
      await getCurrentWindow().toggleMaximize();
    } catch (error) {
      setStatus(formatError(error));
    }
  }

  async function closeWindow() {
    try {
      await getCurrentWindow().close();
    } catch (error) {
      setStatus(formatError(error));
    }
  }

  function togglePanel(panelId: PanelId) {
    toggleWorkbenchPanel(panelId);
    setMenuOpen(null);
  }

  return (
    <main className="shell">
      <header className="topbar" onMouseDown={dragWindow} onDoubleClick={toggleWindowMaximize}>
        <div className="brand">
          <span className="logo-mark">D</span>
          <span>Dawn</span>
        </div>
        <nav
          className="menubar"
          ref={menuRef}
          onClick={(event) => {
            if ((event.target as HTMLElement).closest(".menu-item")) {
              setMenuOpen(null);
            }
          }}
        >
          <MenuButton label="File" open={menuOpen === "File"} onOpen={() => setMenuOpen(menuOpen === "File" ? null : "File")}>
            <MenuItem icon={<FolderOpen size={15} />} label="Open Project..." onClick={openProjectDialog} />
            <MenuItem icon={<X size={15} />} label="Close Project" onClick={closeProject} disabled={!projectState} />
            <MenuSeparator />
            <MenuItem icon={<RefreshCw size={15} />} label="Reload Project from Disk" onClick={reloadProjectFromDisk} disabled={!projectState} />
            <MenuSeparator />
            <MenuItem icon={<SearchCheck size={15} />} label="Check Project" onClick={runCheck} disabled={!projectState} />
            <MenuSeparator />
            <MenuItem icon={<Settings size={15} />} label="Preferences" onClick={() => setStatus("Preferences are not implemented yet.")} />
          </MenuButton>
          <MenuButton label="Edit" open={menuOpen === "Edit"} onOpen={() => setMenuOpen(menuOpen === "Edit" ? null : "Edit")}>
            <MenuItem label="Format Document" onClick={() => setStatus("Format document is not wired yet.")} />
            <MenuItem label="Rename Symbol" onClick={() => setStatus("Rename symbol is not wired yet.")} />
          </MenuButton>
          <MenuButton label="View" open={menuOpen === "View"} onOpen={() => setMenuOpen(menuOpen === "View" ? null : "View")}>
            <MenuItem icon={<LayoutDashboard size={15} />} label="Reset Layout" onClick={resetWorkbenchLayout} />
            <MenuSeparator />
            {registeredPanels.map((panel) => (
              <PanelMenuItem key={panel.id} panelId={panel.id} label={panel.title} visible={panelVisibility[panel.id]} onClick={togglePanel} />
            ))}
          </MenuButton>
          <MenuButton label="Help" open={menuOpen === "Help"} onOpen={() => setMenuOpen(menuOpen === "Help" ? null : "Help")}>
            <MenuItem label="About Dawn" onClick={() => setStatus("Dawn 0.1.0 IDE prototype")} />
          </MenuButton>
        </nav>
        <div className="topbar-actions">
          <button className="run-button" title={playing ? "Pause" : "Play"} onClick={togglePlayback}>
            {playing ? <Pause size={18} /> : <Play size={18} />}
          </button>
          <div className="window-controls">
            <button title="Minimize" onClick={() => void minimizeWindow()}>
              <MinimizeIcon />
            </button>
            <button title={maximized ? "Restore" : "Maximize"} onClick={() => void toggleMaximizeWindow()}>
              {maximized ? <RestoreIcon /> : <MaximizeIcon />}
            </button>
            <button className="close-button" title="Close" onClick={() => void closeWindow()}>
              <CloseIcon />
            </button>
          </div>
        </div>
      </header>
      <DockLayout />
      <footer className="statusbar">
        <span>{status}</span>
        <span>{projectState ? `${projectState.files.length} files` : "No project"}</span>
      </footer>
    </main>
  );
}

function PanelMenuItem({
  panelId,
  label,
  visible,
  onClick
}: {
  panelId: PanelId;
  label: string;
  visible: boolean;
  onClick: (panelId: PanelId) => void;
}) {
  return <MenuItem icon={visible ? <Check size={15} /> : <Circle size={10} />} label={label} onClick={() => onClick(panelId)} />;
}

function MenuButton({
  label,
  open,
  onOpen,
  children
}: {
  label: string;
  open: boolean;
  onOpen: () => void;
  children: React.ReactNode;
}) {
  const buttonRef = useRef<HTMLButtonElement | null>(null);
  const [popoverPosition, setPopoverPosition] = useState({ top: 0, left: 0 });

  useLayoutEffect(() => {
    if (!open) return;

    function updatePosition() {
      const rect = buttonRef.current?.getBoundingClientRect();
      if (!rect) return;

      const width = 220;
      setPopoverPosition({
        top: rect.bottom + 2,
        left: Math.max(4, Math.min(rect.left, window.innerWidth - width - 4))
      });
    }

    updatePosition();
    window.addEventListener("resize", updatePosition);
    window.addEventListener("scroll", updatePosition, true);
    return () => {
      window.removeEventListener("resize", updatePosition);
      window.removeEventListener("scroll", updatePosition, true);
    };
  }, [open]);

  return (
    <div className="menu-root">
      <button ref={buttonRef} className={open ? "menu-button active" : "menu-button"} onClick={onOpen}>
        <span>{label}</span>
      </button>
      {open
        ? createPortal(
            <div className="menu-popover" style={{ top: popoverPosition.top, left: popoverPosition.left }}>
              {children}
            </div>,
            document.body
          )
        : null}
    </div>
  );
}

function MenuItem({
  icon,
  label,
  onClick,
  disabled = false
}: {
  icon?: React.ReactNode;
  label: string;
  onClick: () => void | Promise<void>;
  disabled?: boolean;
}) {
  return (
    <button
      className="menu-item"
      disabled={disabled}
      onClick={() => {
        void onClick();
      }}
    >
      <span className="menu-icon">{icon}</span>
      <span>{label}</span>
    </button>
  );
}

function MenuSeparator() {
  return <div className="menu-separator" role="separator" />;
}

function formatError(error: unknown) {
  return error instanceof Error ? error.message : String(error);
}

function MinimizeIcon() {
  return (
    <svg width="10" height="10" viewBox="0 0 10 10" aria-hidden="true">
      <path d="M1 5.5h8" stroke="currentColor" strokeWidth="1.2" strokeLinecap="square" />
    </svg>
  );
}

function MaximizeIcon() {
  return (
    <svg width="10" height="10" viewBox="0 0 10 10" aria-hidden="true">
      <rect x="1.5" y="1.5" width="7" height="7" fill="none" stroke="currentColor" strokeWidth="1.1" />
    </svg>
  );
}

function RestoreIcon() {
  return (
    <svg width="10" height="10" viewBox="0 0 10 10" aria-hidden="true">
      <path d="M3 1.5h5.5v5.5H7.2V2.8H3V1.5Z" fill="currentColor" />
      <rect x="1.5" y="3" width="5.5" height="5.5" fill="none" stroke="currentColor" strokeWidth="1.1" />
    </svg>
  );
}

function CloseIcon() {
  return (
    <svg width="10" height="10" viewBox="0 0 10 10" aria-hidden="true">
      <path d="M1.5 1.5 8.5 8.5M8.5 1.5 1.5 8.5" stroke="currentColor" strokeWidth="1.2" strokeLinecap="square" />
    </svg>
  );
}
