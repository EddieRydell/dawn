import { commands as generatedCommands } from "./bindings";

type GeneratedResult<T> = Promise<{ status: "ok"; data: T } | { status: "error"; error: string }>;

async function unwrap<T>(result: GeneratedResult<T>): Promise<T> {
  const resolved = await result;
  if (resolved.status === "error") {
    throw new Error(resolved.error);
  }
  return resolved.data;
}

export const commands = {
  getSnapshot: () => unwrap(generatedCommands.getSnapshot()),
  openProjectDialog: () => unwrap(generatedCommands.openProjectDialog()),
  openProject: (path: string) => unwrap(generatedCommands.openProject(path)),
  openFile: (path: string) => unwrap(generatedCommands.openFile(path)),
  closeFile: (path: string) => unwrap(generatedCommands.closeFile(path)),
  setActiveFile: (path: string) => unwrap(generatedCommands.setActiveFile(path)),
  updateActiveText: (text: string) => unwrap(generatedCommands.updateActiveText(text)),
  setActiveViewMode: (mode: "text" | "gui") => unwrap(generatedCommands.setActiveViewMode(mode)),
  undoActiveEdit: () => unwrap(generatedCommands.undoActiveEdit()),
  redoActiveEdit: () => unwrap(generatedCommands.redoActiveEdit()),
  applySequenceGuiEdit: (edit: Parameters<typeof generatedCommands.applySequenceGuiEdit>[0]) =>
    unwrap(generatedCommands.applySequenceGuiEdit(edit)),
  getSequenceEffectPreviews: (
    path: string,
    objectKey: string,
    effectIds: number[]
  ) => unwrap(generatedCommands.getSequenceEffectPreviews(path, objectKey, effectIds)),
  applyLayoutGuiEdit: (edit: Parameters<typeof generatedCommands.applyLayoutGuiEdit>[0]) =>
    unwrap(generatedCommands.applyLayoutGuiEdit(edit)),
  applyFixtureGuiEdit: (edit: Parameters<typeof generatedCommands.applyFixtureGuiEdit>[0]) =>
    unwrap(generatedCommands.applyFixtureGuiEdit(edit)),
  flushAutosave: () => unwrap(generatedCommands.flushAutosave()),
  createFile: (parent: string, name: string) => unwrap(generatedCommands.createFile(parent, name)),
  createDirectory: (parent: string, name: string) => unwrap(generatedCommands.createDirectory(parent, name)),
  renamePath: (path: string, newName: string) => unwrap(generatedCommands.renamePath(path, newName)),
  deletePath: (path: string) => unwrap(generatedCommands.deletePath(path)),
  reloadProject: () => unwrap(generatedCommands.reloadProject()),
  toggleProjectTree: () => unwrap(generatedCommands.toggleProjectTree()),
  openPreviewWindow: () => unwrap(generatedCommands.openPreviewWindow()),
  previewPlay: () => unwrap(generatedCommands.previewPlay()),
  previewPause: () => unwrap(generatedCommands.previewPause()),
  previewStop: () => unwrap(generatedCommands.previewStop()),
  previewSeek: (positionMs: number) => unwrap(generatedCommands.previewSeek(positionMs)),
  getPreviewScene: () => unwrap(generatedCommands.getPreviewScene())
};
