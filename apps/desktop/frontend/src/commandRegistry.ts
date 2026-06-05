import { commands } from "./api";
import type { AppCommandKindDto } from "./bindings";
import { runRuntimeCommand, useAppStore } from "./store";

export type CommandId =
  | "file.newProject"
  | "file.openProject"
  | "file.save"
  | "file.exportFseq"
  | "view.toggleProjectTree"
  | "view.openPreviewWindow"
  | "project.reload";

export type CommandDefinition = {
  id: CommandId;
  commandKind?: AppCommandKindDto;
  label: string;
  shortcut?: string;
  run: () => Promise<void>;
};

export const commandRegistry: Record<CommandId, CommandDefinition> = {
  "file.newProject": {
    id: "file.newProject",
    commandKind: "createNewProject",
    label: "New Project...",
    run: () => {
      window.dispatchEvent(new CustomEvent("dawn:new-project"));
      return Promise.resolve();
    }
  },
  "file.openProject": {
    id: "file.openProject",
    commandKind: "openProjectDialog",
    label: "Open Project",
    shortcut: "Ctrl+O",
    run: async () => {
      await runRuntimeCommand(commands.openProjectDialog);
    }
  },
  "file.save": {
    id: "file.save",
    commandKind: "flushAutosave",
    label: "Save",
    shortcut: "Ctrl+S",
    run: async () => {
      await runRuntimeCommand(commands.flushAutosave);
    }
  },
  "file.exportFseq": {
    id: "file.exportFseq",
    commandKind: "exportActiveSequenceFseq",
    label: "Export FSEQ...",
    run: () => {
      window.dispatchEvent(new CustomEvent("dawn:export-fseq"));
      return Promise.resolve();
    }
  },
  "view.toggleProjectTree": {
    id: "view.toggleProjectTree",
    commandKind: "toggleProjectTree",
    label: "Project Tree",
    shortcut: "Ctrl+B",
    run: async () => {
      await runRuntimeCommand(commands.toggleProjectTree);
    }
  },
  "view.openPreviewWindow": {
    id: "view.openPreviewWindow",
    commandKind: "openPreviewWindow",
    label: "Preview Window",
    run: async () => {
      await commands.openPreviewWindow();
      useAppStore.getState().setError(null);
    }
  },
  "project.reload": {
    id: "project.reload",
    commandKind: "reloadProject",
    label: "Reload / Check",
    shortcut: "Ctrl+R",
    run: async () => {
      await runRuntimeCommand(commands.reloadProject);
    }
  }
};

export function installGlobalShortcuts() {
  const onKeyDown = (event: KeyboardEvent) => {
    if (event.defaultPrevented) return;
    const ctrl = event.ctrlKey || event.metaKey;
    if (!ctrl) return;
    const active = useAppStore.getState().runtimeState;
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
    if (command && isCommandAvailable(command)) {
      event.preventDefault();
      void command.run();
    }
  };
  window.addEventListener("keydown", onKeyDown);
  return () => {
    window.removeEventListener("keydown", onKeyDown);
  };
}

export function isCommandAvailable(command: CommandDefinition) {
  if (command.commandKind === undefined) return true;
  const availability = useAppStore
    .getState()
    .runtimeState?.commandAvailability.find((entry) => entry.command === command.commandKind);
  return availability?.available ?? true;
}
