import { THEME_METRICS } from "./theme";

const pending = new Map<string, () => Promise<unknown>>();
let timer: number | undefined;
let saving: Promise<unknown> = Promise.resolve();

export function scheduleViewStateSave(key: string, save: () => Promise<unknown>, onError: (error: unknown) => void, immediate = false) {
  pending.set(key, save);
  window.clearTimeout(timer);
  if (immediate) {
    void flushViewStateSaves().catch(onError);
  } else {
    timer = window.setTimeout(() => {
      void flushViewStateSaves().catch(onError);
    }, THEME_METRICS.workspaceLayoutSaveDelay);
  }
}

export async function flushViewStateSaves(): Promise<void> {
  window.clearTimeout(timer);
  const saves = [...pending.entries()];
  pending.clear();
  const previous = saving;
  const job = (async () => {
    await previous;
    for (const [index, [, save]] of saves.entries()) {
      try {
        await save();
      } catch (error) {
        for (const [key, save] of saves.slice(index)) {
          if (!pending.has(key)) pending.set(key, save);
        }
        throw error;
      }
    }
  })();
  // Callers receive the failure; a later flush can retry the retained writes.
  saving = job.catch(() => undefined);
  await job;
}
