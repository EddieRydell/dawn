import { commands } from "./api";
import { runSnapshotCommand, useAppStore } from "./store";

export type CommandId =
  | "file.openProject"
  | "file.save"
  | "view.toggleProjectTree"
  | "project.reload";

export type CommandDefinition = {
  id: CommandId;
  label: string;
  shortcut?: string;
  run: () => Promise<void>;
};

export const commandRegistry: Record<CommandId, CommandDefinition> = {
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
  return () => window.removeEventListener("keydown", onKeyDown);
}
