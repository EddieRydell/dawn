import { commands as generatedCommands } from "./bindings";
import type { AppCommandDto, AppCommandResponseDto, SequenceEffectPreviewRequestEffectDto } from "./bindings";

type GeneratedResult<T> = Promise<{ status: "ok"; data: T } | { status: "error"; error: string }>;

async function unwrap<T>(result: GeneratedResult<T>): Promise<T> {
  const resolved = await result;
  if (resolved.status === "error") {
    throw new Error(resolved.error);
  }
  return resolved.data;
}

async function dispatch(command: AppCommandDto): Promise<AppCommandResponseDto> {
  return unwrap(generatedCommands.dispatchAppCommand(command));
}

async function dispatchNone(command: AppCommandDto): Promise<void> {
  await dispatch(command);
}

async function dispatchOptionalString(command: AppCommandDto): Promise<string | null> {
  const response = await dispatch(command);
  if (response.type !== "optionalString") {
    throw new Error(`unexpected app command response: ${response.type}`);
  }
  return response.value;
}

export const commands = {
  getAppSnapshot: () => unwrap(generatedCommands.getAppSnapshot()),
  openProjectDialog: () => dispatchNone({ type: "openProjectDialog" }),
  openProject: (path: string) => dispatchNone({ type: "openProject", path }),
  chooseNewProjectParentDirectory: () => dispatchOptionalString({ type: "chooseNewProjectParentDirectory" }),
  createNewProject: (parentPath: string, directoryName: string) =>
    dispatchNone({ type: "createNewProject", parentPath, directoryName }),
  openFile: (path: string) => dispatchNone({ type: "openFile", path }),
  closeFile: (path: string) => dispatchNone({ type: "closeFile", path }),
  setActiveFile: (path: string) => dispatchNone({ type: "setActiveFile", path }),
  updateActiveText: (text: string) => dispatchNone({ type: "updateActiveText", text }),
  setActiveViewMode: (mode: "text" | "gui") => dispatchNone({ type: "setActiveViewMode", mode }),
  undoActiveEdit: () => dispatchNone({ type: "undoActiveEdit" }),
  redoActiveEdit: () => dispatchNone({ type: "redoActiveEdit" }),
  applySequenceGuiEdit: (edit: Extract<AppCommandDto, { type: "applySequenceGuiEdit" }>["edit"]) =>
    dispatchNone({ type: "applySequenceGuiEdit", edit }),
  applySequenceSelectionEdit: async (edit: Extract<AppCommandDto, { type: "applySequenceSelectionEdit" }>["edit"]) => {
    const response = await dispatch({ type: "applySequenceSelectionEdit", edit });
    if (response.type !== "sequenceSelectionEditResult") {
      throw new Error(`unexpected app command response: ${response.type}`);
    }
    return response.result;
  },
  chooseSequenceAudio: () => dispatchNone({ type: "chooseSequenceAudio" }),
  clearSequenceAudio: () => dispatchNone({ type: "clearSequenceAudio" }),
  exportActiveSequenceFseq: (stepMs: number) => dispatchNone({ type: "exportActiveSequenceFseq", stepMs }),
  requestSequenceEffectPreviews: (
    path: string,
    objectKey: string,
    requestId: number,
    effects: SequenceEffectPreviewRequestEffectDto[]
  ) => unwrap(generatedCommands.requestSequenceEffectPreviews(path, objectKey, requestId, effects)),
  takeSequenceEffectPreviewResults: (path: string, objectKey: string) =>
    unwrap(generatedCommands.takeSequenceEffectPreviewResults(path, objectKey)),
  applyLayoutGuiEdit: (edit: Extract<AppCommandDto, { type: "applyLayoutGuiEdit" }>["edit"]) =>
    dispatchNone({ type: "applyLayoutGuiEdit", edit }),
  applyFixtureGuiEdit: (edit: Extract<AppCommandDto, { type: "applyFixtureGuiEdit" }>["edit"]) =>
    dispatchNone({ type: "applyFixtureGuiEdit", edit }),
  flushAutosave: () => dispatchNone({ type: "flushAutosave" }),
  reloadActiveBufferFromDisk: () => dispatchNone({ type: "reloadActiveBufferFromDisk" }),
  keepActiveBuffer: () => dispatchNone({ type: "keepActiveBuffer" }),
  createFile: (parent: string, name: string) => dispatchNone({ type: "createFile", parent, name }),
  createDirectory: (parent: string, name: string) => dispatchNone({ type: "createDirectory", parent, name }),
  renamePath: (path: string, newName: string) => dispatchNone({ type: "renamePath", path, newName }),
  deletePath: (path: string) => dispatchNone({ type: "deletePath", path }),
  reloadProject: () => dispatchNone({ type: "reloadProject" }),
  toggleProjectTree: () => dispatchNone({ type: "toggleProjectTree" }),
  setEffectPreviewEnabled: (enabled: boolean) => dispatchNone({ type: "setEffectPreviewEnabled", enabled }),
  setEffectPreviewEffects: (ids: number[]) => dispatchNone({ type: "setEffectPreviewEffects", ids }),
  openPreviewWindow: () => dispatchNone({ type: "openPreviewWindow" }),
  previewPlay: () => dispatchNone({ type: "previewPlay" }),
  previewPause: () => dispatchNone({ type: "previewPause" }),
  previewStop: () => dispatchNone({ type: "previewStop" }),
  previewRewindToZero: () => dispatchNone({ type: "previewRewindToZero" }),
  previewSeek: (positionSeconds: number) => dispatchNone({ type: "previewSeek", positionSeconds }),
  setLiveOutputEnabled: (enabled: boolean) => dispatchNone({ type: "setLiveOutputEnabled", enabled }),
  getPreviewScene: () => unwrap(generatedCommands.getPreviewScene())
};
