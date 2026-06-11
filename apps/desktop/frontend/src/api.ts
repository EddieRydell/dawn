import { commands as generatedCommands } from "./generated/bindings";
import type {
  FixtureGuiEdit,
  GuiDocumentRequest,
  GuiEditResult,
  LayoutGuiEdit,
  SequenceGuiEdit
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
  return result.snapshot;
}

export const commands = {
  ...generatedCommands,
  applyGuiEdit: async (request: GuiDocumentRequest, edit: Parameters<typeof generatedCommands.applyGuiEdit>[1]) => {
    const result = await generatedCommands.applyGuiEdit(request, edit);
    guiEditResultHandler?.(result);
    return result;
  },
  applySequenceGuiEdit: (edit: SequenceGuiEdit) => applyCurrentGuiEdit({ type: "sequence", edit }),
  applyLayoutGuiEdit: (edit: LayoutGuiEdit) => applyCurrentGuiEdit({ type: "layout", edit }),
  applyFixtureGuiEdit: (edit: FixtureGuiEdit) => applyCurrentGuiEdit({ type: "fixture", edit })
};
