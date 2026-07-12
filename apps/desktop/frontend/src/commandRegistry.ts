import { commands, getCurrentGuiRequest } from "./api";
import { effectiveEditorViewMode } from "./editorViewMode";
import { runSnapshotCommand, useAppStore } from "./store";

export type CommandId =
  | "file.newProject"
  | "file.newSequence"
  | "file.openProject"
  | "file.save"
  | "file.reloadFromDisk"
  | "file.settings"
  | "edit.undo"
  | "edit.redo"
  | "view.toggleGuiMode"
  | "view.toggleProjectTree"
  | "project.reload";

export type CommandDefinition = {
  label: string;
  shortcut?: string;
  run: () => Promise<void>;
};

export const commandRegistry: Record<CommandId, CommandDefinition> = {
  "file.newProject": {
    label: "New Project...",
    run: () => {
      window.dispatchEvent(new CustomEvent("dawn:new-project"));
      return Promise.resolve();
    }
  },
  "file.newSequence": {
    label: "New Sequence...",
    run: () => {
      window.dispatchEvent(new CustomEvent("dawn:new-sequence"));
      return Promise.resolve();
    }
  },
  "file.openProject": {
    label: "Open Project...",
    shortcut: "Ctrl+O",
    run: async () => {
      await runSnapshotCommand(commands.openProjectDialog);
    }
  },
  "file.save": {
    label: "Save",
    shortcut: "Ctrl+S",
    run: async () => {
      const store = useAppStore.getState();
      if (store.snapshot === null || store.snapshot.activeBuffer === null || effectiveEditorViewMode(store.snapshot) !== "text") return;
      const text = store.localText;
      await runSnapshotCommand(commands.updateActiveText.bind(null, text));
      await runSnapshotCommand(commands.flushAutosave);
    }
  },
  "file.reloadFromDisk": {
    label: "Reload From Disk",
    run: async () => {
      await runSnapshotCommand(commands.reloadActiveBufferFromDisk);
      await refreshActiveGuiDocument();
    }
  },
  "file.settings": {
    label: "Settings...",
    run: () => {
      window.dispatchEvent(new CustomEvent("dawn:settings"));
      return Promise.resolve();
    }
  },
  "edit.undo": {
    label: "Undo",
    shortcut: "Ctrl+Z",
    run: async () => {
      if (effectiveEditorViewMode(useAppStore.getState().snapshot) !== "gui") return;
      await runSnapshotCommand(commands.undoActiveEdit);
      await refreshActiveGuiDocument();
    }
  },
  "edit.redo": {
    label: "Redo",
    shortcut: "Ctrl+Shift+Z / Ctrl+Y",
    run: async () => {
      if (effectiveEditorViewMode(useAppStore.getState().snapshot) !== "gui") return;
      await runSnapshotCommand(commands.redoActiveEdit);
      await refreshActiveGuiDocument();
    }
  },
  "view.toggleGuiMode": {
    label: "GUI Mode",
    run: async () => {
      const mode = (useAppStore.getState().snapshot?.settings.editorViewMode ?? "gui") === "gui" ? "text" : "gui";
      await runSnapshotCommand(() => commands.setEditorViewMode(mode));
    }
  },
  "view.toggleProjectTree": {
    label: "Project Tree",
    shortcut: "Ctrl+B",
    run: async () => {
      await runSnapshotCommand(commands.toggleProjectTree);
    }
  },
  "project.reload": {
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
      if (effectiveEditorViewMode(active) !== "gui") return;
      event.preventDefault();
      void (event.shiftKey ? commandRegistry["edit.redo"] : commandRegistry["edit.undo"]).run();
      return;
    }
    if (key === "y") {
      if (effectiveEditorViewMode(active) !== "gui") return;
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
