import { commands as generatedCommands } from "./generated/bindings";
import type {
  FixtureGuiEdit,
  GuiDocumentRequest,
  GuiEditResult,
  LayoutGuiEdit,
  SequenceGuiEdit,
  SequenceSelectionEdit,
  SequenceSelectionEditResult
} from "./types";

let currentGuiRequest: GuiDocumentRequest | null = null;
let guiEditResultHandler: ((result: GuiEditResult) => void) | null = null;

export function setCurrentGuiRequest(request: GuiDocumentRequest | null): void {
  currentGuiRequest = request;
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
  applyGuiEdit: async (request: GuiDocumentRequest, edit: Parameters<typeof generatedCommands.applyGuiEdit>[1]) => {
    const result = await generatedCommands.applyGuiEdit(request, edit);
    guiEditResultHandler?.(result);
    if (result.document.type === "blocked") {
      throw new Error(result.document.reason);
    }
    return result;
  },
  applySequenceGuiEdit: (edit: SequenceGuiEdit) => applyCurrentGuiEdit({ type: "sequence", edit }),
  applySequenceSelectionEdit: async (edit: SequenceSelectionEdit) => {
    const result = await generatedCommands.applySequenceSelectionEdit(edit);
    const guiDiagnostic = result.snapshot.diagnostics.find((diagnostic) => diagnostic.code.startsWith("gui."));
    if (guiDiagnostic !== undefined) {
      throw new Error(guiDiagnostic.message);
    }
    guiEditResultHandler?.(guiEditResultFromSelection(result));
    return result;
  },
  applyLayoutGuiEdit: (edit: LayoutGuiEdit) => applyCurrentGuiEdit({ type: "layout", edit }),
  applyFixtureGuiEdit: (edit: FixtureGuiEdit) => applyCurrentGuiEdit({ type: "fixture", edit }),
  chooseSequenceAudio: () => {
    if (currentGuiRequest === null) {
      throw new Error("Audio selection attempted without an active GUI document request.");
    }
    return generatedCommands.chooseSequenceAudio(currentGuiRequest);
  },
  clearSequenceAudio: () => {
    if (currentGuiRequest === null) {
      throw new Error("Audio clearing attempted without an active GUI document request.");
    }
    return generatedCommands.clearSequenceAudio(currentGuiRequest);
  }
};

export function guiEditResultFromSelection(result: SequenceSelectionEditResult): GuiEditResult {
  return {
    snapshot: result.snapshot,
    document: result.document
  };
}
