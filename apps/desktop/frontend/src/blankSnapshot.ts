import type { AppSnapshotDto } from "./types";

export function blankSnapshot(): AppSnapshotDto {
  return {
    projectRoot: null,
    projectTreeVisible: true,
    projectEntries: [],
    tabs: [],
    activeFile: null,
    activeBuffer: null,
    activeDocumentDescriptor: null,
    activeGuiDocument: null,
    diagnostics: [],
    status: "Ready",
    sequenceTransport: {
      sourceLabel: "No sequence",
      sourceKey: null,
      renderGeneration: 0,
      renderDirtyRevision: 0,
      transportState: "stopped",
      renderUpdating: false,
      positionSeconds: 0,
      homeSeconds: 0,
      durationSeconds: 0,
      audio: null,
      clockSource: "none",
      audioPlaybackStatus: "none",
      geometryIdentity: "",
      status: "Idle"
    },
    liveOutput: {
      enabled: false,
      status: "Disabled",
      activeUniverseCount: 0,
      lastError: null
    }
  };
}
