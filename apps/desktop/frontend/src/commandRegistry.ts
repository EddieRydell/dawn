import { commands, getCurrentGuiRequest } from "./api";
import { runSnapshotCommand, useAppStore } from "./store";

export type CommandId =
  | "file.newProject"
  | "file.openProject"
  | "file.save"
  | "file.exportFseq"
  | "file.settings"
  | "edit.undo"
  | "edit.redo"
  | "view.toggleProjectTree"
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
    label: "Open Project...",
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
      const store = useAppStore.getState();
      if (store.snapshot?.activeBuffer?.viewMode !== "text") return;
      const text = store.localText;
      await runSnapshotCommand(commands.updateActiveText.bind(null, text));
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
  "file.settings": {
    id: "file.settings",
    label: "Settings...",
    run: () => {
      window.dispatchEvent(new CustomEvent("dawn:settings"));
      return Promise.resolve();
    }
  },
  "edit.undo": {
    id: "edit.undo",
    label: "Undo",
    shortcut: "Ctrl+Z",
    run: async () => {
      if (useAppStore.getState().snapshot?.activeBuffer?.viewMode !== "gui") return;
      await runSnapshotCommand(commands.undoActiveEdit);
      await refreshActiveGuiDocument();
    }
  },
  "edit.redo": {
    id: "edit.redo",
    label: "Redo",
    shortcut: "Ctrl+Shift+Z / Ctrl+Y",
    run: async () => {
      if (useAppStore.getState().snapshot?.activeBuffer?.viewMode !== "gui") return;
      await runSnapshotCommand(commands.redoActiveEdit);
      await refreshActiveGuiDocument();
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
    const ctrl = event.ctrlKey || event.metaKey;
    if (!ctrl) return;
    const active = useAppStore.getState().snapshot;
    if (!active) return;
    const key = event.key.toLowerCase();
    if (key === "z") {
      if (active.activeBuffer?.viewMode !== "gui") return;
      event.preventDefault();
      void (event.shiftKey ? commandRegistry["edit.redo"] : commandRegistry["edit.undo"]).run();
      return;
    }
    if (key === "y") {
      if (active.activeBuffer?.viewMode !== "gui") return;
      event.preventDefault();
      void commandRegistry["edit.redo"].run();
      return;
    }
    const command =
      key === "o"
        ? commandRegistry["file.openProject"]
        : key === "s"
          ? commandRegistry["file.save"]
          : key === "b"
            ? commandRegistry["view.toggleProjectTree"]
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

async function refreshActiveGuiDocument() {
  const request = getCurrentGuiRequest();
  if (request === null) return;
  const document = await commands.getGuiDocument(request);
  const store = useAppStore.getState();
  store.setGuiDocument(document);
  store.resetGuiLocalState();
}
