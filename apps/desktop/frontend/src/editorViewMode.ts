import type { AppSnapshot, EditorViewMode } from "./types";

type EditorViewSnapshot = Pick<AppSnapshot, "settings" | "activeDocumentDescriptor">;

export function effectiveEditorViewMode(snapshot: EditorViewSnapshot | null): EditorViewMode {
  if (snapshot === null || (snapshot.settings.editorViewMode ?? "gui") === "text") {
    return "text";
  }
  return descriptorHasGuiView(snapshot.activeDocumentDescriptor) ? "gui" : "text";
}

function descriptorHasGuiView(descriptor: AppSnapshot["activeDocumentDescriptor"]): boolean {
  return descriptor?.availableViews.some((view) => view !== "text") ?? false;
}
