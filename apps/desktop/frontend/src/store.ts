import { listen } from "@tauri-apps/api/event";
import { create } from "zustand";
import { commands } from "./api";
import type { RuntimeStateDto } from "./bindings";

type AppStore = {
  runtimeState: RuntimeStateDto | null;
  error: string | null;
  localText: string;
  setRuntimeState: (runtimeState: RuntimeStateDto) => void;
  setError: (error: string | null) => void;
  setLocalText: (text: string) => void;
  hydrate: () => Promise<void>;
};

export const useAppStore = create<AppStore>((set) => ({
  runtimeState: null,
  error: null,
  localText: "",
  setRuntimeState: (runtimeState) => {
    set({
      runtimeState,
      localText: runtimeState.activeBuffer?.text ?? ""
    });
  },
  setError: (error) => {
    set({ error });
  },
  setLocalText: (localText) => {
    set({ localText });
  },
  hydrate: async () => {
    const runtimeState = await commands.getRuntimeState();
    set({ runtimeState, localText: runtimeState.activeBuffer?.text ?? "", error: null });
  }
}));

export async function subscribeToRuntimeState() {
  const disposeRuntimeState = await listen<RuntimeStateDto>("runtime_state_changed", (event) => {
    useAppStore.getState().setRuntimeState(event.payload);
  });
  return () => {
    disposeRuntimeState();
  };
}

export async function runRuntimeCommand<T>(command: () => Promise<T>) {
  try {
    const result = await command();
    const runtimeState = isRuntimeState(result) ? result : await commands.getRuntimeState();
    useAppStore.getState().setRuntimeState(runtimeState);
    useAppStore.getState().setError(null);
    return result;
  } catch (error) {
    useAppStore.getState().setError(String(error));
    throw error;
  }
}

function isRuntimeState(value: unknown): value is RuntimeStateDto {
  return typeof value === "object" && value !== null && "activeBuffer" in value && "preview" in value;
}
