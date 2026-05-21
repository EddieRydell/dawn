export type Diagnostic = {
  severity: "Error" | "Warning";
  path: string;
  message: string;
};

export type ProjectState = {
  root: string;
  files: string[];
  diagnostics: Diagnostic[];
};

export type FrameSummary = {
  pixels: number;
  fixtureSpans: number;
  warnings?: string[];
};

export type FileMove = {
  oldPath: string;
  newPath: string;
};

export type FileOperationState = {
  project: ProjectState;
  moved: FileMove[];
};
