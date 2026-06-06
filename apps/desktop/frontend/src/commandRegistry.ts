import { commands } from "./api";
import { runSnapshotCommand, useAppStore } from "./store";

export type CommandId =
  | "file.newProject"
  | "file.openProject"
  | "file.save"
  | "file.exportFseq"
  | "edit.undo"
  | "edit.redo"
  | "view.toggleProjectTree"
  | "view.toggleTerminal"
  | "view.openPreviewWindow"
  | "project.reload";

export type CommandDefinition = {
  id: CommandId;
  label: string;
  shortcut?: string;
  run: () => Promise<void>;
};

export const commandRegistry: Record<CommandId, CommandDefinition> = {
  "file.newProject": {
    id: "file.newProject",
    label: "New Project...",
    run: () => {
      window.dispatchEvent(new CustomEvent("dawn:new-project"));
      return Promise.resolve();
    }
  },
  "file.openProject": {
    id: "file.openProject",
    label: "Open Project",
    shortcut: "Ctrl+O",
    run: async () => {
      await runSnapshotCommand(commands.openProjectDialog);
    }
  },
  "file.save": {
    id: "file.save",
    label: "Save",
    shortcut: "Ctrl+S",
    run: async () => {
      await runSnapshotCommand(commands.flushAutosave);
    }
  },
  "file.exportFseq": {
    id: "file.exportFseq",
    label: "Export FSEQ...",
    run: () => {
      window.dispatchEvent(new CustomEvent("dawn:export-fseq"));
      return Promise.resolve();
    }
  },
  "edit.undo": {
    id: "edit.undo",
    label: "Undo",
    shortcut: "Ctrl+Z",
    run: async () => {
      const text = useAppStore.getState().localText;
      await runSnapshotCommand(commands.updateActiveText.bind(null, text));
      await runSnapshotCommand(commands.undoActiveEdit);
    }
  },
  "edit.redo": {
    id: "edit.redo",
    label: "Redo",
    shortcut: "Ctrl+Shift+Z",
    run: async () => {
      await runSnapshotCommand(commands.redoActiveEdit);
    }
  },
  "view.toggleProjectTree": {
    id: "view.toggleProjectTree",
    label: "Project Tree",
    shortcut: "Ctrl+B",
    run: async () => {
      await runSnapshotCommand(commands.toggleProjectTree);
    }
  },
  "view.toggleTerminal": {
    id: "view.toggleTerminal",
    label: "Terminal",
    shortcut: "Ctrl+`",
    run: async () => {
      const snapshot = useAppStore.getState().snapshot;
      if (!snapshot || snapshot.projectRoot === null) return;
      const layout = snapshot.terminalPanelLayout;
      const updated = await commands.setTerminalPanelLayout({
        ...layout,
        visible: !layout.visible,
        tabProfiles: layout.tabProfiles.length > 0 ? layout.tabProfiles : ["powerShell"]
      });
      useAppStore.getState().setSnapshot(updated);
      useAppStore.getState().setError(null);
      if (!layout.visible) {
        window.dispatchEvent(new CustomEvent("dawn:terminal-open"));
      }
    }
  },
  "view.openPreviewWindow": {
    id: "view.openPreviewWindow",
    label: "Preview Window",
    run: async () => {
      await commands.openPreviewWindow();
      useAppStore.getState().setError(null);
    }
  },
  "project.reload": {
    id: "project.reload",
    label: "Reload / Check",
    shortcut: "Ctrl+R",
    run: async () => {
      await runSnapshotCommand(commands.reloadProject);
    }
  }
};

export function installGlobalShortcuts() {
  const onKeyDown = (event: KeyboardEvent) => {
    if (event.target instanceof Element && event.target.closest(".terminal-panel")) return;
    const ctrl = event.ctrlKey || event.metaKey;
    if (!ctrl) return;
    const active = useAppStore.getState().snapshot;
    if (!active) return;
    const key = event.key.toLowerCase();
    if (key === "z") {
      event.preventDefault();
      void (event.shiftKey ? commandRegistry["edit.redo"] : commandRegistry["edit.undo"]).run();
      return;
    }
    const command =
      key === "o"
        ? commandRegistry["file.openProject"]
        : key === "s"
          ? commandRegistry["file.save"]
          : key === "b"
            ? commandRegistry["view.toggleProjectTree"]
            : key === "`"
              ? commandRegistry["view.toggleTerminal"]
              : key === "r"
                ? commandRegistry["project.reload"]
                : null;
    if (command) {
      event.preventDefault();
      void command.run();
    }
  };
  window.addEventListener("keydown", onKeyDown);
  return () => {
    window.removeEventListener("keydown", onKeyDown);
  };
}
