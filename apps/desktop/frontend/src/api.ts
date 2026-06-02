import { invoke } from "@tauri-apps/api/core";
import { commands as generatedCommands } from "./bindings";
import type { AppSnapshotDto } from "./bindings";

type GeneratedResult<T> = Promise<{ status: "ok"; data: T } | { status: "error"; error: string }>;

async function unwrap<T>(result: GeneratedResult<T>): Promise<T> {
  const resolved = await result;
  if (resolved.status === "error") {
    throw new Error(resolved.error);
  }
  return resolved.data;
}

async function rawInvoke<T>(command: string): Promise<T> {
  return await invoke<T>(command);
}

export const commands = {
  getSnapshot: () => unwrap(generatedCommands.getSnapshot()),
  openProjectDialog: () => unwrap(generatedCommands.openProjectDialog()),
  openProject: (path: string) => unwrap(generatedCommands.openProject(path)),
  chooseNewProjectParentDirectory: () => unwrap(generatedCommands.chooseNewProjectParentDirectory()),
  createNewProject: (parentPath: string, directoryName: string) =>
    unwrap(generatedCommands.createNewProject(parentPath, directoryName)),
  openFile: (path: string) => unwrap(generatedCommands.openFile(path)),
  closeFile: (path: string) => unwrap(generatedCommands.closeFile(path)),
  setActiveFile: (path: string) => unwrap(generatedCommands.setActiveFile(path)),
  updateActiveText: (text: string) => unwrap(generatedCommands.updateActiveText(text)),
  setActiveViewMode: (mode: "text" | "gui") => unwrap(generatedCommands.setActiveViewMode(mode)),
  undoActiveEdit: () => unwrap(generatedCommands.undoActiveEdit()),
  redoActiveEdit: () => unwrap(generatedCommands.redoActiveEdit()),
  applySequenceGuiEdit: (edit: Parameters<typeof generatedCommands.applySequenceGuiEdit>[0]) =>
    unwrap(generatedCommands.applySequenceGuiEdit(edit)),
  applySequenceSelectionEdit: (edit: Parameters<typeof generatedCommands.applySequenceSelectionEdit>[0]) =>
    unwrap(generatedCommands.applySequenceSelectionEdit(edit)),
  chooseSequenceAudio: () => unwrap(generatedCommands.chooseSequenceAudio()),
  clearSequenceAudio: () => unwrap(generatedCommands.clearSequenceAudio()),
  exportActiveSequenceFseq: (stepMs: number) => unwrap(generatedCommands.exportActiveSequenceFseq(stepMs)),
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
  reloadActiveBufferFromDisk: () => rawInvoke<AppSnapshotDto>("reload_active_buffer_from_disk"),
  keepActiveBuffer: () => rawInvoke<AppSnapshotDto>("keep_active_buffer"),
  createFile: (parent: string, name: string) => unwrap(generatedCommands.createFile(parent, name)),
  createDirectory: (parent: string, name: string) => unwrap(generatedCommands.createDirectory(parent, name)),
  renamePath: (path: string, newName: string) => unwrap(generatedCommands.renamePath(path, newName)),
  deletePath: (path: string) => unwrap(generatedCommands.deletePath(path)),
  reloadProject: () => unwrap(generatedCommands.reloadProject()),
  toggleProjectTree: () => unwrap(generatedCommands.toggleProjectTree()),
  setEffectPreviewEnabled: (enabled: boolean) => unwrap(generatedCommands.setEffectPreviewEnabled(enabled)),
  setEffectPreviewEffects: (ids: number[]) => unwrap(generatedCommands.setEffectPreviewEffects(ids)),
  openPreviewWindow: () => unwrap(generatedCommands.openPreviewWindow()),
  previewPlay: () => unwrap(generatedCommands.previewPlay()),
  previewPause: () => unwrap(generatedCommands.previewPause()),
  previewStop: () => unwrap(generatedCommands.previewStop()),
  previewRewindToZero: () => unwrap(generatedCommands.previewRewindToZero()),
  previewSeek: (positionSeconds: number) => unwrap(generatedCommands.previewSeek(positionSeconds)),
  setLiveOutputEnabled: (enabled: boolean) => unwrap(generatedCommands.setLiveOutputEnabled(enabled)),
  getPreviewScene: () => unwrap(generatedCommands.getPreviewScene())
};
