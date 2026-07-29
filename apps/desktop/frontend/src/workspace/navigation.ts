import { commands } from "../api";
import { runSnapshotCommand } from "../store";
import type { TextRange } from "../types";

export const NAVIGATE_TO_TEXT_EVENT = "dawn:navigate-to-text";

export type TextNavigation = {
  path: string;
  range: TextRange | null;
};

export async function navigateToText(path: string, range: TextRange | null): Promise<void> {
  await runSnapshotCommand(() => commands.setEditorViewMode("text"));
  await runSnapshotCommand(() => commands.openFile(path));
  window.requestAnimationFrame(() => {
    window.dispatchEvent(
      new CustomEvent<TextNavigation>(NAVIGATE_TO_TEXT_EVENT, {
        detail: { path, range }
      })
    );
  });
}
