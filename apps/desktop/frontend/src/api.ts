import { blankSnapshot } from "./blankSnapshot";
import type {
  AppSnapshotDto,
  FixtureGuiEditDto,
  LayoutGuiEditDto,
  SequenceGuiEditDto,
  SequenceSelectionEditDto,
  SequenceSelectionEditResultDto
} from "./types";

function snapshot(): Promise<AppSnapshotDto> {
  return Promise.resolve(blankSnapshot());
}

export const commands = {
  getSnapshot: snapshot,
  openProjectDialog: snapshot,
  openProject: (path: string) => {
    void path;
    return snapshot();
  },
  chooseNewProjectParentDirectory: () => Promise.resolve<string | null>(null),
  createNewProject: (parentPath: string, directoryName: string) => {
    void parentPath;
    void directoryName;
    return snapshot();
  },
  openFile: (path: string) => {
    void path;
    return snapshot();
  },
  closeFile: (path: string) => {
    void path;
    return snapshot();
  },
  setActiveFile: (path: string) => {
    void path;
    return snapshot();
  },
  updateActiveText: (text: string) => {
    void text;
    return snapshot();
  },
  setActiveViewMode: (mode: "text" | "gui") => {
    void mode;
    return snapshot();
  },
  undoActiveEdit: snapshot,
  redoActiveEdit: snapshot,
  applySequenceGuiEdit: (edit: SequenceGuiEditDto) => {
    void edit;
    return snapshot();
  },
  applySequenceSelectionEdit: (edit: SequenceSelectionEditDto): Promise<SequenceSelectionEditResultDto> => {
    void edit;
    return Promise.resolve({ snapshot: blankSnapshot(), selection: null, copiedCount: 0, skippedCount: 0 });
  },
  chooseSequenceAudio: snapshot,
  clearSequenceAudio: snapshot,
  exportActiveSequenceFseq: (stepMs: number) => {
    void stepMs;
    return snapshot();
  },
  applyLayoutGuiEdit: (edit: LayoutGuiEditDto) => {
    void edit;
    return snapshot();
  },
  applyFixtureGuiEdit: (edit: FixtureGuiEditDto) => {
    void edit;
    return snapshot();
  },
  flushAutosave: snapshot,
  reloadActiveBufferFromDisk: snapshot,
  keepActiveBuffer: snapshot,
  createFile: (parent: string, name: string) => {
    void parent;
    void name;
    return snapshot();
  },
  createDirectory: (parent: string, name: string) => {
    void parent;
    void name;
    return snapshot();
  },
  renamePath: (path: string, newName: string) => {
    void path;
    void newName;
    return snapshot();
  },
  deletePath: (path: string) => {
    void path;
    return snapshot();
  },
  reloadProject: snapshot,
  toggleProjectTree: snapshot,
  sequenceTransportPlay: snapshot,
  sequenceTransportPause: snapshot,
  sequenceTransportStop: snapshot,
  sequenceTransportRewindToZero: snapshot,
  sequenceTransportSeek: (positionSeconds: number) => {
    void positionSeconds;
    return snapshot();
  },
  setLiveOutputEnabled: (enabled: boolean) => {
    void enabled;
    return snapshot();
  }
};
