import { commands as generatedCommands } from "./generated/bindings";
import type {
  PropGuiEdit,
  GuiDocumentRequest,
  GuiEditResult,
  PreviewGuiEdit,
  SetupGuiEdit,
  SequenceGuiEdit,
  SequenceSelectionEdit
} from "./types";

let currentGuiRequest: GuiDocumentRequest | null = null;
let guiEditResultHandler: ((result: GuiEditResult) => void) | null = null;

export function setCurrentGuiRequest(request: GuiDocumentRequest | null): void {
  currentGuiRequest = request;
}

export function getCurrentGuiRequest(): GuiDocumentRequest | null {
  return currentGuiRequest;
}

export function setGuiEditResultHandler(handler: (result: GuiEditResult) => void): void {
  guiEditResultHandler = handler;
}

async function applyCurrentGuiEdit(edit: Parameters<typeof generatedCommands.applyGuiEdit>[1]) {
  if (currentGuiRequest === null) {
    throw new Error("GUI edit attempted without an active GUI document request.");
  }
  const result = await generatedCommands.applyGuiEdit(currentGuiRequest, edit);
  guiEditResultHandler?.(result);
  if (result.document.type === "blocked") {
    throw new Error(result.document.reason);
  }
  return result;
}

export const commands = {
  ...generatedCommands,
  autosaveActiveText: async (path: string, text: string) => {
    const result = await generatedCommands.autosaveActiveText(path, text);
    if (result.status === "error") {
      throw new Error(result.error);
    }
    return result.data;
  },
  applySequenceGuiEdit: (edit: SequenceGuiEdit) => applyCurrentGuiEdit({ type: "sequence", edit }),
  applySetupGuiEdit: (edit: SetupGuiEdit) => applyCurrentGuiEdit({ type: "setup", edit }),
  applySequenceSelectionEdit: async (edit: SequenceSelectionEdit) => {
    const result = await generatedCommands.applySequenceSelectionEdit(edit);
    const guiDiagnostic = result.snapshot.diagnostics.find((diagnostic) => diagnostic.code.startsWith("gui."));
    if (guiDiagnostic !== undefined) {
      throw new Error(guiDiagnostic.message);
    }
    guiEditResultHandler?.({ snapshot: result.snapshot, document: result.document });
    return result;
  },
  applyPreviewGuiEdit: (edit: PreviewGuiEdit) => applyCurrentGuiEdit({ type: "preview", edit }),
  applyPropGuiEdit: (edit: PropGuiEdit) => applyCurrentGuiEdit({ type: "prop", edit }),
  chooseSequenceAudio: () => {
    if (currentGuiRequest === null) {
      throw new Error("Audio selection attempted without an active GUI document request.");
    }
    return generatedCommands.chooseSequenceAudio(currentGuiRequest).then((result) => {
      guiEditResultHandler?.(result);
      if (result.document.type === "blocked") {
        throw new Error(result.document.reason);
      }
      return result;
    });
  }
};
